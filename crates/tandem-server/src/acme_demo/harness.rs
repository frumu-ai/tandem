//! Deterministic end-to-end ACME Slack demo harness (TAN-667).
//!
//! The live Slack Events endpoint establishes signed/allowlisted ingress. This
//! harness gives the demo a resettable replay surface for the same prompt and the
//! five seeded Slack users without depending on Slack delivery timing or an LLM.
//! It consumes the TAN-655 dataset and renders the governance receipt contract
//! the control panel expects (`run`, `actors`, `tool_manifest`, decisions,
//! approvals, memory audit, protected events, redactions, final response).

use serde_json::{json, Value};
use tandem_core::tool_schema_risk_tier;
use tandem_types::{DataClass, PolicyDecisionEffect};

use super::{
    acme_demo_dataset, profile_can_read_memory, profile_can_use_tool, profile_holds_resource_grant,
    AcmeDemoDataset, DemoMemoryRow, DemoProfile, DEMO_BASE_NOW_MS, DEMO_ORG_ID, DEMO_PROMPT,
    DEMO_TAXONOMY_ID, DEMO_WORKSPACE_ID,
};

pub const DEMO_SLACK_TEAM_ID: &str = "T_ACME_HQ";
pub const DEMO_SLACK_WORKSPACE_NAME: &str = "acme-hq";
pub const DEMO_SLACK_CHANNEL_ID: &str = "C_ACME_DEMO";
pub const DEMO_SLACK_CHANNEL_NAME: &str = "acme-governance-demo";

/// Run the deterministic five-profile Slack governance demo and return the
/// receipt bundle. This is the harness behind the documented single-command
/// test flow:
///
/// `cargo test -p tandem-server acme_slack_demo_harness --lib`
pub fn run_acme_slack_demo_harness() -> Value {
    let dataset = acme_demo_dataset();
    let runs = dataset
        .profiles
        .iter()
        .map(|profile| acme_slack_demo_receipt_for_profile(&dataset, profile))
        .collect::<Vec<_>>();

    json!({
        "schema_version": 1,
        "harness": "acme_slack_governance_demo",
        "prompt": DEMO_PROMPT,
        "reset_mode": "pure_seeded_fixture",
        "slack": {
            "workspace_id": DEMO_SLACK_TEAM_ID,
            "workspace_name": DEMO_SLACK_WORKSPACE_NAME,
            "channel_id": DEMO_SLACK_CHANNEL_ID,
            "channel_name": DEMO_SLACK_CHANNEL_NAME,
        },
        "tenant": {
            "org_id": DEMO_ORG_ID,
            "workspace_id": DEMO_WORKSPACE_ID,
            "taxonomy_id": DEMO_TAXONOMY_ID,
        },
        "profile_count": runs.len(),
        "runs": runs,
    })
}

/// Build the control-panel-compatible governance receipt for one Slack profile.
pub fn acme_slack_demo_receipt_for_profile(
    dataset: &AcmeDemoDataset,
    profile: &DemoProfile,
) -> Value {
    let run_id = format!("acme-slack-demo-{}", profile.unit_id);
    let context_run_id = format!("ctx-{run_id}");
    let role = profile_role(profile);
    let grants = profile_grants(dataset, profile);
    let (returned_memory, hidden_memory) = memory_scope_rows(dataset, profile);
    let (offered_tools, hidden_tools) = tool_scope_rows(dataset, profile);
    let used_tools = used_tools_for_profile(profile);
    let policy_decisions = policy_decisions_for_profile(
        dataset,
        profile,
        &run_id,
        &context_run_id,
        &role,
        &offered_tools,
        &hidden_tools,
    );
    let approval_required_events =
        approval_required_events_for_profile(dataset, profile, &run_id, &used_tools);
    let redactions = redactions_for_hidden_memory(&hidden_memory);
    let final_response = final_slack_response(profile);
    let memory_audit = memory_audit_rows(profile, &run_id, &returned_memory, &hidden_memory);
    let protected_events = protected_audit_rows(profile, &run_id, &role, &final_response);
    let tool_manifest = tool_manifest(&offered_tools, &hidden_tools, &used_tools);

    json!({
        "run_id": run_id,
        "context_run_id": context_run_id,
        "profile": {
            "display_name": profile.display_name,
            "slack_user_id": profile.slack_user_id,
            "unit_id": profile.unit_id,
            "org_unit": profile.owner_org_unit_id(),
            "role": role,
        },
        "slack_request": {
            "workspace_id": DEMO_SLACK_TEAM_ID,
            "workspace_name": DEMO_SLACK_WORKSPACE_NAME,
            "channel_id": DEMO_SLACK_CHANNEL_ID,
            "channel_name": DEMO_SLACK_CHANNEL_NAME,
            "user_id": profile.slack_user_id,
            "event_id": format!("Ev-{}", profile.unit_id),
            "text": DEMO_PROMPT,
        },
        "resolved_tandem_principal": {
            "actor_id": profile.actor_id,
            "source": "channel:slack",
            "principal": profile.principal,
        },
        "verified_tenant_context": {
            "tenant_context": dataset.tenant_context,
            "human_actor": {
                "id": profile.actor_id,
                "source": "slack",
                "display_name": profile.display_name,
            },
            "org_units": profile.org_units(),
            "roles": [role],
            "strict_projection": {
                "principal": profile.unit_principal,
                "grants": grants,
            },
        },
        "memory": {
            "scopes_queried": memory_scopes_queried(dataset),
            "returned": returned_memory,
            "hidden_by_scope": hidden_memory,
            "denied_or_hidden": hidden_memory,
        },
        "tools": {
            "offered": offered_tools,
            "hidden_by_scope": hidden_tools,
            "used": used_tools,
        },
        "policy_decisions": policy_decisions,
        "approvals": {
            "approval_required_events": approval_required_events,
            "pending_gate": Value::Null,
            "gate_history": approval_required_events,
        },
        "redactions": redactions,
        "final_slack_visible_response": final_response,
        "control_panel_receipt": control_panel_receipt(
            profile,
            &run_id,
            &context_run_id,
            &role,
            &tool_manifest,
            &policy_decisions,
            &approval_required_events,
            &memory_audit,
            &protected_events,
            &redactions,
            &final_response,
        ),
    })
}

fn profile_role(profile: &DemoProfile) -> String {
    match profile.unit_id {
        "sales" => "sales.account_viewer",
        "engineering" => "engineering.delivery_viewer",
        "finance" => "finance.financial_record_viewer",
        "leadership" => "leadership.cross_functional_viewer",
        "contractor_acme_x" => "contractor.project_x_viewer",
        other => return format!("demo.{other}"),
    }
    .to_string()
}

fn profile_grants(dataset: &AcmeDemoDataset, profile: &DemoProfile) -> Vec<Value> {
    let mut rows = dataset
        .graph
        .effective_grants(&profile.principal, DEMO_BASE_NOW_MS)
        .into_iter()
        .map(|grant| {
            json!({
                "grant_id": grant.grant_id,
                "effect": grant.effect,
                "resource": grant.resource,
                "permissions": grant.permissions,
                "data_classes": grant.data_classes,
                "tool_patterns": grant.tool_patterns,
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| value_str(a, "grant_id").cmp(&value_str(b, "grant_id")));
    rows
}

fn memory_scopes_queried(dataset: &AcmeDemoDataset) -> Vec<Value> {
    let mut rows = dataset
        .memory_rows
        .iter()
        .map(|row| {
            json!({
                "memory_id": row.id,
                "resource": row.resource,
                "owner_org_unit_id": row.owner_org_unit_id,
                "data_class": row.data_class,
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| value_str(a, "memory_id").cmp(&value_str(b, "memory_id")));
    rows
}

fn memory_scope_rows(dataset: &AcmeDemoDataset, profile: &DemoProfile) -> (Vec<Value>, Vec<Value>) {
    let mut returned = Vec::new();
    let mut hidden = Vec::new();
    for row in &dataset.memory_rows {
        if profile_can_read_memory(profile, row, DEMO_BASE_NOW_MS) {
            returned.push(json!({
                "memory_id": row.id,
                "owner_org_unit_id": row.owner_org_unit_id,
                "resource": row.resource,
                "data_class": row.data_class,
                "summary": row.summary,
            }));
        } else {
            hidden.push(hidden_memory_row(dataset, profile, row));
        }
    }
    returned.sort_by(|a, b| value_str(a, "memory_id").cmp(&value_str(b, "memory_id")));
    hidden.sort_by(|a, b| value_str(a, "memory_id").cmp(&value_str(b, "memory_id")));
    (returned, hidden)
}

fn hidden_memory_row(
    dataset: &AcmeDemoDataset,
    profile: &DemoProfile,
    row: &DemoMemoryRow,
) -> Value {
    let has_clearance = profile_holds_resource_grant(
        dataset,
        profile,
        &row.resource,
        row.data_class,
        DEMO_BASE_NOW_MS,
    );
    let reason = if row.data_class == DataClass::Credential {
        "credential_hidden"
    } else if row.data_class == DataClass::FinancialRecord && profile.unit_id != "finance" {
        "financial_record_not_granted"
    } else if has_clearance {
        "department_scope_hidden"
    } else {
        "resource_grant_not_present"
    };
    json!({
        "memory_id": row.id,
        "owner_org_unit_id": row.owner_org_unit_id,
        "resource": row.resource,
        "data_class": row.data_class,
        "reason": reason,
    })
}

fn tool_scope_rows(dataset: &AcmeDemoDataset, profile: &DemoProfile) -> (Vec<String>, Vec<String>) {
    let mut offered = Vec::new();
    let mut hidden = Vec::new();
    for tool in &dataset.tools {
        if profile_can_use_tool(dataset, profile, tool, DEMO_BASE_NOW_MS) {
            offered.push(tool.schema.name.clone());
        } else {
            hidden.push(tool.schema.name.clone());
        }
    }
    offered.sort();
    hidden.sort();
    (offered, hidden)
}

fn used_tools_for_profile(profile: &DemoProfile) -> Vec<String> {
    let names: &[&str] = match profile.unit_id {
        "sales" => &["mcp.crm.search_accounts", "mcp.support.list_summaries"],
        "engineering" => &[
            "mcp.github.read_repo",
            "mcp.incidents.list_incidents",
            "mcp.linear.list_issues",
        ],
        "finance" => &["mcp.contracts.read_contracts", "mcp.invoices.read_invoices"],
        "leadership" => &[
            "mcp.crm.search_accounts",
            "mcp.incidents.list_incidents",
            "mcp.linear.list_issues",
        ],
        "contractor_acme_x" => &["mcp.projects.x.read_spec"],
        _ => &[],
    };
    let mut rows = names
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

fn policy_decisions_for_profile(
    dataset: &AcmeDemoDataset,
    profile: &DemoProfile,
    run_id: &str,
    context_run_id: &str,
    role: &str,
    offered_tools: &[String],
    hidden_tools: &[String],
) -> Vec<Value> {
    let mut rows = Vec::new();
    for tool in &dataset.tools {
        let name = &tool.schema.name;
        let (decision, reason_code, reason) = if offered_tools.contains(name) {
            if tool.approval_required() {
                (
                    PolicyDecisionEffect::ApprovalRequired,
                    "approval_required_by_risk_tier",
                    "Tool is in scope, but the tool risk tier requires approval before execution.",
                )
            } else {
                (
                    PolicyDecisionEffect::Allow,
                    "tool_offered_by_scope",
                    "Tool is available through the requester's department grant.",
                )
            }
        } else if hidden_tools.contains(name) {
            (
                PolicyDecisionEffect::Deny,
                "hidden_by_scope",
                "Tool is not exposed to this requester because no department grant matches it.",
            )
        } else {
            continue;
        };
        rows.push(json!({
            "decision_id": format!("pd-{run_id}-{}", sanitize_id(name)),
            "requester_context": requester_context(profile, role),
            "actor_id": profile.actor_id,
            "session_id": format!("session-{run_id}"),
            "message_id": format!("msg-{run_id}"),
            "run_id": run_id,
            "context_run_id": context_run_id,
            "tool": name,
            "data_classes": tool.schema.security.data_classes,
            "risk_tier": tool_schema_risk_tier(&tool.schema).as_str(),
            "decision": decision,
            "reason_code": reason_code,
            "reason": reason,
            "created_at_ms": DEMO_BASE_NOW_MS,
        }));
    }
    rows.sort_by(|a, b| value_str(a, "decision_id").cmp(&value_str(b, "decision_id")));
    rows
}

fn approval_required_events_for_profile(
    dataset: &AcmeDemoDataset,
    profile: &DemoProfile,
    run_id: &str,
    used_tools: &[String],
) -> Vec<Value> {
    let mut rows = dataset
        .tools
        .iter()
        .filter(|tool| used_tools.contains(&tool.schema.name) && tool.approval_required())
        .map(|tool| {
            json!({
                "approval_id": format!("approval-{run_id}-{}", sanitize_id(&tool.schema.name)),
                "run_id": run_id,
                "tool": tool.schema.name,
                "risk_tier": tool_schema_risk_tier(&tool.schema).as_str(),
                "decision": "approval_required",
                "reason": "risk tier requires an approval gate before the tool can execute",
                "requested_at_ms": DEMO_BASE_NOW_MS,
                "requester_org_units": profile.org_units(),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| value_str(a, "approval_id").cmp(&value_str(b, "approval_id")));
    rows
}

fn redactions_for_hidden_memory(hidden_memory: &[Value]) -> Vec<Value> {
    hidden_memory
        .iter()
        .filter(|row| {
            row.get("data_class").is_some_and(|class| {
                class == &json!(DataClass::FinancialRecord)
                    || class == &json!(DataClass::Credential)
            })
        })
        .map(|row| {
            json!({
                "field": "memory.summary",
                "memory_id": row["memory_id"],
                "data_class": row["data_class"],
                "reason": row["reason"],
                "replacement": "[redacted]",
            })
        })
        .collect()
}

fn memory_audit_rows(
    profile: &DemoProfile,
    run_id: &str,
    returned_memory: &[Value],
    hidden_memory: &[Value],
) -> Vec<Value> {
    vec![json!({
        "audit_id": format!("mem-audit-{run_id}"),
        "action": "slack_demo_memory_query",
        "run_id": run_id,
        "memory_id": Value::Null,
        "source_memory_id": Value::Null,
        "to_tier": Value::Null,
        "partition_key": format!("tenant:{DEMO_ORG_ID}/{DEMO_WORKSPACE_ID}:org_unit:{}", profile.owner_org_unit_id()),
        "actor": profile.actor_id,
        "status": "scoped",
        "returned_memory_ids": ids_from_rows(returned_memory, "memory_id"),
        "hidden_memory_ids": ids_from_rows(hidden_memory, "memory_id"),
        "created_at_ms": DEMO_BASE_NOW_MS,
    })]
}

fn protected_audit_rows(
    profile: &DemoProfile,
    run_id: &str,
    role: &str,
    final_response: &str,
) -> Vec<Value> {
    vec![
        json!({
            "event_id": format!("audit-{run_id}-ingress"),
            "event_type": "slack.demo.ingress",
            "requester_context": requester_context(profile, role),
            "actor": profile.actor_id,
            "created_at_ms": DEMO_BASE_NOW_MS,
            "payload": {
                "slack_workspace_id": DEMO_SLACK_TEAM_ID,
                "slack_channel_id": DEMO_SLACK_CHANNEL_ID,
                "slack_user_id": profile.slack_user_id,
                "prompt_sha256": crate::sha256_hex(&[DEMO_PROMPT]),
            },
        }),
        json!({
            "event_id": format!("audit-{run_id}-response"),
            "event_type": "slack.demo.response",
            "requester_context": requester_context(profile, role),
            "actor": profile.actor_id,
            "created_at_ms": DEMO_BASE_NOW_MS + 1,
            "payload": {
                "response_sha256": crate::sha256_hex(&[final_response]),
                "slack_visible": true,
            },
        }),
    ]
}

fn tool_manifest(offered: &[String], hidden: &[String], used: &[String]) -> Value {
    let used_unoffered = used
        .iter()
        .filter(|tool| !offered.contains(*tool))
        .cloned()
        .collect::<Vec<_>>();
    json!({
        "offered": offered,
        "used": used,
        "hidden_by_scope": hidden,
        "used_subset_offered": used_unoffered.is_empty(),
        "used_unoffered": used_unoffered,
    })
}

#[allow(clippy::too_many_arguments)]
fn control_panel_receipt(
    profile: &DemoProfile,
    run_id: &str,
    context_run_id: &str,
    role: &str,
    tool_manifest: &Value,
    policy_decisions: &[Value],
    approval_required_events: &[Value],
    memory_audit: &[Value],
    protected_events: &[Value],
    redactions: &[Value],
    final_response: &str,
) -> Value {
    json!({
        "ledger": {
            "records": used_tool_ledger_records(run_id, context_run_id, tool_manifest),
            "summary": {
                "record_count": tool_manifest["used"].as_array().map(Vec::len).unwrap_or_default(),
            },
            "tool_manifest": tool_manifest,
        },
        "evidence_package": {
            "schema_version": 1,
            "package_type": "tandem_run_governance_evidence",
            "run": {
                "run_id": run_id,
                "context_run_id": context_run_id,
                "run_type": "slack_demo_harness",
                "goal": DEMO_PROMPT,
                "goal_sha256": crate::sha256_hex(&[DEMO_PROMPT]),
                "tenant_context": {
                    "org_id": DEMO_ORG_ID,
                    "workspace_id": DEMO_WORKSPACE_ID,
                    "actor_id": profile.actor_id,
                },
                "counts": {
                    "tool_calls": tool_manifest["used"].as_array().map(Vec::len).unwrap_or_default(),
                    "policy_decisions": policy_decisions.len(),
                    "approval_records": approval_required_events.len(),
                    "memory_audit_records": memory_audit.len(),
                    "protected_audit_records": protected_events.len(),
                    "redactions": redactions.len(),
                },
            },
            "actors": {
                "tenant_actor_id": profile.actor_id,
                "policy_actor_ids": [profile.actor_id.clone()],
                "memory_actor_ids": [profile.actor_id.clone()],
                "requester_org_units": profile.org_units(),
                "requester_roles": [role],
                "approval_deciders": [],
            },
            "tool_manifest": tool_manifest,
            "policy_decisions": policy_decisions,
            "approvals": {
                "pending_gate": Value::Null,
                "gate_history": approval_required_events,
                "approval_required_events": approval_required_events,
            },
            "memory_audit": memory_audit,
            "audit": {
                "protected_events": protected_events,
            },
            "redactions": redactions,
            "redaction_policy": {
                "memory_content": "department_scoped_or_redacted",
                "financial_records": "finance_only",
                "credentials": "never_surface",
                "slack_response": "only_slack_visible_summary",
            },
            "final_outcome": {
                "context_status": "completed",
                "automation_status": "completed",
                "slack_visible_response": final_response,
            },
            "limitations": [],
            "artifacts": [],
        },
    })
}

fn used_tool_ledger_records(
    run_id: &str,
    context_run_id: &str,
    tool_manifest: &Value,
) -> Vec<Value> {
    tool_manifest["used"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(idx, tool)| {
            json!({
                "seq": idx + 1,
                "event_id": format!("ledger-{run_id}-{}", idx + 1),
                "record": {
                    "run_id": run_id,
                    "context_run_id": context_run_id,
                    "tool": tool,
                    "phase": "completed",
                    "status": "succeeded",
                },
            })
        })
        .collect()
}

fn requester_context(profile: &DemoProfile, role: &str) -> Value {
    json!({
        "org_units": profile.org_units(),
        "roles": [role],
    })
}

fn final_slack_response(profile: &DemoProfile) -> &'static str {
    match profile.unit_id {
        "sales" => "ACME changes this week: renewal is active, SSO onboarding friction is the top support theme, and the champion change keeps relationship risk at medium. Financial/payment details and raw repo details were not included.",
        "engineering" => "Engineering view for ACME: JWT rotation landed, the SSO integration branch remains open, the Linear epic targets M2, and the SEV-2 cache incident was mitigated. Contract value and payment status are hidden.",
        "finance" => "Finance view for ACME: INV-2043 is $120k net-30 and 7 days overdue, an $8k refund is pending approval, and the MSA auto-renews on 2026-09-01 with a 14% uplift. Raw repo details are hidden; financial reads are approval-gated.",
        "leadership" => "Leadership view for ACME: top-5 account, renewal on track, one open SEV, and a payment slip noted with financial details redacted.",
        "contractor_acme_x" => "Contractor view: only Project X export-pipeline material is in scope. Customer account, finance, repo, and support details are unavailable for this requester.",
        _ => "No ACME demo response is configured for this requester.",
    }
}

fn ids_from_rows(rows: &[Value], key: &str) -> Vec<String> {
    let mut ids = rows
        .iter()
        .filter_map(|row| row.get(key).and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn value_str(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn sanitize_id(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acme_slack_demo_harness_replays_all_five_profiles() {
        let bundle = run_acme_slack_demo_harness();
        assert_eq!(bundle["prompt"].as_str(), Some(DEMO_PROMPT));
        assert_eq!(bundle["profile_count"].as_u64(), Some(5));
        let runs = bundle["runs"].as_array().expect("runs");
        assert_eq!(runs.len(), 5);
        let mut actors = runs
            .iter()
            .map(|run| {
                run["resolved_tandem_principal"]["actor_id"]
                    .as_str()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        actors.sort();
        actors.dedup();
        assert_eq!(
            actors.len(),
            5,
            "each run must resolve a distinct requester"
        );
    }

    #[test]
    fn acme_slack_demo_receipts_have_required_evidence_shape() {
        for run in receipt_runs() {
            assert!(run["slack_request"]["workspace_id"].as_str().is_some());
            assert!(run["slack_request"]["channel_id"].as_str().is_some());
            assert!(run["slack_request"]["user_id"].as_str().is_some());
            assert!(run["resolved_tandem_principal"]["actor_id"]
                .as_str()
                .is_some());
            assert!(run["verified_tenant_context"]["tenant_context"].is_object());
            assert_nonempty_array(&run["verified_tenant_context"]["org_units"]);
            assert_nonempty_array(&run["verified_tenant_context"]["roles"]);
            assert_nonempty_array(&run["verified_tenant_context"]["strict_projection"]["grants"]);
            assert_nonempty_array(&run["memory"]["scopes_queried"]);
            assert!(run["memory"]["hidden_by_scope"].as_array().is_some());
            assert!(run["tools"]["offered"].as_array().is_some());
            assert!(run["tools"]["hidden_by_scope"].as_array().is_some());
            assert!(run["tools"]["used"].as_array().is_some());
            assert_nonempty_array(&run["policy_decisions"]);
            assert!(run["approvals"]["approval_required_events"]
                .as_array()
                .is_some());
            assert!(run["redactions"].as_array().is_some());
            assert!(run["final_slack_visible_response"].as_str().is_some());
        }
    }

    #[test]
    fn acme_slack_demo_department_answers_are_profile_appropriate() {
        let sales = run_for("U_SALES");
        assert_returned_memory(
            &sales,
            &["sales_crm_acme", "sales_risk_flag", "sales_support_theme"],
        );
        assert_response_contains(&sales, &["renewal", "support", "relationship risk"]);
        assert_response_omits(&sales, &["$120k", "INV-2043", "PR #4821"]);

        let engineering = run_for("U_ENG");
        assert_returned_memory(
            &engineering,
            &[
                "eng_github_auth",
                "eng_incident_sev2",
                "eng_linear_milestone",
            ],
        );
        assert_response_contains(&engineering, &["JWT rotation", "Linear epic", "SEV-2"]);
        assert_response_omits(&engineering, &["$120k", "net-30", "14% uplift"]);

        let finance = run_for("U_FINANCE");
        assert_returned_memory(
            &finance,
            &[
                "finance_contract_acme",
                "finance_invoice_acme",
                "finance_payment_run",
            ],
        );
        assert_response_contains(&finance, &["$120k", "net-30", "$8k", "14% uplift"]);
        assert_response_omits(&finance, &["PR #4821", "JWT rotation"]);

        let leadership = run_for("U_LEADER");
        assert_returned_memory(&leadership, &["leadership_board_summary"]);
        assert_response_contains(
            &leadership,
            &["top-5 account", "financial details redacted"],
        );
        assert_response_omits(&leadership, &["$120k", "14% uplift", "INV-2043"]);

        let contractor = run_for("U_CONTRACTOR");
        assert_returned_memory(&contractor, &["contractor_project_x"]);
        assert_response_contains(&contractor, &["Project X", "only", "in scope"]);
        assert_response_omits(&contractor, &["$120k", "JWT rotation", "top-5 account"]);
    }

    #[test]
    fn acme_slack_demo_tool_scoping_keeps_hidden_tools_unoffered() {
        for run in receipt_runs() {
            let offered = string_array(&run["tools"]["offered"]);
            let hidden = string_array(&run["tools"]["hidden_by_scope"]);
            let used = string_array(&run["tools"]["used"]);
            for tool in &used {
                assert!(
                    offered.contains(tool),
                    "used tool must have been offered: {tool}"
                );
            }
            for tool in hidden {
                assert!(
                    !offered.contains(&tool),
                    "hidden tool must not be offered: {tool}"
                );
                assert!(
                    !used.contains(&tool),
                    "hidden tool must not be used: {tool}"
                );
            }
        }

        let engineering = run_for("U_ENG");
        let eng_hidden = string_array(&engineering["tools"]["hidden_by_scope"]);
        assert!(eng_hidden.contains(&"mcp.invoices.read_invoices".to_string()));
        assert!(eng_hidden.contains(&"mcp.contracts.read_contracts".to_string()));

        let finance = run_for("U_FINANCE");
        let finance_hidden = string_array(&finance["tools"]["hidden_by_scope"]);
        assert!(finance_hidden.contains(&"mcp.github.read_repo".to_string()));
    }

    #[test]
    fn acme_slack_demo_approval_gate_covers_finance_sensitive_actions() {
        let finance = run_for("U_FINANCE");
        let approvals = finance["approvals"]["approval_required_events"]
            .as_array()
            .expect("approval events");
        let tools = approvals
            .iter()
            .filter_map(|event| event["tool"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        assert!(tools.contains(&"mcp.invoices.read_invoices".to_string()));
        assert!(tools.contains(&"mcp.contracts.read_contracts".to_string()));

        for run in receipt_runs() {
            if run["profile"]["slack_user_id"].as_str() == Some("U_FINANCE") {
                continue;
            }
            let offered = string_array(&run["tools"]["offered"]);
            assert!(
                !offered.contains(&"mcp.invoices.read_invoices".to_string()),
                "non-finance profiles must not globally receive finance read tools"
            );
        }
    }

    #[test]
    fn acme_slack_demo_receipt_matches_control_panel_contract() {
        for run in receipt_runs() {
            let receipt = &run["control_panel_receipt"];
            assert!(receipt["ledger"]["tool_manifest"].is_object());
            let package = &receipt["evidence_package"];
            assert_eq!(
                package["package_type"].as_str(),
                Some("tandem_run_governance_evidence")
            );
            assert!(package["run"].is_object());
            assert!(package["actors"]["tenant_actor_id"].as_str().is_some());
            assert_nonempty_array(&package["actors"]["requester_org_units"]);
            assert_nonempty_array(&package["actors"]["requester_roles"]);
            assert!(package["tool_manifest"]["used_subset_offered"]
                .as_bool()
                .unwrap_or(false));
            assert!(package["policy_decisions"].as_array().is_some());
            assert!(package["approvals"].is_object());
            assert!(package["memory_audit"].as_array().is_some());
            assert!(package["audit"]["protected_events"].as_array().is_some());
            assert!(package["redaction_policy"].is_object());
            assert!(package["final_outcome"]["slack_visible_response"]
                .as_str()
                .is_some());
        }
    }

    fn receipt_runs() -> Vec<Value> {
        run_acme_slack_demo_harness()["runs"]
            .as_array()
            .expect("runs")
            .clone()
    }

    fn run_for(slack_user: &str) -> Value {
        receipt_runs()
            .into_iter()
            .find(|run| run["profile"]["slack_user_id"].as_str() == Some(slack_user))
            .unwrap_or_else(|| panic!("missing run for {slack_user}"))
    }

    fn assert_nonempty_array(value: &Value) {
        assert!(
            value.as_array().is_some_and(|rows| !rows.is_empty()),
            "expected non-empty array: {value:?}"
        );
    }

    fn assert_returned_memory(run: &Value, expected: &[&str]) {
        let mut actual = run["memory"]["returned"]
            .as_array()
            .expect("returned memory")
            .iter()
            .filter_map(|row| row["memory_id"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        actual.sort();
        let mut expected = expected
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(actual, expected);
    }

    fn assert_response_contains(run: &Value, snippets: &[&str]) {
        let response = run["final_slack_visible_response"].as_str().unwrap();
        for snippet in snippets {
            assert!(
                response.contains(snippet),
                "response for {} should contain {snippet:?}: {response}",
                run["profile"]["slack_user_id"]
            );
        }
    }

    fn assert_response_omits(run: &Value, snippets: &[&str]) {
        let response = run["final_slack_visible_response"].as_str().unwrap();
        for snippet in snippets {
            assert!(
                !response.contains(snippet),
                "response for {} leaked {snippet:?}: {response}",
                run["profile"]["slack_user_id"]
            );
        }
    }

    fn string_array(value: &Value) -> Vec<String> {
        let rows: &[Value] = value.as_array().map(Vec::as_slice).unwrap_or(&[]);
        rows.iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect()
    }
}
