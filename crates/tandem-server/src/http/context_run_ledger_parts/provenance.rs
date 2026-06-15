const ARTICLE_50_NOTICE: &str =
    "This content was produced or materially transformed by an AI system. Review before relying on it.";

/// Derive the reviewer/approval state for a single generated artifact from the run's
/// gate decision history, aligned with the `AIGeneratedBadge` states:
/// `draft` (no recorded human decision), `reviewed` (a decision exists but is not an
/// approval), or `approved` (a recorded approval). Returns the state, the matching
/// transparency label, and a redacted-free review summary when a decision exists.
///
/// Gate decisions are recorded for the gate node itself, not for upstream artifact nodes.
/// `gate_coverage` maps artifact node_id → gate node_ids that review it (from the automation
/// spec's `depends_on` lists), so decisions on a covering gate are also considered.
///
/// Returns `(reviewer_state, transparency_label, review_json, matched_gate_node_id)`.
/// `matched_gate_node_id` is the node_id of the gate decision that determined state — used
/// by the caller to look up the correct `approval_id` from the policy decisions map.
fn artifact_reviewer_state<'a>(
    node_id: &str,
    gate_history: &'a [crate::AutomationGateDecisionRecord],
    gate_coverage: &BTreeMap<String, Vec<String>>,
) -> (&'static str, &'static str, Option<Value>, Option<String>) {
    let covering_gate_ids: &[String] = gate_coverage
        .get(node_id)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let latest = gate_history
        .iter()
        .filter(|record| {
            record.node_id == node_id
                || covering_gate_ids
                    .iter()
                    .any(|gid| gid == &record.node_id)
        })
        .max_by_key(|record| record.decided_at_ms);
    match latest {
        Some(record) if record.decision.to_ascii_lowercase().starts_with("approv") => (
            "approved",
            "AI-Generated, approved",
            Some(json!({
                "decision": record.decision,
                "decided_by": record.decided_by,
                "decided_at_ms": record.decided_at_ms,
            })),
            Some(record.node_id.clone()),
        ),
        Some(record) => (
            "reviewed",
            "AI-Generated, reviewed",
            Some(json!({
                "decision": record.decision,
                "decided_by": record.decided_by,
                "decided_at_ms": record.decided_at_ms,
            })),
            Some(record.node_id.clone()),
        ),
        None => ("draft", "AI-Generated", None, None),
    }
}

/// Build the top-level provenance block for the evidence package. Captures generation
/// status, transparency label, run/automation identifiers, model/provider metadata, and
/// an overall reviewer state derived from the run's gate history (EUAI-05/TAN-246).
fn governance_evidence_provenance(
    context_run: &ContextRunState,
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
) -> Value {
    let empty_history: &[crate::AutomationGateDecisionRecord] = &[];
    let gate_history = automation_run
        .map(|run| run.checkpoint.gate_history.as_slice())
        .unwrap_or(empty_history);
    let awaiting = automation_run
        .and_then(|run| run.checkpoint.awaiting_gate.as_ref())
        .is_some();
    let any_approved = gate_history
        .iter()
        .any(|record| record.decision.to_ascii_lowercase().starts_with("approv"));
    let any_decided = !gate_history.is_empty();
    let (reviewer_state, transparency_label) = if any_approved && !awaiting {
        ("approved", "AI-Generated, approved")
    } else if any_decided {
        ("reviewed", "AI-Generated, reviewed")
    } else {
        ("draft", "AI-Generated")
    };

    json!({
        "generation": "ai_generated",
        "transparency_label": transparency_label,
        "article_50_notice": ARTICLE_50_NOTICE,
        "reviewer_state": reviewer_state,
        "run_id": automation_run
            .map(|run| run.run_id.as_str())
            .unwrap_or(context_run.run_id.as_str()),
        "context_run_id": context_run.run_id,
        "automation_v2_run_id": automation_run.map(|run| run.run_id.clone()),
        "automation_id": automation_run.map(|run| run.automation_id.clone()),
        "model_provider": context_run.model_provider,
        "model_id": context_run.model_id,
        "mcp_servers": context_run.mcp_servers,
        "source_client": context_run.source_client,
        "generated_at_ms": context_run.started_at_ms.unwrap_or(context_run.created_at_ms),
        "run_created_at_ms": context_run.created_at_ms,
    })
}

/// Map each node that produced an artifact to its approval id (where a policy decision
/// recorded one), so per-artifact provenance can link to the approval that gated it.
/// Policy decisions are loaded ascending by `created_at_ms`, so later inserts overwrite
/// earlier ones — keeping the most-recent approval_id for each node (important when a gate
/// is sent back for rework and later re-approved).
fn governance_evidence_node_approval_ids(
    policy_decisions: &[PolicyDecisionRecord],
) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for decision in policy_decisions {
        if let (Some(node_id), Some(approval_id)) =
            (decision.node_id.as_ref(), decision.approval_id.as_ref())
        {
            map.insert(node_id.clone(), approval_id.clone());
        }
    }
    map
}

/// Build a map from artifact node_id → list of gate node_ids that review it, derived from
/// gate nodes' `depends_on` lists in the automation spec. Used to find gate decisions that
/// cover an artifact even though the decision is recorded for the gate node, not the artifact.
fn build_artifact_gate_coverage(
    automation: &crate::automation_v2::types::AutomationV2Spec,
) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in &automation.flow.nodes {
        if node.gate.is_some() {
            for upstream_id in &node.depends_on {
                map.entry(upstream_id.clone())
                    .or_default()
                    .push(node.node_id.clone());
            }
        }
    }
    map
}
