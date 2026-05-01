fn bug_monitor_triage_manual_schedule() -> crate::AutomationV2Schedule {
    crate::AutomationV2Schedule {
        schedule_type: crate::AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
    }
}

fn bug_monitor_triage_output_contract(
    artifact_type: &str,
    summary_guidance: &str,
) -> crate::AutomationFlowOutputContract {
    crate::AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: Some(crate::AutomationOutputEnforcement {
            validation_profile: Some("local_research".to_string()),
            required_tools: vec!["read".to_string()],
            required_tool_calls: Vec::new(),
            required_evidence: vec!["local_source_reads".to_string()],
            required_sections: Vec::new(),
            prewrite_gates: vec!["concrete_reads".to_string()],
            retry_on_missing: vec![
                "no_concrete_reads".to_string(),
                "local_source_reads".to_string(),
            ],
            terminal_on: vec![
                "no_concrete_reads".to_string(),
                "local_source_reads".to_string(),
            ],
            repair_budget: Some(1),
            session_text_recovery: Some("allow".to_string()),
        }),
        schema: None,
        summary_guidance: Some(summary_guidance.to_string()),
    }
}

fn bug_monitor_triage_repo_evidence_guidance(artifact_type: &str) -> String {
    format!(
        "Required output: valid completed JSON for `{artifact_type}`. Before writing, perform a local repo evidence pass using `codesearch`, `grep`, `glob`, and `read` as appropriate. Prefer fast local search for symbols, node IDs, error strings, event names, artifact paths, and workflow IDs from the payload. Include `search_queries_used`, `files_examined`, `file_references` with path and line/snippet when available, `likely_files_to_edit`, `affected_components`, `tool_evidence`, `uncertainty`, and `bounded_next_steps`. If no relevant code is found, say which searches were run and why they were inconclusive. Do not finish with only generic diagnosis."
    )
}

fn bug_monitor_triage_node(
    node_id: &str,
    agent_id: &str,
    objective: &str,
    depends_on: Vec<String>,
    timeout_ms: u64,
    artifact_path: &str,
    artifact_type: &str,
    payload: serde_json::Value,
) -> crate::AutomationFlowNode {
    crate::AutomationFlowNode {
        node_id: node_id.to_string(),
        agent_id: agent_id.to_string(),
        objective: objective.to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        depends_on,
        input_refs: Vec::new(),
        output_contract: Some(bug_monitor_triage_output_contract(
            artifact_type,
            &bug_monitor_triage_repo_evidence_guidance(artifact_type),
        )),
        retry_policy: Some(json!({
            "max_attempts": 2,
            "backoff_ms": 10_000,
        })),
        timeout_ms: Some(timeout_ms),
        max_tool_calls: Some(24),
        stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "task_kind": artifact_type,
                "title": objective,
                "output_path": artifact_path,
                "knowledge": {
                    "subject": objective,
                    "payload": payload,
                },
            },
            "bug_monitor": {
                "artifact_type": artifact_type,
            },
        })),
    }
}

pub(crate) fn bug_monitor_triage_spec(
    draft: &BugMonitorDraftRecord,
    workspace_root: Option<String>,
    model_policy: Option<serde_json::Value>,
    mcp_servers: Vec<String>,
    inspection_payload: serde_json::Value,
    research_payload: serde_json::Value,
    validation_payload: serde_json::Value,
    fix_payload: serde_json::Value,
) -> crate::AutomationV2Spec {
    let now = crate::now_ms();
    let automation_id = format!("automation-v2-bug-monitor-triage-{}", draft.draft_id);
    let model_policy = model_policy.or_else(|| {
        Some(json!({
            "default_model": "system_default",
        }))
    });
    crate::AutomationV2Spec {
        automation_id,
        name: format!(
            "Bug Monitor triage: {}",
            draft.title.as_deref().unwrap_or("runtime failure").trim()
        ),
        description: Some(format!(
            "Inspect, research, validate, and propose a fix for Bug Monitor draft {}.",
            draft.draft_id
        )),
        status: crate::AutomationV2Status::Active,
        schedule: bug_monitor_triage_manual_schedule(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![crate::AutomationAgentProfile {
            agent_id: "bug_monitor_triage_agent".to_string(),
            template_id: None,
            display_name: "Bug Monitor Triage".to_string(),
            avatar_url: None,
            model_policy,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec![
                    "read".to_string(),
                    "grep".to_string(),
                    "glob".to_string(),
                    "codesearch".to_string(),
                    "mcp_list".to_string(),
                    "write".to_string(),
                    "web_search".to_string(),
                ],
                denylist: vec!["edit".to_string(), "apply_patch".to_string()],
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: mcp_servers,
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![
                bug_monitor_triage_node(
                    "inspect_failure_report",
                    "bug_monitor_triage_agent",
                    "Inspect the failure report, extract concrete search terms, then use fast local repo search/read tools to identify the affected files, functions, modules, and evidence lines before writing the inspection artifact",
                    Vec::new(),
                    240_000,
                    ".tandem/artifacts/bug_monitor.inspection.json",
                    "bug_monitor_inspection",
                    inspection_payload,
                ),
                bug_monitor_triage_node(
                    "research_likely_root_cause",
                    "bug_monitor_triage_agent",
                    "Research likely root cause and related prior failures by combining the inspection artifact with local repo search, failure memory, issue search when available, and artifact/log review; include concrete file references and searched terms",
                    vec!["inspect_failure_report".to_string()],
                    600_000,
                    ".tandem/artifacts/bug_monitor.research.json",
                    "bug_monitor_research",
                    research_payload,
                ),
                bug_monitor_triage_node(
                    "validate_failure_scope",
                    "bug_monitor_triage_agent",
                    "Validate the failure scope using the researched files, symbols, artifacts, and logs; classify config versus capability versus provider/tool versus code defect, and cite the repo evidence used",
                    vec!["research_likely_root_cause".to_string()],
                    240_000,
                    ".tandem/artifacts/bug_monitor.validation.json",
                    "bug_monitor_validation",
                    validation_payload,
                ),
                bug_monitor_triage_node(
                    "propose_fix_and_verification",
                    "bug_monitor_triage_agent",
                    "Propose a bounded fix and verification plan grounded in specific file references, likely edit points, acceptance criteria, and smoke-test commands; mark coder_ready only when evidence is concrete",
                    vec!["validate_failure_scope".to_string()],
                    360_000,
                    ".tandem/artifacts/bug_monitor.fix_proposal.json",
                    "bug_monitor_fix_proposal",
                    fix_payload,
                ),
            ],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: Some(1_800_000),
            max_total_tool_calls: Some(96),
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: "bug_monitor".to_string(),
        workspace_root,
        metadata: Some(json!({
            "source": "bug_monitor",
            "draft_id": draft.draft_id,
            "repo": draft.repo,
            "fingerprint": draft.fingerprint,
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

pub(crate) fn bug_monitor_automation_run_id_from_triage_run_id(
    triage_run_id: &str,
) -> Option<String> {
    triage_run_id
        .strip_prefix("automation-v2-automation-v2-run-")
        .map(|suffix| format!("automation-v2-run-{suffix}"))
}

pub(crate) fn bug_monitor_triage_context_run_id(run_id: &str) -> String {
    super::context_runs::automation_v2_context_run_id(run_id)
}

pub(crate) async fn bug_monitor_triage_effective_started_at_ms(
    state: &AppState,
    triage_run_id: &str,
) -> Option<u64> {
    if let Some(run_id) = bug_monitor_automation_run_id_from_triage_run_id(triage_run_id) {
        return state
            .get_automation_v2_run(&run_id)
            .await
            .map(|run| run.started_at_ms.unwrap_or(run.created_at_ms));
    }
    super::context_runs::context_run_effective_started_at_ms(state, triage_run_id)
        .await
        .ok()
}

/// True if the triage run has reached a terminal status. Missing or
/// corrupt run state is treated as non-terminal so the timeout recovery
/// can mark the draft and publish the evidence-rich fallback body.
pub(crate) async fn bug_monitor_triage_run_is_terminal(state: &AppState, run_id: &str) -> bool {
    if let Some(automation_run_id) = bug_monitor_automation_run_id_from_triage_run_id(run_id) {
        return state
            .get_automation_v2_run(&automation_run_id)
            .await
            .map(|run| {
                matches!(
                    run.status,
                    crate::AutomationRunStatus::Completed
                        | crate::AutomationRunStatus::Failed
                        | crate::AutomationRunStatus::Cancelled
                )
            })
            .unwrap_or(false);
    }
    match load_context_run_state(state, run_id).await {
        Ok(run) => super::context_runs::context_run_is_terminal(&run.status),
        Err(_) => false,
    }
}

pub(crate) async fn finalize_completed_bug_monitor_triage(
    state: &AppState,
    draft_id: &str,
) -> anyhow::Result<bool> {
    let Some(draft) = state.get_bug_monitor_draft(draft_id).await else {
        return Ok(false);
    };
    if draft.github_issue_url.is_some() || draft.issue_number.is_some() {
        return Ok(false);
    }
    let Some(triage_run_id) = draft.triage_run_id.clone() else {
        return Ok(false);
    };
    let Some(automation_run_id) = bug_monitor_automation_run_id_from_triage_run_id(&triage_run_id)
    else {
        return Ok(false);
    };
    let Some(run) = state.get_automation_v2_run(&automation_run_id).await else {
        return Ok(false);
    };
    if !matches!(run.status, crate::AutomationRunStatus::Completed) {
        return Ok(false);
    }

    let incident_id = bug_monitor_incident_id_for_draft(state, draft_id, &triage_run_id).await;
    if load_bug_monitor_triage_summary_artifact(state, &triage_run_id)
        .await
        .is_none()
    {
        let response = create_bug_monitor_triage_summary(
            State(state.clone()),
            Path(draft_id.to_string()),
            Json(BugMonitorTriageSummaryInput::default()),
        )
        .await;
        if !response.status().is_success() {
            anyhow::bail!(
                "Bug Monitor triage summary finalization failed with HTTP {}",
                response.status()
            );
        }
    } else if load_bug_monitor_issue_draft_artifact(state, &triage_run_id)
        .await
        .is_none()
    {
        ensure_bug_monitor_issue_draft(state.clone(), draft_id, true).await?;
    }

    match bug_monitor_github::publish_draft(
        state,
        draft_id,
        incident_id.as_deref(),
        bug_monitor_github::PublishMode::Auto,
    )
    .await
    {
        Ok(outcome) => {
            if let Some(incident_id) = incident_id {
                if let Some(mut incident) = state.get_bug_monitor_incident(&incident_id).await {
                    incident.status = outcome.action.clone();
                    incident.last_error = None;
                    incident.updated_at_ms = crate::now_ms();
                    let _ = state.put_bug_monitor_incident(incident).await;
                }
            }
            Ok(true)
        }
        Err(error) => {
            let detail = crate::truncate_text(&error.to_string(), 500);
            if let Some(mut draft) = state.get_bug_monitor_draft(draft_id).await {
                draft.status = "github_post_failed".to_string();
                draft.github_status = Some("github_post_failed".to_string());
                draft.last_post_error = Some(detail.clone());
                let _ = state.put_bug_monitor_draft(draft.clone()).await;
                if let Err(record_err) = bug_monitor_github::record_post_failure(
                    state,
                    &draft,
                    incident_id.as_deref(),
                    "triage_finalization",
                    draft.evidence_digest.as_deref(),
                    &detail,
                )
                .await
                {
                    tracing::warn!(
                        draft_id = %draft_id,
                        error = %record_err,
                        "failed to record Bug Monitor triage finalization post failure",
                    );
                }
            }
            if let Some(incident_id) = incident_id {
                if let Some(mut incident) = state.get_bug_monitor_incident(&incident_id).await {
                    incident.status = "github_post_failed".to_string();
                    incident.last_error = Some(detail);
                    incident.updated_at_ms = crate::now_ms();
                    let _ = state.put_bug_monitor_incident(incident).await;
                }
            }
            Ok(true)
        }
    }
}

async fn bug_monitor_incident_id_for_draft(
    state: &AppState,
    draft_id: &str,
    triage_run_id: &str,
) -> Option<String> {
    let incidents = state.bug_monitor_incidents.read().await;
    incidents
        .values()
        .find(|incident| {
            incident.draft_id.as_deref() == Some(draft_id)
                || incident.triage_run_id.as_deref() == Some(triage_run_id)
        })
        .map(|incident| incident.incident_id.clone())
}

pub(crate) async fn bug_monitor_triage_timeout_diagnostics(
    state: &AppState,
    run_id: &str,
    timeout_ms: u64,
) -> Option<serde_json::Value> {
    if let Some(automation_run_id) = bug_monitor_automation_run_id_from_triage_run_id(run_id) {
        return bug_monitor_automation_triage_timeout_diagnostics(
            state,
            &automation_run_id,
            timeout_ms,
        )
        .await;
    }
    super::context_runs::bug_monitor_triage_timeout_diagnostics(state, run_id, timeout_ms).await
}

async fn bug_monitor_automation_triage_timeout_diagnostics(
    state: &AppState,
    run_id: &str,
    timeout_ms: u64,
) -> Option<serde_json::Value> {
    let run = state.get_automation_v2_run(run_id).await?;
    let now = crate::now_ms();
    let started_at_ms = run.started_at_ms.unwrap_or(run.created_at_ms);
    let elapsed_ms = now.saturating_sub(started_at_ms);
    let stale_ms = now.saturating_sub(run.updated_at_ms);
    let status = serde_json::to_value(&run.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    let total_steps = run
        .automation_snapshot
        .as_ref()
        .map(|automation| automation.flow.nodes.len())
        .unwrap_or_else(|| {
            run.checkpoint
                .completed_nodes
                .len()
                .saturating_add(run.checkpoint.pending_nodes.len())
                .saturating_add(run.checkpoint.blocked_nodes.len())
        });
    let completed_steps = run.checkpoint.completed_nodes.len();
    let failed_steps = usize::from(run.checkpoint.last_failure.is_some());
    let active_step = run
        .checkpoint
        .pending_nodes
        .first()
        .or_else(|| run.checkpoint.blocked_nodes.first())
        .map(|node_id| {
            json!({
                "step_id": node_id,
                "title": node_id,
                "status": if run.checkpoint.blocked_nodes.contains(node_id) {
                    "blocked"
                } else {
                    "pending"
                },
            })
        });
    let node_attempts = collect_triage_per_node_attempt_stats(&run).await;
    Some(json!({
        "run_id": run.run_id,
        "context_run_id": bug_monitor_triage_context_run_id(&run.run_id),
        "run_status": status,
        "timeout_ms": timeout_ms,
        "elapsed_ms": elapsed_ms,
        "stale_ms": stale_ms,
        "last_event_seq": Value::Null,
        "step_count": total_steps,
        "completed_steps": completed_steps,
        "failed_steps": failed_steps,
        "active_step": active_step,
        "last_failure": run.checkpoint.last_failure,
        "node_attempts": node_attempts,
    }))
}

/// Aggregate per-step receipt records for a triage run so the
/// timeout diagnostics can answer "where did the time go" without
/// needing per-LLM-call receipts (those don't exist yet — see
/// `provider.call.iteration.*` events on the bus that aren't yet
/// persisted). Today's receipts have `tool_invoked`,
/// `tool_succeeded`, `tool_failed`, and `attempt_summary` records,
/// which is enough to distinguish "model is thinking" from "tool
/// round-trips dominate." For each node we surface tool counts,
/// attempt count, and the wall-clock span we observed activity in.
async fn collect_triage_per_node_attempt_stats(
    run: &crate::AutomationV2RunRecord,
) -> Vec<Value> {
    use crate::app::state::automation::receipts;
    let receipts_root = receipts::automation_attempt_receipts_root();
    let mut node_ids: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut push_unique = |node_id: &str, into: &mut Vec<String>| {
        if seen.insert(node_id.to_string()) {
            into.push(node_id.to_string());
        }
    };
    for node_id in &run.checkpoint.completed_nodes {
        push_unique(node_id, &mut node_ids);
    }
    for node_id in &run.checkpoint.pending_nodes {
        push_unique(node_id, &mut node_ids);
    }
    for node_id in &run.checkpoint.blocked_nodes {
        push_unique(node_id, &mut node_ids);
    }
    if let Some(automation) = run.automation_snapshot.as_ref() {
        for node in &automation.flow.nodes {
            push_unique(&node.node_id, &mut node_ids);
        }
    }
    let mut out = Vec::new();
    let completed: std::collections::HashSet<&str> = run
        .checkpoint
        .completed_nodes
        .iter()
        .map(String::as_str)
        .collect();
    let blocked: std::collections::HashSet<&str> = run
        .checkpoint
        .blocked_nodes
        .iter()
        .map(String::as_str)
        .collect();
    for node_id in node_ids {
        let path = receipts::automation_attempt_receipts_path(&receipts_root, &run.run_id, &node_id);
        let records = receipts::read_automation_attempt_receipt_records(&path)
            .await
            .unwrap_or_default();
        // Always emit a row per known node — including in-flight nodes
        // whose receipts file is empty (P1 fix: receipts are written at
        // attempt finalization, so the very step that timed out often
        // has zero records yet, and dropping it would hide the most
        // important diagnostic).
        let lifecycle_status = if completed.contains(node_id.as_str()) {
            "completed"
        } else if blocked.contains(node_id.as_str()) {
            "blocked"
        } else if records.is_empty() {
            "in_flight_no_receipts"
        } else {
            "in_flight"
        };
        let mut stats = aggregate_per_node_records(&node_id, &records);
        if let Some(obj) = stats.as_object_mut() {
            obj.insert(
                "lifecycle_status".to_string(),
                Value::String(lifecycle_status.to_string()),
            );
        }
        out.push(stats);
    }
    out
}

fn aggregate_per_node_records(
    node_id: &str,
    records: &[crate::app::state::automation::receipts::AutomationAttemptReceiptRecord],
) -> Value {
    let mut tool_invocations: u64 = 0;
    let mut tool_succeeded: u64 = 0;
    let mut tool_failed: u64 = 0;
    let mut attempt_summary_count: u64 = 0;
    let mut max_attempt: u32 = 0;
    for record in records {
        if record.attempt > max_attempt {
            max_attempt = record.attempt;
        }
        match record.event_type.as_str() {
            "tool_invoked" => tool_invocations += 1,
            "tool_succeeded" => tool_succeeded += 1,
            "tool_failed" => tool_failed += 1,
            "attempt_summary" => attempt_summary_count += 1,
            _ => {}
        }
    }
    // Deliberately not surfacing wall-clock spans here: the receipt
    // ts_ms is the time the JSONL line was *appended* (after attempt
    // finalization), not when the LLM/tool work actually ran. A
    // 4-minute attempt could show as ~milliseconds. True per-step
    // execution timing requires persisting `provider.call.iteration.*`
    // events to receipts (a tandem-core change) and is the natural
    // follow-up.
    json!({
        "node_id": node_id,
        "max_attempt": max_attempt,
        "attempt_summary_count": attempt_summary_count,
        "tool_invocations": tool_invocations,
        "tool_succeeded": tool_succeeded,
        "tool_failed": tool_failed,
    })
}

#[cfg(test)]
mod bug_monitor_triage_spec_tests {
    use super::*;

    #[test]
    fn triage_spec_has_four_nodes_with_correct_dependencies() {
        let draft = BugMonitorDraftRecord {
            draft_id: "draft-1".to_string(),
            repo: "owner/repo".to_string(),
            title: Some("Failure".to_string()),
            detail: Some("detail".to_string()),
            fingerprint: "fp".to_string(),
            ..Default::default()
        };
        let spec = bug_monitor_triage_spec(
            &draft,
            Some("/tmp/workspace".to_string()),
            None,
            Vec::new(),
            json!({}),
            json!({}),
            json!({}),
            json!({}),
        );

        assert_eq!(spec.flow.nodes.len(), 4);
        assert_eq!(spec.flow.nodes[0].node_id, "inspect_failure_report");
        assert!(spec.flow.nodes[0].depends_on.is_empty());
        assert_eq!(
            spec.flow.nodes[1].depends_on,
            vec!["inspect_failure_report".to_string()]
        );
        assert_eq!(
            spec.flow.nodes[2].depends_on,
            vec!["research_likely_root_cause".to_string()]
        );
        assert_eq!(
            spec.flow.nodes[3].depends_on,
            vec!["validate_failure_scope".to_string()]
        );
        assert_eq!(spec.flow.nodes[1].timeout_ms, Some(600_000));
        assert!(spec.flow.nodes[0]
            .objective
            .contains("fast local repo search/read tools"));
        assert!(spec.flow.nodes[3]
            .objective
            .contains("specific file references"));
        assert!(spec.flow.nodes.iter().all(|node| {
            node.output_contract
                .as_ref()
                .and_then(|contract| contract.validator)
                == Some(crate::AutomationOutputValidatorKind::StructuredJson)
        }));
        assert!(spec.flow.nodes.iter().all(|node| node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.enforcement.as_ref())
            .is_some_and(|enforcement| enforcement
                .required_tools
                .iter()
                .any(|tool| tool == "read")
                && enforcement
                    .required_evidence
                    .iter()
                    .any(|item| item == "local_source_reads"))));
        assert!(spec.flow.nodes.iter().all(|node| node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.summary_guidance.as_deref())
            .is_some_and(|guidance| guidance.contains("search_queries_used")
                && guidance.contains("file_references")
                && guidance.contains("likely_files_to_edit"))));
    }

    fn record(seq: u64, ts_ms: u64, attempt: u32, event_type: &str) -> crate::app::state::automation::receipts::AutomationAttemptReceiptRecord {
        crate::app::state::automation::receipts::AutomationAttemptReceiptRecord {
            version: 1,
            run_id: "run-x".to_string(),
            node_id: "n".to_string(),
            attempt,
            session_id: "s".to_string(),
            seq,
            ts_ms,
            event_type: event_type.to_string(),
            payload: json!({}),
        }
    }

    #[test]
    fn aggregate_per_node_records_counts_tool_calls_and_attempts() {
        let records = vec![
            record(1, 100, 1, "tool_invoked"),
            record(2, 110, 1, "tool_succeeded"),
            record(3, 200, 1, "tool_invoked"),
            record(4, 250, 1, "tool_failed"),
            record(5, 400, 1, "attempt_summary"),
            record(6, 500, 2, "tool_invoked"),
            record(7, 600, 2, "tool_succeeded"),
            record(8, 700, 2, "attempt_summary"),
        ];
        let stats = aggregate_per_node_records("inspect", &records);
        assert_eq!(stats["node_id"], "inspect");
        assert_eq!(stats["max_attempt"], 2);
        assert_eq!(stats["attempt_summary_count"], 2);
        assert_eq!(stats["tool_invocations"], 3);
        assert_eq!(stats["tool_succeeded"], 2);
        assert_eq!(stats["tool_failed"], 1);
    }

    /// Receipt timestamps reflect JSONL append time, not real
    /// execution time, so we deliberately do NOT publish a span.
    /// This guards against regressing back to a misleading wall-clock
    /// derived from `record.ts_ms`.
    #[test]
    fn aggregate_per_node_records_does_not_publish_misleading_span() {
        let records = vec![
            record(1, 1, 1, "tool_invoked"),
            record(2, 2, 1, "tool_succeeded"),
        ];
        let stats = aggregate_per_node_records("inspect", &records);
        assert!(stats.get("activity_span_ms").is_none());
        assert!(stats.get("first_event_ts_ms").is_none());
        assert!(stats.get("last_event_ts_ms").is_none());
    }

    #[test]
    fn aggregate_per_node_records_handles_empty_input() {
        let stats = aggregate_per_node_records("research", &[]);
        assert_eq!(stats["max_attempt"], 0);
        assert_eq!(stats["tool_invocations"], 0);
    }
}
