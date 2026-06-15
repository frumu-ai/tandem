// Article 12 log-completeness checks for protected actions and approval decisions
// (EUAI-09 / TAN-250).
//
// Cross-references the four records that together evidence a protected action —
// the policy decision, the approval, the tool-effect ledger entry, and the
// protected audit event — and reports any record that is missing, dangling,
// mis-tenanted, expired, or out of sequence. The result is surfaced in the
// governance evidence package so an operator can see incomplete audit evidence
// before relying on an exported packet. The checker is a pure function over the
// same inputs the package is built from, so it is also callable for offline
// verification of an exported bundle.

/// Article 12 record-keeping event taxonomy this checker reasons about. Emitted in
/// the package so a reviewer knows which event classes completeness is asserted over.
const ARTICLE_12_EVENT_TAXONOMY: &[&str] = &[
    "approval_granted",
    "approval_denied",
    "approval_reworked",
    "approval_cancelled",
    "protected_tool_call",
    "policy_decision",
    "evidence_export",
    "incident_failure",
];

const COMPLETENESS_SEVERITY_ERROR: &str = "error";
const COMPLETENESS_SEVERITY_WARNING: &str = "warning";

/// Append a protected audit health event when an exported evidence packet is not
/// fully `complete`. The payload carries only the status, counts, and distinct
/// finding kinds (IDs/tool names already appear in the packet) — no redacted detail.
async fn emit_completeness_health_event(
    state: &AppState,
    tenant_context: &TenantContext,
    run_id: &str,
    principal_id: Option<String>,
    completeness: &Value,
) {
    if completeness["status"].as_str() == Some("complete") {
        return;
    }
    let finding_kinds = completeness["findings"]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .filter_map(|finding| finding["kind"].as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let _ = crate::audit::append_protected_audit_event(
        state,
        "audit.health.completeness_incomplete",
        tenant_context,
        principal_id,
        json!({
            "runID": run_id,
            "resourceKind": "audit_export",
            "status": completeness["status"],
            "counts": completeness["counts"],
            "findingKinds": finding_kinds,
        }),
    )
    .await;
}

fn completeness_finding(severity: &str, kind: &str, detail: String, subject: Value) -> Value {
    json!({
        "severity": severity,
        "kind": kind,
        "detail": detail,
        "subject": subject,
    })
}

/// Returns `true` when a tool requires approval under the fintech protected-action
/// classification (i.e. is not `Safe`). Used to decide which executed tool calls
/// must carry full policy/approval/audit evidence.
fn tool_is_protected(tool: &str) -> bool {
    !classify_fintech_tool(tool).allowed_without_approval()
}

/// Build the `audit_completeness` block for the governance evidence package.
///
/// `error`-severity findings mark a packet `incomplete`; `warning`-severity findings
/// (e.g. an approval recorded before decider attribution was enforced) mark it
/// `complete_with_warnings`. A packet with no findings is `complete`.
fn governance_evidence_completeness(
    context_run: &ContextRunState,
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
    records: &[ContextRunLedgerEventView],
    policy_decisions: &[PolicyDecisionRecord],
    protected_audit: &[ProtectedAuditEnvelope],
) -> Value {
    let run_tenant = &context_run.tenant_context;
    let mut findings: Vec<Value> = Vec::new();

    let empty_history: &[crate::AutomationGateDecisionRecord] = &[];
    let gate_history = automation_run
        .map(|run| run.checkpoint.gate_history.as_slice())
        .unwrap_or(empty_history);

    let audit_event_ids: BTreeSet<&str> = protected_audit
        .iter()
        .map(|event| event.event_id.as_str())
        .collect();

    let mut protected_action_count = 0usize;
    let mut approval_decision_count = 0usize;

    // ---- Policy decisions: tenant, approval, tool-effect, audit, expiry ----
    for decision in policy_decisions {
        if decision.tenant_context != *run_tenant {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "tenant_mismatch",
                "policy decision tenant does not match the run tenant".to_string(),
                json!({ "policy_decision_id": decision.decision_id }),
            ));
        }

        if !matches!(decision.decision, PolicyDecisionEffect::ApprovalRequired) {
            continue;
        }
        approval_decision_count += 1;
        protected_action_count += 1;

        // Approval evidence: an approval id on the decision, or an approve gate
        // decision recorded for the decision's node.
        let has_approval_id = decision.approval_id.is_some();
        let has_gate_approval = decision
            .node_id
            .as_deref()
            .map(|node_id| {
                gate_history.iter().any(|gate| {
                    gate.node_id == node_id
                        && gate.decision.to_ascii_lowercase().starts_with("approv")
                })
            })
            .unwrap_or(false);
        if !has_approval_id && !has_gate_approval {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "missing_approval_evidence",
                "approval-required policy decision has no approval id and no recorded approve gate decision".to_string(),
                json!({ "policy_decision_id": decision.decision_id, "node_id": decision.node_id }),
            ));
        }

        // Tool-effect evidence: an outcome ledger record linked by decision id.
        let linked_effects: Vec<&ContextRunLedgerEventView> = records
            .iter()
            .filter(|row| {
                row.record.policy_decision_id.as_deref() == Some(decision.decision_id.as_str())
            })
            .collect();
        if linked_effects.is_empty() {
            // Warning rather than error: the gated action may have been reworked or
            // cancelled before execution, in which case no tool-effect is expected.
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_WARNING,
                "missing_tool_effect_evidence",
                "approval-required policy decision has no linked tool-effect ledger record".to_string(),
                json!({ "policy_decision_id": decision.decision_id }),
            ));
        }

        // Protected audit linkage: a referenced audit event must be present.
        if let Some(audit_event_id) = decision.audit_event_id.as_deref() {
            if !audit_event_ids.contains(audit_event_id) {
                findings.push(completeness_finding(
                    COMPLETENESS_SEVERITY_ERROR,
                    "missing_protected_audit_event",
                    "policy decision references an audit event that is not present in the packet"
                        .to_string(),
                    json!({
                        "policy_decision_id": decision.decision_id,
                        "audit_event_id": audit_event_id,
                    }),
                ));
            }
        }

        // Expiry: a linked protected action executed after the approval expired.
        if let Some(expires_at_ms) = decision
            .metadata
            .get("expires_at_ms")
            .and_then(Value::as_u64)
        {
            for row in linked_effects.iter().filter(|row| {
                matches!(row.record.status, ToolEffectLedgerStatus::Succeeded)
                    && matches!(row.record.phase, ToolEffectLedgerPhase::Outcome)
            }) {
                if row.ts_ms > expires_at_ms {
                    findings.push(completeness_finding(
                        COMPLETENESS_SEVERITY_ERROR,
                        "expired_approval",
                        "protected action executed after its approval expired".to_string(),
                        json!({
                            "policy_decision_id": decision.decision_id,
                            "expires_at_ms": expires_at_ms,
                            "executed_at_ms": row.ts_ms,
                            "tool": row.record.tool,
                        }),
                    ));
                }
            }
        }
    }

    // ---- Executed protected tool calls must carry a policy decision ----
    for row in records {
        if !matches!(row.record.phase, ToolEffectLedgerPhase::Outcome)
            || !matches!(row.record.status, ToolEffectLedgerStatus::Succeeded)
            || !tool_is_protected(&row.record.tool)
        {
            continue;
        }
        protected_action_count += 1;
        match row.record.policy_decision_id.as_deref() {
            None => findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "missing_policy_decision",
                "protected tool call succeeded without a linked policy decision".to_string(),
                json!({ "tool": row.record.tool, "event_id": row.event_id }),
            )),
            Some(decision_id) => {
                if !policy_decisions
                    .iter()
                    .any(|decision| decision.decision_id == decision_id)
                {
                    findings.push(completeness_finding(
                        COMPLETENESS_SEVERITY_ERROR,
                        "missing_policy_decision",
                        "protected tool call references a policy decision that is not present in the packet"
                            .to_string(),
                        json!({
                            "tool": row.record.tool,
                            "event_id": row.event_id,
                            "policy_decision_id": decision_id,
                        }),
                    ));
                }
            }
        }
    }

    // ---- Approval gate decisions must record who decided (Article 14) ----
    for gate in gate_history {
        if gate.decided_by.is_none() {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_WARNING,
                "unattributed_approval",
                "gate decision has no recorded decider (legacy record predating attribution enforcement)"
                    .to_string(),
                json!({ "node_id": gate.node_id, "decision": gate.decision }),
            ));
        }
    }

    // ---- Protected audit events: tenant scope and hash-chain continuity ----
    for event in protected_audit {
        if event.tenant_context != *run_tenant {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "tenant_mismatch",
                "protected audit event tenant does not match the run tenant".to_string(),
                json!({ "event_id": event.event_id }),
            ));
        }
    }
    let mut hashed: Vec<&ProtectedAuditEnvelope> = protected_audit
        .iter()
        .filter(|event| !event.record_hash.is_empty())
        .collect();
    hashed.sort_by_key(|event| event.seq);
    for window in hashed.windows(2) {
        let (prev, next) = (window[0], window[1]);
        if next.seq == prev.seq {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "sequence_gap",
                "protected audit ledger contains a replayed sequence number".to_string(),
                json!({ "seq": next.seq, "event_id": next.event_id }),
            ));
        } else if next.seq == prev.seq + 1
            && next.prev_hash.as_deref() != Some(prev.record_hash.as_str())
        {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "sequence_gap",
                "protected audit ledger hash chain is broken between adjacent records".to_string(),
                json!({ "seq": next.seq, "event_id": next.event_id }),
            ));
        }
    }

    let error_count = findings
        .iter()
        .filter(|finding| finding["severity"] == COMPLETENESS_SEVERITY_ERROR)
        .count();
    let warning_count = findings.len() - error_count;
    let status = if error_count > 0 {
        "incomplete"
    } else if warning_count > 0 {
        "complete_with_warnings"
    } else {
        "complete"
    };

    json!({
        "schema_version": 1,
        "status": status,
        "checked_at_ms": crate::now_ms(),
        "event_taxonomy": ARTICLE_12_EVENT_TAXONOMY,
        "counts": {
            "protected_actions_checked": protected_action_count,
            "approval_decisions_checked": approval_decision_count,
            "policy_decisions": policy_decisions.len(),
            "gate_decisions": gate_history.len(),
            "protected_audit_events": protected_audit.len(),
            "tool_effect_records": records.len(),
            "findings": findings.len(),
            "errors": error_count,
            "warnings": warning_count,
        },
        "findings": findings,
    })
}
