// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Cross-subsystem aggregator for pending approvals.
//!
//! Surfaces a unified list of [`ApprovalRequest`]s drawn from every Tandem
//! subsystem that owns a pending-approval primitive.
//!
//! Sources: `automation_v2` mission runs whose `checkpoint.awaiting_gate` is
//! set or can be recovered from a pending approval node, and workflow runs
//! paused on an `approval:gate` action (TAN-73). Coder runs will be added
//! once their pause/resume path is wired.
//!
//! The aggregator never mutates state. Decisions still go through the
//! authoritative subsystem handlers (e.g. `automations_v2_run_gate_decide`);
//! a unified `/approvals/{id}/decide` endpoint is intentionally deferred until
//! at least two source subsystems are wired.

use tandem_types::{
    ApprovalDecision, ApprovalListFilter, ApprovalRequest, ApprovalSourceKind, ApprovalTenantRef,
    ApprovalWaitRef,
};

use crate::automation_v2::types::{
    AutomationPendingGate, AutomationRunStatus, AutomationV2RunRecord, AutomationV2Spec,
};
use crate::AppState;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Default cap on returned approvals when no `limit` is supplied.
const DEFAULT_PENDING_LIMIT: usize = 100;
/// Hard upper bound regardless of caller-supplied `limit`.
const MAX_PENDING_LIMIT: usize = 500;

/// Aggregate every pending approval matching `filter`.
///
/// Today this walks automation-v2 run history, including sharded run records.
/// The list is ordered most-recent first by `requested_at_ms`. Surfaces are expected to apply additional
/// per-user filtering (e.g. only show approvals targeting the current user)
/// at the surface layer; this aggregator does tenant filtering only.
pub async fn list_pending_approvals(
    state: &AppState,
    filter: &ApprovalListFilter,
) -> Vec<ApprovalRequest> {
    let limit = filter
        .limit
        .map(|value| (value as usize).min(MAX_PENDING_LIMIT))
        .unwrap_or(DEFAULT_PENDING_LIMIT);

    let mut out: Vec<ApprovalRequest> = Vec::new();

    if filter
        .source
        .as_ref()
        .map(|source| matches!(source, ApprovalSourceKind::AutomationV2))
        .unwrap_or(true)
    {
        let runs = state
            .list_automation_v2_runs_scoped(
                None,
                filter.org_id.as_deref(),
                filter.workspace_id.as_deref(),
                MAX_PENDING_LIMIT,
            )
            .await;
        for listed_run in runs.iter() {
            let run = state
                .get_automation_v2_run(&listed_run.run_id)
                .await
                .unwrap_or_else(|| listed_run.clone());
            if run.status != AutomationRunStatus::AwaitingApproval {
                continue;
            }
            let gate = run.checkpoint.awaiting_gate.clone().or_else(|| {
                run.automation_snapshot
                    .as_ref()
                    .and_then(|automation| recover_automation_v2_pending_gate(&run, automation))
            });
            let Some(gate) = gate else {
                continue;
            };
            if !tenant_matches(filter, &run) {
                continue;
            }
            let action_preview_markdown =
                automation_v2_approval_preview_markdown(state, &run, &gate).await;
            out.push(automation_v2_run_to_approval_request(
                &run,
                &gate,
                action_preview_markdown,
            ));
        }
    }

    if filter
        .source
        .as_ref()
        .map(|source| matches!(source, ApprovalSourceKind::Workflow))
        .unwrap_or(true)
    {
        let mut runs = state
            .workflow_runs
            .read()
            .await
            .values()
            .filter(|run| workflow_tenant_matches(filter, run))
            .cloned()
            .collect::<Vec<_>>();
        runs.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        runs.truncate(MAX_PENDING_LIMIT);
        for run in runs.iter() {
            if run.status != tandem_workflows::WorkflowRunStatus::AwaitingApproval {
                continue;
            }
            let Some(gate) = run.awaiting_gate.as_ref() else {
                continue;
            };
            if !workflow_tenant_matches(filter, run) {
                continue;
            }
            out.push(workflow_run_to_approval_request(run, gate));
        }
    }

    // Future: coder source slots in here.

    out.sort_by(|a, b| b.requested_at_ms.cmp(&a.requested_at_ms));
    out.truncate(limit);
    out
}

fn workflow_tenant_matches(
    filter: &ApprovalListFilter,
    run: &tandem_workflows::WorkflowRunRecord,
) -> bool {
    if let Some(org) = filter.org_id.as_deref() {
        if run.tenant_context.org_id != org {
            return false;
        }
    }
    if let Some(workspace) = filter.workspace_id.as_deref() {
        if run.tenant_context.workspace_id != workspace {
            return false;
        }
    }
    true
}

pub(crate) fn workflow_run_to_approval_request(
    run: &tandem_workflows::WorkflowRunRecord,
    gate: &tandem_workflows::WorkflowPendingGate,
) -> ApprovalRequest {
    // The next non-completed action after the gate is what approval unblocks.
    let action_kind = run
        .actions
        .iter()
        .skip_while(|action| action.action_id != gate.action_id)
        .skip(1)
        .find(|action| action.status != tandem_workflows::WorkflowActionRunStatus::Completed)
        .map(|action| action.action.clone());
    let approval_wait =
        ApprovalWaitRef::for_gate(ApprovalSourceKind::Workflow, &run.run_id, &gate.action_id);
    let mut surface_payload = serde_json::json!({
        "workflow_run_id": run.run_id,
        "workflow_id": run.workflow_id,
        "action_id": gate.action_id,
        "decide_endpoint": format!("/workflows/runs/{}/gate", run.run_id),
        "wait_id": approval_wait.wait_id.clone(),
        "approval_request_id": approval_wait.approval_request_id.clone(),
    });
    if let Some(transition_id) = approval_wait.transition_id.as_ref() {
        surface_payload["transition_id"] = serde_json::json!(transition_id);
    }
    ApprovalRequest {
        request_id: approval_wait.approval_request_id.clone(),
        approval_wait: Some(approval_wait),
        source: ApprovalSourceKind::Workflow,
        tenant: ApprovalTenantRef {
            org_id: run.tenant_context.org_id.clone(),
            workspace_id: run.tenant_context.workspace_id.clone(),
            user_id: run.tenant_context.actor_id.clone(),
        },
        run_id: run.run_id.clone(),
        node_id: Some(gate.action_id.clone()),
        workflow_name: Some(run.workflow_id.clone()),
        action_kind,
        action_preview_markdown: None,
        surface_payload: Some(surface_payload),
        requested_at_ms: gate.requested_at_ms,
        expires_at_ms: None,
        decisions: gate
            .decisions
            .iter()
            .filter_map(|raw| approval_decision_from_gate(raw))
            .collect(),
        rework_targets: gate.rework_targets.clone(),
        instructions: gate.instructions.clone(),
        decided_by: None,
        decided_at_ms: None,
        decision: None,
        rework_feedback: None,
    }
}

fn recover_automation_v2_pending_gate(
    run: &AutomationV2RunRecord,
    automation: &AutomationV2Spec,
) -> Option<AutomationPendingGate> {
    let pending_nodes = run
        .checkpoint
        .pending_nodes
        .iter()
        .collect::<std::collections::HashSet<_>>();
    automation
        .flow
        .nodes
        .iter()
        .find(|node| {
            pending_nodes.contains(&node.node_id)
                && !crate::app::state::automation_gate_has_settled_decision(run, &node.node_id)
                && crate::app::state::is_automation_approval_node(node)
        })
        .and_then(crate::app::state::build_automation_pending_gate)
        .map(|mut gate| {
            gate.requested_at_ms = run.updated_at_ms.max(run.created_at_ms);
            gate
        })
}

fn tenant_matches(filter: &ApprovalListFilter, run: &AutomationV2RunRecord) -> bool {
    if let Some(org) = filter.org_id.as_deref() {
        if run.tenant_context.org_id != org {
            return false;
        }
    }
    if let Some(workspace) = filter.workspace_id.as_deref() {
        if run.tenant_context.workspace_id != workspace {
            return false;
        }
    }
    true
}

pub(crate) fn automation_v2_run_to_approval_request(
    run: &AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    action_preview_markdown: Option<String>,
) -> ApprovalRequest {
    let workflow_name = run
        .automation_snapshot
        .as_ref()
        .map(|snap| snap.name.clone())
        .or_else(|| Some(run.automation_id.clone()));

    let action_kind = run.automation_snapshot.as_ref().and_then(|snap| {
        snap.flow
            .nodes
            .iter()
            .find(|node| node.node_id == gate.node_id)
            .map(|node| node.objective.clone())
    });

    let decisions = approval_decisions_for_gate(gate);
    let expires_at_ms = crate::app::state::automation_gate_expires_at_ms(gate);
    let approval_wait =
        ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id);
    let mut surface_payload = serde_json::json!({
        "automation_v2_run_id": run.run_id,
        "automation_id": run.automation_id,
        "node_id": gate.node_id,
        "decide_endpoint": format!(
            "/automations/v2/runs/{}/gate",
            run.run_id
        ),
        "wait_id": approval_wait.wait_id.clone(),
        "approval_request_id": approval_wait.approval_request_id.clone(),
    });
    if let Some(transition_id) = approval_wait.transition_id.as_ref() {
        surface_payload["transition_id"] = serde_json::json!(transition_id);
    }
    if let Some(expires_at_ms) = expires_at_ms {
        surface_payload["expires_at_ms"] = serde_json::json!(expires_at_ms);
    }
    if let Some(policy) = gate.expiry_policy.as_ref() {
        surface_payload["expiry_policy"] = serde_json::json!(policy);
    }
    if let Some(policy_state) = gate
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("gate_policy_state"))
    {
        surface_payload["gate_policy_state"] = policy_state.clone();
        if let Some(notification_key) = policy_state
            .get("notification_key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            surface_payload["notification_key"] = serde_json::json!(notification_key);
        }
    }

    ApprovalRequest {
        request_id: approval_wait.approval_request_id.clone(),
        approval_wait: Some(approval_wait),
        source: ApprovalSourceKind::AutomationV2,
        tenant: ApprovalTenantRef {
            org_id: run.tenant_context.org_id.clone(),
            workspace_id: run.tenant_context.workspace_id.clone(),
            user_id: run.tenant_context.actor_id.clone(),
        },
        run_id: run.run_id.clone(),
        node_id: Some(gate.node_id.clone()),
        workflow_name,
        action_kind,
        action_preview_markdown,
        surface_payload: Some(surface_payload),
        requested_at_ms: gate.requested_at_ms,
        expires_at_ms,
        decisions,
        rework_targets: gate.rework_targets.clone(),
        instructions: gate.instructions.clone(),
        decided_by: None,
        decided_at_ms: None,
        decision: None,
        rework_feedback: None,
    }
}

fn approval_decisions_for_gate(gate: &AutomationPendingGate) -> Vec<ApprovalDecision> {
    let mut decisions = gate
        .decisions
        .iter()
        .filter_map(|raw| approval_decision_from_gate(raw))
        .collect::<Vec<_>>();
    if !gate.rework_targets.is_empty() && !decisions.contains(&ApprovalDecision::Rework) {
        decisions.push(ApprovalDecision::Rework);
    }
    decisions
}

fn approval_decision_from_gate(raw: &str) -> Option<ApprovalDecision> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "approve" => Some(ApprovalDecision::Approve),
        "rework" | "changes" | "request_changes" | "ask_changes" => Some(ApprovalDecision::Rework),
        "cancel" | "reject" | "deny" => Some(ApprovalDecision::Cancel),
        _ => None,
    }
}

async fn automation_v2_approval_preview_markdown(
    state: &AppState,
    run: &AutomationV2RunRecord,
    gate: &AutomationPendingGate,
) -> Option<String> {
    let automation = run.automation_snapshot.as_ref();
    let workspace_root = match automation.and_then(|snapshot| snapshot.workspace_root.clone()) {
        Some(root) => root,
        None => state.workspace_index.snapshot().await.root,
    };
    let workspace_root = PathBuf::from(workspace_root);
    let mut sections = Vec::new();

    for node_id in &gate.upstream_node_ids {
        if !is_safe_artifact_node_id(node_id) {
            continue;
        }
        if let Some(output) = run.checkpoint.node_outputs.get(node_id) {
            if let Some(text) = output.pointer("/content/text").and_then(Value::as_str) {
                let digest = output
                    .pointer("/provenance/content_digest")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| crate::sha256_hex(&[text]));
                sections.push(format!(
                    "#### `{node_id}`\n\nContent digest: `{digest}`\n\n{}",
                    crate::app::state::truncate_text(text, 12_000)
                ));
                continue;
            }
        }
        let artifact_path = workspace_root
            .join(".tandem")
            .join("runs")
            .join(&run.run_id)
            .join("artifacts")
            .join(format!("{node_id}.json"));
        let Ok(raw) = tokio::fs::read_to_string(&artifact_path).await else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        if let Some(section) = approval_artifact_preview_section(node_id, &value) {
            sections.push(section);
        }
    }

    if sections.is_empty() {
        return None;
    }

    let mut markdown = String::from("### Approval Evidence\n\n");
    markdown.push_str(&sections.join("\n\n"));
    Some(markdown)
}

fn is_safe_artifact_node_id(node_id: &str) -> bool {
    !node_id.is_empty()
        && node_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn approval_artifact_preview_section(node_id: &str, value: &Value) -> Option<String> {
    let mut section = String::new();
    let _ = writeln!(section, "#### `{node_id}`");

    if let Some(rows) = value.get("ready_to_write").and_then(Value::as_array) {
        let has_rows = value
            .get("has_rows_to_write")
            .and_then(Value::as_bool)
            .unwrap_or(!rows.is_empty());
        let _ = writeln!(
            section,
            "- Proposed contact rows: **{}**{}",
            rows.len(),
            if has_rows {
                ""
            } else {
                " (contact writer should no-op)"
            }
        );
        if !has_rows {
            let _ = writeln!(
                section,
                "- Company Research Status updates are still expected for every selected company."
            );
        }
        append_contact_rows_preview(&mut section, rows);
        return Some(section);
    }

    if let Some(scored) = value.get("scored_by_company").and_then(Value::as_array) {
        let selected_count: usize = scored
            .iter()
            .map(|company| array_len(company, "selected_contacts") + array_len(company, "contacts"))
            .sum();
        let _ = writeln!(
            section,
            "- High-value contacts selected: **{}**",
            selected_count
        );
        if selected_count == 0 {
            let _ = writeln!(
                section,
                "- Approval will not write contacts unless later artifacts contain rows."
            );
        }
        return Some(section);
    }

    if let Some(companies) = value.get("candidates_by_company").and_then(Value::as_array) {
        let candidate_count: usize = companies
            .iter()
            .map(|company| {
                company
                    .get("candidate_count")
                    .and_then(Value::as_u64)
                    .map(|count| count as usize)
                    .unwrap_or_else(|| array_len(company, "candidates"))
            })
            .sum();
        let company_names = companies
            .iter()
            .filter_map(|company| company.get("company").and_then(Value::as_str))
            .take(8)
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            section,
            "- Candidate contacts found: **{}**",
            candidate_count
        );
        if !company_names.is_empty() {
            let _ = writeln!(section, "- Companies checked: {company_names}");
        }
        if candidate_count == 0 {
            let status_notes = companies
                .iter()
                .filter_map(company_status_preview)
                .take(8)
                .collect::<Vec<_>>();
            if !status_notes.is_empty() {
                let _ = writeln!(
                    section,
                    "- Company Research Status outcomes to record: {}",
                    status_notes.join("; ")
                );
            }
        }
        return Some(section);
    }

    if let Some(companies) = value.get("selected_companies").and_then(Value::as_array) {
        let company_names = companies
            .iter()
            .filter_map(|company| company.get("company").and_then(Value::as_str))
            .take(8)
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(section, "- Companies in batch: **{}**", companies.len());
        if !company_names.is_empty() {
            let _ = writeln!(section, "- Selected: {company_names}");
        }
        return Some(section);
    }

    None
}

fn append_contact_rows_preview(section: &mut String, rows: &[Value]) {
    if rows.is_empty() {
        let _ = writeln!(section, "- No contact rows are ready to write.");
        return;
    }

    section.push_str("\n| Company | Contact | Role | Email | Status |\n");
    section.push_str("| --- | --- | --- | --- | --- |\n");
    for row in rows.iter().take(10) {
        let company = markdown_cell(first_string(row, &["Company", "company"]));
        let contact = markdown_cell(first_string(
            row,
            &["Contact name", "contact_name", "name", "Contact / Lead"],
        ));
        let role = markdown_cell(first_string(row, &["Role / Title", "role_title", "title"]));
        let email = markdown_cell(first_string(row, &["Email", "email"]));
        let status = markdown_cell(first_string(row, &["Status", "status"]));
        let _ = writeln!(
            section,
            "| {company} | {contact} | {role} | {email} | {status} |"
        );
    }
    if rows.len() > 10 {
        let _ = writeln!(section, "\n_Showing 10 of {} proposed rows._", rows.len());
    }
}

fn company_status_preview(company: &Value) -> Option<String> {
    let name = company.get("company").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let status = match company
        .get("domain_resolution_status")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "not_found" | "ambiguous" => "no_domain",
        "tool_failed" => "retry_later",
        _ => {
            let candidate_count = company
                .get("candidate_count")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| array_len(company, "candidates") as u64);
            let hunter_checked = company
                .get("hunter_checked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if hunter_checked && candidate_count == 0 {
                "no_hunter_results"
            } else if candidate_count == 0 {
                "no_relevant_contacts"
            } else {
                "contacts_found"
            }
        }
    };
    Some(format!("{name} -> {status}"))
}

fn first_string<'a>(value: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or("")
}

fn markdown_cell(value: &str) -> String {
    let escaped = value.replace('|', "\\|").replace('\n', " ");
    if escaped.trim().is_empty() {
        "-".to_string()
    } else {
        escaped
    }
}

fn array_len(value: &Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}
