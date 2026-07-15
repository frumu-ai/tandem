// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Dry-run runner for production-mirroring adversarial scenario packs (TAN-487).
//!
//! Each scenario is evaluated against the destination router's route preview,
//! approval gate, and readiness gates using the operator's live config. The
//! runner never mutates external systems — it only reads config and computes a
//! route preview. A scenario "passes" when the control behavior the operator's
//! configuration produces matches the scenario's expectation; a failing scenario
//! surfaces a governance gap.

use serde_json::{json, Value};
use tandem_types::TenantContext;

use crate::{IncidentMonitorScenario, IncidentMonitorScenarioPack, IncidentMonitorStatus};

/// Run every scenario in a pack in dry-run mode and return one result per
/// scenario, plus a summary. `mutates_external_systems` is always false.
pub fn run_incident_monitor_scenario_pack(
    status: &IncidentMonitorStatus,
    tenant_context: &TenantContext,
    pack: &IncidentMonitorScenarioPack,
) -> Value {
    let results = pack
        .scenarios
        .iter()
        .map(|scenario| run_incident_monitor_scenario(status, tenant_context, scenario))
        .collect::<Vec<_>>();
    let count = |status_value: &str| {
        results
            .iter()
            .filter(|row| row.get("status").and_then(Value::as_str) == Some(status_value))
            .count()
    };
    json!({
        "pack_id": pack.pack_id,
        "version": pack.version,
        "description": pack.description,
        "mode": "dry_run",
        "mutates_external_systems": false,
        "counts": {
            "total": results.len(),
            "passed": count("pass"),
            "failed": count("fail"),
            "blocked": count("blocked"),
        },
        "results": results,
    })
}

/// Evaluate a single scenario against the live route preview.
pub fn run_incident_monitor_scenario(
    status: &IncidentMonitorStatus,
    _tenant_context: &TenantContext,
    scenario: &IncidentMonitorScenario,
) -> Value {
    // Operator-authored test input: set the routing signals directly (like the
    // report's own route-preview helper) so source-kind / tenant / risk all
    // reach route matching and the approval/readiness gates.
    let input = &scenario.input;
    let mut context = crate::incident_monitor::router::build_route_context(
        input.event_type.as_deref(),
        input.source.as_deref(),
        None,
        input.risk_level.as_deref(),
        input.risk_category.as_deref(),
        input.confidence.as_deref(),
        input.expected_destination.as_deref(),
        input.project_id.as_deref(),
        input.log_source_id.as_deref(),
        &input.route_tags,
        None,
        None,
        None,
    );
    context.source_kind = input.source_kind.clone();
    context.tenant_id = input.tenant_id.clone();
    context.workspace_id = input.workspace_id.clone();
    let preview = crate::incident_monitor::router::build_route_preview(
        &status.config,
        &status.destinations,
        &status.destination_readiness,
        &status.source_readiness,
        &context,
        &scenario.input.requested_destination_ids,
    );

    let expect = &scenario.expect;
    let mut assertions = Vec::new();
    let mut all_ok = true;
    // An approval assertion is only meaningful when the publish is actually
    // routable: no routable destination, or a preview the readiness/allowlist
    // gate already blocked, means the approval path was not validated and the
    // case is reported blocked (not evaluated) rather than a false pass.
    let mut not_evaluable = false;

    if let Some(expected_blocked) = expect.blocked {
        let actual = preview.blocked;
        let ok = actual == expected_blocked;
        all_ok &= ok;
        assertions.push(assertion(
            "blocked",
            json!(expected_blocked),
            json!(actual),
            ok,
        ));
    }
    if let Some(expected_approval) = expect.approval_required {
        // A scenario that asserts approval but not a block cannot be evaluated
        // when the publish would be blocked or has no routable destination.
        if expect.blocked != Some(true)
            && (preview.effective_destination_ids.is_empty() || preview.blocked)
        {
            not_evaluable = true;
        }
        let actual = preview.approval_required;
        let ok = actual == expected_approval;
        all_ok &= ok;
        assertions.push(assertion(
            "approval_required",
            json!(expected_approval),
            json!(actual),
            ok,
        ));
    }
    if let Some(reason) = expect.reason_contains.as_deref() {
        let actual = preview
            .blocked_reasons
            .iter()
            .any(|row| row.contains(reason));
        all_ok &= actual;
        assertions.push(assertion(
            "reason_contains",
            json!(reason),
            json!(actual),
            actual,
        ));
    }
    if let Some(destination_id) = expect.effective_destination_id.as_deref() {
        let actual = preview
            .effective_destination_ids
            .first()
            .map(String::as_str);
        let ok = actual == Some(destination_id);
        all_ok &= ok;
        assertions.push(assertion(
            "effective_destination_id",
            json!(destination_id),
            json!(actual),
            ok,
        ));
    }

    let status_str = if not_evaluable {
        "blocked"
    } else if all_ok {
        "pass"
    } else {
        "fail"
    };

    let expected_behavior = scenario
        .expect
        .note
        .clone()
        .unwrap_or_else(|| format!("Scenario `{}` control expectation", scenario.scenario_id));
    let observed_behavior = if not_evaluable {
        "No routable destination is configured, so the approval gate could not be evaluated in dry-run.".to_string()
    } else {
        format!(
            "route preview: blocked={}, approval_required={}, effective_destinations={:?}, reasons={:?}",
            preview.blocked,
            preview.approval_required,
            preview.effective_destination_ids,
            preview.blocked_reasons,
        )
    };

    let hash = crate::sha256_hex(&[
        scenario.scenario_id.as_str(),
        scenario.category.as_str(),
        status_str,
    ]);
    let finding_id = (status_str == "fail").then(|| format!("asp_{}", &hash[..hash.len().min(16)]));

    json!({
        "scenario_id": scenario.scenario_id,
        "category": scenario.category,
        "description": scenario.description,
        "status": status_str,
        "passed": status_str == "pass",
        "expected_behavior": expected_behavior,
        "observed_behavior": observed_behavior,
        "assertions": assertions,
        "route_preview": {
            "requested_destination_ids": scenario.input.requested_destination_ids,
            "effective_destination_ids": preview.effective_destination_ids,
            "approval_required": preview.approval_required,
            "blocked": preview.blocked,
            "blocked_reasons": preview.blocked_reasons,
        },
        "finding_id": finding_id,
        "evidence_refs": [
            json!({"kind": "scenario_pack", "id": scenario.scenario_id}),
        ],
        "prompt_injection": scenario.input.prompt_injection,
        "dry_run": true,
        "mutates_external_systems": false,
    })
}

fn assertion(name: &str, expected: Value, actual: Value, ok: bool) -> Value {
    json!({ "name": name, "expected": expected, "actual": actual, "ok": ok })
}
