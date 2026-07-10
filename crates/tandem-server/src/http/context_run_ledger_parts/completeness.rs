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
    crate::audit::append_protected_audit_event_best_effort(
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
/// classification (i.e. is not `Safe`).
fn tool_is_protected(tool: &str) -> bool {
    !classify_fintech_tool(tool).allowed_without_approval()
}

/// Whether a policy decision marks an action the policy engine treated as protected:
/// an explicit approval gate (`ApprovalRequired`), or an `Allow` granted on the basis
/// of an approval (the `matching_approval_receipt` execution path sets `approval_id`).
fn decision_is_protected_action(decision: &PolicyDecisionRecord) -> bool {
    match decision.decision {
        PolicyDecisionEffect::ApprovalRequired => true,
        PolicyDecisionEffect::Allow => {
            decision.approval_id.is_some()
                || decision
                    .reason_code
                    .to_ascii_lowercase()
                    .contains("approval")
        }
        PolicyDecisionEffect::Deny => false,
    }
}

/// Whether a policy decision carries approval evidence: an explicit approval id, or a
/// recorded approve gate decision for the decision's node.
fn decision_evidences_approval(
    decision: &PolicyDecisionRecord,
    gate_history: &[crate::AutomationGateDecisionRecord],
) -> bool {
    if decision.approval_id.is_some() {
        return true;
    }
    decision
        .node_id
        .as_deref()
        .map(|node_id| {
            gate_history.iter().any(|gate| {
                gate.node_id == node_id && gate.decision.to_ascii_lowercase().starts_with("approv")
            })
        })
        .unwrap_or(false)
}

/// Whether a protected audit event in the packet attests to a decision — either an
/// explicit `audit_event_id` match, or an event whose payload references the decision
/// id or its approval id. Protected audit events are recorded separately from the
/// policy decision (recorders set `audit_event_id: None` and append the audit event
/// independently), so payload linkage is the normal case.
fn protected_audit_attests_decision(
    decision: &PolicyDecisionRecord,
    protected_audit: &[ProtectedAuditEnvelope],
) -> bool {
    if let Some(audit_event_id) = decision.audit_event_id.as_deref() {
        if protected_audit
            .iter()
            .any(|event| event.event_id == audit_event_id)
        {
            return true;
        }
    }
    let mut needles: BTreeSet<String> = BTreeSet::new();
    needles.insert(decision.decision_id.clone());
    if let Some(approval_id) = decision.approval_id.as_ref() {
        needles.insert(approval_id.clone());
    }
    protected_audit
        .iter()
        .any(|event| value_contains_any_string(&event.payload, &needles))
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

    let mut protected_action_count = 0usize;
    let mut approval_decision_count = 0usize;

    // ---- Tenant scope: every policy decision must match the run tenant ----
    for decision in policy_decisions {
        if decision.tenant_context != *run_tenant {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "tenant_mismatch",
                "policy decision tenant does not match the run tenant".to_string(),
                json!({ "policy_decision_id": decision.decision_id }),
            ));
        }
    }

    // ---- Approval-required gate requests: count and tool-effect presence ----
    for decision in policy_decisions {
        if !matches!(decision.decision, PolicyDecisionEffect::ApprovalRequired) {
            continue;
        }
        approval_decision_count += 1;
        // The gated action may have been reworked or cancelled before execution, in
        // which case no tool-effect is expected — so a missing one is advisory here.
        // Strict approval/audit/expiry evidence is enforced on the executed action below.
        let has_linked_effect = records.iter().any(|row| {
            row.record.policy_decision_id.as_deref() == Some(decision.decision_id.as_str())
        });
        if !has_linked_effect {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_WARNING,
                "missing_tool_effect_evidence",
                "approval-required policy decision has no linked tool-effect ledger record"
                    .to_string(),
                json!({ "policy_decision_id": decision.decision_id }),
            ));
        }
    }

    // ---- Executed protected actions need the full four-record evidence chain ----
    //
    // A protected tool call that actually succeeded is the case Article 12 most cares
    // about. In fintech strict mode the runtime records the execution as a
    // `PolicyDecisionEffect::Allow` (`matching_approval_receipt`) decision with the
    // approval id attached, appending the protected audit event separately. Anchoring on
    // the succeeded tool-effect and resolving its linked decision covers both the
    // `ApprovalRequired` gate path and the `Allow` execution path. A tool is in scope if
    // it is fintech-protected by name, or its linked decision marks it protected (the
    // risk-tier action-gate path, where the tool name alone may not classify).
    for row in records {
        if !matches!(row.record.phase, ToolEffectLedgerPhase::Outcome)
            || !matches!(row.record.status, ToolEffectLedgerStatus::Succeeded)
        {
            continue;
        }
        let linked_decision = row
            .record
            .policy_decision_id
            .as_deref()
            .and_then(|id| policy_decisions.iter().find(|d| d.decision_id == id));
        let decision_marks_protected = linked_decision
            .map(decision_is_protected_action)
            .unwrap_or(false);
        if !tool_is_protected(&row.record.tool) && !decision_marks_protected {
            continue;
        }
        protected_action_count += 1;

        let decision = match (row.record.policy_decision_id.as_deref(), linked_decision) {
            (None, _) => {
                findings.push(completeness_finding(
                    COMPLETENESS_SEVERITY_ERROR,
                    "missing_policy_decision",
                    "protected tool call succeeded without a linked policy decision".to_string(),
                    json!({ "tool": row.record.tool, "event_id": row.event_id }),
                ));
                continue;
            }
            (Some(decision_id), None) => {
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
                continue;
            }
            (Some(_), Some(decision)) => decision,
        };

        // Approval evidence.
        if !decision_evidences_approval(decision, gate_history) {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "missing_approval_evidence",
                "protected tool call succeeded without an approval id or recorded approve gate decision"
                    .to_string(),
                json!({
                    "policy_decision_id": decision.decision_id,
                    "tool": row.record.tool,
                    "node_id": decision.node_id,
                }),
            ));
        }

        // Protected audit evidence (recorded separately; matched by id or payload).
        if !protected_audit_attests_decision(decision, protected_audit) {
            findings.push(completeness_finding(
                COMPLETENESS_SEVERITY_ERROR,
                "missing_protected_audit_event",
                "protected tool call succeeded without a protected audit event attesting the action"
                    .to_string(),
                json!({
                    "policy_decision_id": decision.decision_id,
                    "tool": row.record.tool,
                }),
            ));
        }

        // Expiry: the action executed after its approval expired.
        if let Some(expires_at_ms) = decision
            .metadata
            .get("expires_at_ms")
            .and_then(Value::as_u64)
        {
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
