#[tokio::test]
async fn code_loop_flow_repairs_after_missing_verification_and_completes() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-code-loop-integration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");
    std::fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn release_note_title() -> &'static str {\n    \"old title\"\n}\n",
    )
    .expect("seed source");

    let state = ready_test_state().await;
    let node = code_loop_node("implement_release_fix", ".tandem/artifacts/code-loop.md");
    let automation = automation_with_single_node(
        "automation-code-loop",
        node.clone(),
        &workspace_root,
        vec![
            "read".to_string(),
            "apply_patch".to_string(),
            "write".to_string(),
            "bash".to_string(),
        ],
    );
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let output_path = automation_node_required_output_path_for_run(&node, Some(&run.run_id))
        .expect("required output path");
    let workspace_snapshot_before = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let handoff_text = "# Implementation Handoff\n\n## Files changed\n- `src/lib.rs`\n\n## Summary\nUpdated the release note title helper to use the repaired title string.\n\n## Verification\n- `cargo test`\n";

    let artifact_dir = workspace_root
        .join(".tandem/runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    std::fs::write(artifact_dir.join("code-loop.md"), handoff_text).expect("write artifact");
    std::fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn release_note_title() -> &'static str {\n    \"repaired title\"\n}\n",
    )
    .expect("write patched source");

    let first_session = assistant_session_with_tool_invocations(
        "code-loop-attempt-1",
        &workspace_root,
        vec![
            (
                "read",
                json!({"path":"src/lib.rs"}),
                json!({"output":"pub fn release_note_title() -> &'static str { \"old title\" }\n"}),
                None,
            ),
            (
                "apply_patch",
                json!({"patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-pub fn release_note_title() -> &'static str {\n-    \"old title\"\n-}\n+pub fn release_note_title() -> &'static str {\n+    \"repaired title\"\n+}\n*** End Patch\n"}),
                json!({"ok": true}),
                None,
            ),
            (
                "write",
                json!({"path":output_path,"content":handoff_text}),
                json!({"ok": true}),
                None,
            ),
        ],
    );
    let requested_tools = vec![
        "read".to_string(),
        "apply_patch".to_string(),
        "write".to_string(),
        "bash".to_string(),
    ];
    let first_telemetry =
        summarize_automation_tool_activity(&node, &first_session, &requested_tools);
    assert_eq!(
        first_telemetry
            .get("verification_expected")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        first_telemetry
            .get("verification_ran")
            .and_then(Value::as_bool),
        Some(false)
    );

    let first_session_text =
        "Patched the code and wrote the handoff.\n\n{\"status\":\"completed\"}";
    let (first_accepted_output, first_artifact_validation, first_rejected) =
        validate_automation_artifact_output(
            &node,
            &first_session,
            workspace_root.to_str().expect("workspace root string"),
            first_session_text,
            &first_telemetry,
            None,
            Some((output_path.clone(), handoff_text.to_string())),
            &workspace_snapshot_before,
        );
    assert!(first_rejected.is_none());
    assert_eq!(
        first_artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    let first_status = detect_automation_node_status(
        &node,
        first_session_text,
        first_accepted_output.as_ref(),
        &first_telemetry,
        Some(&first_artifact_validation),
    );
    assert_eq!(first_status.0, "needs_repair");
    assert_eq!(
        first_status.1.as_deref(),
        Some("coding task completed without running the declared verification command")
    );

    let second_session = assistant_session_with_tool_invocations(
        "code-loop-attempt-2",
        &workspace_root,
        vec![
            (
                "read",
                json!({"path":"src/lib.rs"}),
                json!({"output":"pub fn release_note_title() -> &'static str { \"repaired title\" }\n"}),
                None,
            ),
            (
                "apply_patch",
                json!({"patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-pub fn release_note_title() -> &'static str {\n-    \"repaired title\"\n-}\n+pub fn release_note_title() -> &'static str {\n+    \"repaired title\"\n+}\n*** End Patch\n"}),
                json!({"ok": true}),
                None,
            ),
            (
                "bash",
                json!({"command":"cargo test"}),
                json!({
                    "output": "test result: ok. 1 passed; 0 failed;",
                    "metadata": {
                        "exit_code": 0
                    }
                }),
                None,
            ),
            (
                "write",
                json!({"path":output_path,"content":handoff_text}),
                json!({"ok": true}),
                None,
            ),
        ],
    );
    let second_telemetry =
        summarize_automation_tool_activity(&node, &second_session, &requested_tools);
    assert_eq!(
        second_telemetry
            .get("verification_ran")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        second_telemetry
            .get("verification_failed")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        second_telemetry
            .get("latest_verification_command")
            .and_then(Value::as_str),
        Some("cargo test")
    );

    let second_session_text =
        "Patched the code, reran verification, and finalized the handoff.\n\n{\"status\":\"completed\"}";
    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &second_session,
        workspace_root.to_str().expect("workspace root string"),
        second_session_text,
        &second_telemetry,
        Some(handoff_text),
        Some((output_path.clone(), handoff_text.to_string())),
        &workspace_snapshot_before,
    );
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    let status = detect_automation_node_status(
        &node,
        second_session_text,
        accepted_output.as_ref(),
        &second_telemetry,
        Some(&artifact_validation),
    );
    assert_eq!(status.0, "done");

    let output = wrap_automation_node_output(
        &node,
        &second_session,
        &requested_tools,
        &second_session.id,
        Some(&run.run_id),
        second_session_text,
        accepted_output.clone(),
        Some(artifact_validation.clone()),
    );
    persist_validated_output(
        &state,
        &run.run_id,
        &node.node_id,
        output,
        AutomationRunStatus::Completed,
        2,
    )
    .await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Completed);
    assert_eq!(
        persisted
            .checkpoint
            .node_attempts
            .get("implement_release_fix"),
        Some(&2)
    );
    let output = persisted
        .checkpoint
        .node_outputs
        .get("implement_release_fix")
        .expect("node output");
    assert_eq!(output.get("status").and_then(Value::as_str), Some("done"));
    assert_eq!(
        output
            .pointer("/tool_telemetry/verification_ran")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .pointer("/tool_telemetry/latest_verification_command")
            .and_then(Value::as_str),
        Some("cargo test")
    );

    let written_handoff =
        std::fs::read_to_string(artifact_dir.join("code-loop.md")).expect("written artifact");
    assert_eq!(written_handoff, handoff_text);
    let patched_source =
        std::fs::read_to_string(workspace_root.join("src/lib.rs")).expect("patched source");
    assert!(patched_source.contains("repaired title"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn repair_retry_after_needs_repair_completes_on_second_attempt() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-repair-retry-integration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("docs")).expect("create workspace");
    std::fs::write(
        workspace_root.join("docs/source.md"),
        "# Source\n\nWorkspace evidence for the retry brief.\n",
    )
    .expect("seed source file");

    let state = ready_test_state().await;

    let mut node = brief_research_node("research_retry", ".tandem/artifacts/retry-brief.md", true);
    node.retry_policy = Some(json!({
        "max_attempts": 2
    }));
    let automation = automation_with_single_node(
        "automation-retry-research",
        node.clone(),
        &workspace_root,
        vec!["read".to_string()],
    );
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let output_path = automation_node_required_output_path_for_run(&node, Some(&run.run_id))
        .expect("required output path");
    let workspace_snapshot_before = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let local_brief_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nLocal-only comparison for this first pass.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md: not needed for this first pass.\n"
        .to_string();
    let web_brief_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n### Files Reviewed\n| Local Path | Evidence Summary |\n|---|---|\n| `docs/source.md` | Core source reviewed |\n\n### Files Not Reviewed\n| Local Path | Reason |\n|---|---|\n| `docs/extra.md` | Out of scope for this run |\n\n### Web Sources Reviewed\n| URL | Status | Notes |\n|---|---|---|\n| https://example.com | Fetched | Confirmed live |\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nExternal web comparison for the retry run.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n"
        .to_string();

    let artifact_dir = workspace_root
        .join(".tandem/runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    std::fs::write(artifact_dir.join("retry-brief.md"), &local_brief_text)
        .expect("write first artifact");

    let first_session = assistant_session_with_tool_invocations(
        "repair-retry-attempt-1",
        &workspace_root,
        vec![
            (
                "glob",
                json!({"pattern":"docs/**/*.md"}),
                json!({
                    "output": workspace_root
                        .join("docs/source.md")
                        .display()
                        .to_string()
                }),
                None,
            ),
            (
                "read",
                json!({"path":"docs/source.md"}),
                json!({"output":"Workspace evidence for the retry brief."}),
                None,
            ),
            (
                "write",
                json!({"path":output_path,"content":local_brief_text}),
                json!({"ok": true}),
                None,
            ),
        ],
    );
    let requested_tools = vec![
        "glob".to_string(),
        "read".to_string(),
        "websearch".to_string(),
        "write".to_string(),
    ];
    let first_telemetry =
        summarize_automation_tool_activity(&node, &first_session, &requested_tools);
    assert_eq!(
        first_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["glob", "read", "write"])
    );
    assert_eq!(
        first_telemetry
            .get("web_research_used")
            .and_then(Value::as_bool),
        Some(false)
    );

    let first_session_text = "Done\n\n{\"status\":\"completed\"}";
    let (first_accepted_output, first_artifact_validation, first_rejected) =
        validate_automation_artifact_output(
            &node,
            &first_session,
            workspace_root.to_str().expect("workspace root string"),
            first_session_text,
            &first_telemetry,
            None,
            Some((output_path.clone(), local_brief_text.clone())),
            &workspace_snapshot_before,
        );
    assert_eq!(
        first_artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("needs_repair")
    );
    let first_status = detect_automation_node_status(
        &node,
        first_session_text,
        first_accepted_output.as_ref(),
        &first_telemetry,
        Some(&first_artifact_validation),
    );
    assert_eq!(first_status.0, "needs_repair");
    assert!(first_rejected.is_some());
    assert!(first_artifact_validation
        .get("semantic_block_reason")
        .and_then(Value::as_str)
        .is_some());

    std::fs::write(artifact_dir.join("retry-brief.md"), &web_brief_text)
        .expect("write repaired artifact");

    let second_session = assistant_session_with_tool_invocations(
        "repair-retry-attempt-2",
        &workspace_root,
        vec![
            (
                "glob",
                json!({"pattern":"docs/**/*.md"}),
                json!({
                    "output": workspace_root
                        .join("docs/source.md")
                        .display()
                        .to_string()
                }),
                None,
            ),
            (
                "read",
                json!({"path":"docs/source.md"}),
                json!({"output":"Workspace evidence for the retry brief."}),
                None,
            ),
            (
                "write",
                json!({"path":output_path,"content":local_brief_text}),
                json!({"ok": true}),
                None,
            ),
            (
                "websearch",
                json!({"query":"tandem competitor landscape"}),
                json!({
                    "output": "Matched Tandem web research",
                    "metadata": {"count": 2}
                }),
                None,
            ),
            (
                "write",
                json!({"path":output_path,"content":web_brief_text}),
                json!({"ok": true}),
                None,
            ),
        ],
    );
    let second_telemetry =
        summarize_automation_tool_activity(&node, &second_session, &requested_tools);
    let second_executed_tools = second_telemetry
        .get("executed_tools")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .expect("executed tools");
    assert!(second_executed_tools.iter().any(|tool| *tool == "glob"));
    assert!(second_executed_tools.iter().any(|tool| *tool == "read"));
    assert!(second_executed_tools
        .iter()
        .any(|tool| *tool == "websearch"));
    assert!(second_executed_tools.iter().any(|tool| *tool == "write"));
    assert_eq!(
        second_telemetry
            .pointer("/tool_call_counts/write")
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        second_telemetry
            .get("web_research_used")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        second_telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );

    let second_session_text = "Done\n\n{\"status\":\"completed\"}";
    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &second_session,
        workspace_root.to_str().expect("workspace root string"),
        second_session_text,
        &second_telemetry,
        Some(&local_brief_text),
        Some((output_path.clone(), web_brief_text.clone())),
        &workspace_snapshot_before,
    );
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        artifact_validation
            .get("repair_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );

    let status = detect_automation_node_status(
        &node,
        second_session_text,
        accepted_output.as_ref(),
        &second_telemetry,
        Some(&artifact_validation),
    );
    assert_eq!(status.0, "completed");

    let output = wrap_automation_node_output(
        &node,
        &second_session,
        &requested_tools,
        &second_session.id,
        Some(&run.run_id),
        second_session_text,
        accepted_output.clone(),
        Some(artifact_validation.clone()),
    );
    persist_validated_output(
        &state,
        &run.run_id,
        &node.node_id,
        output.clone(),
        AutomationRunStatus::Completed,
        2,
    )
    .await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Completed);
    assert_eq!(
        persisted.checkpoint.node_attempts.get("research_retry"),
        Some(&2)
    );

    let output = persisted
        .checkpoint
        .node_outputs
        .get("research_retry")
        .expect("node output");
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        output
            .pointer("/tool_telemetry/web_research_used")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .pointer("/tool_telemetry/web_research_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );
    let output_tools = output
        .pointer("/tool_telemetry/executed_tools")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .expect("output tools");
    assert!(output_tools.iter().any(|tool| *tool == "glob"));
    assert!(output_tools.iter().any(|tool| *tool == "read"));
    assert!(output_tools.iter().any(|tool| *tool == "websearch"));
    assert!(output_tools.iter().any(|tool| *tool == "write"));

    let written = std::fs::read_to_string(
        workspace_root
            .join(".tandem/runs")
            .join(&run.run_id)
            .join("artifacts")
            .join("retry-brief.md"),
    )
    .expect("written artifact");
    assert_eq!(written, web_brief_text);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[derive(Debug, PartialEq, Eq)]
struct RestartResumeGolden {
    status: AutomationRunStatus,
    detail: Option<String>,
    stop_kind: Option<AutomationStopKind>,
    completed_nodes: Vec<String>,
    pending_nodes: Vec<String>,
    blocked_nodes: Vec<String>,
    awaiting_gate_node: Option<String>,
    node_outputs: std::collections::BTreeMap<String, String>,
    node_attempts: std::collections::BTreeMap<String, u32>,
    gate_decisions: Vec<(String, String)>,
    last_failure: Option<(String, String)>,
}

fn sorted_node_ids(mut node_ids: Vec<String>) -> Vec<String> {
    node_ids.sort();
    node_ids
}

fn restart_resume_golden(run: &AutomationV2RunRecord) -> RestartResumeGolden {
    let node_outputs = run
        .checkpoint
        .node_outputs
        .iter()
        .map(|(node_id, output)| {
            let status = output
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("<missing>");
            let contract = output
                .get("contract_kind")
                .and_then(Value::as_str)
                .unwrap_or("<missing>");
            let decision = output
                .pointer("/content/decision")
                .and_then(Value::as_str)
                .unwrap_or("<none>");
            (
                node_id.clone(),
                format!("status={status};contract={contract};decision={decision}"),
            )
        })
        .collect();
    let node_attempts = run
        .checkpoint
        .node_attempts
        .iter()
        .map(|(node_id, attempt)| (node_id.clone(), *attempt))
        .collect();
    RestartResumeGolden {
        status: run.status.clone(),
        detail: run.detail.clone(),
        stop_kind: run.stop_kind.clone(),
        completed_nodes: sorted_node_ids(run.checkpoint.completed_nodes.clone()),
        pending_nodes: sorted_node_ids(run.checkpoint.pending_nodes.clone()),
        blocked_nodes: sorted_node_ids(run.checkpoint.blocked_nodes.clone()),
        awaiting_gate_node: run
            .checkpoint
            .awaiting_gate
            .as_ref()
            .map(|gate| gate.node_id.clone()),
        node_outputs,
        node_attempts,
        gate_decisions: run
            .checkpoint
            .gate_history
            .iter()
            .map(|record| (record.node_id.clone(), record.decision.clone()))
            .collect(),
        last_failure: run
            .checkpoint
            .last_failure
            .as_ref()
            .map(|failure| (failure.node_id.clone(), failure.reason.clone())),
    }
}

async fn reload_automation_state_after_restart(source: &AppState) -> (AppState, usize) {
    let mut reloaded = ready_test_state().await;
    reloaded.automations_v2_path = source.automations_v2_path.clone();
    reloaded.automation_v2_runs_path = source.automation_v2_runs_path.clone();
    reloaded.memory_db_path = source.memory_db_path.clone();
    reloaded
        .load_automations_v2()
        .await
        .expect("load persisted automations");
    reloaded
        .load_automation_v2_runs()
        .await
        .expect("load persisted automation runs");
    let recovered = reloaded.recover_in_flight_runs().await;
    (reloaded, recovered)
}

async fn claim_and_drain_restart_run(state: &AppState, run_id: &str) -> AutomationV2RunRecord {
    let claimed = state
        .claim_specific_automation_v2_run(run_id)
        .await
        .expect("claim queued restart run");
    crate::automation_v2::executor::run_automation_v2_run(state.clone(), claimed).await;
    state
        .get_automation_v2_run(run_id)
        .await
        .expect("persisted restart run")
}

fn restart_test_workspace(prefix: &str) -> std::path::PathBuf {
    let workspace_root = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create restart test workspace");
    workspace_root
}

fn empty_restart_automation(
    automation_id: &str,
    workspace_root: &std::path::Path,
) -> AutomationV2Spec {
    AutomationSpecBuilder::new(automation_id)
        .name(format!("{automation_id} restart test"))
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .build()
}

fn approval_restart_automation(
    automation_id: &str,
    workspace_root: &std::path::Path,
) -> AutomationV2Spec {
    let mut approval = AutomationNodeBuilder::new("approve_consequential_delivery")
        .objective("Review the prepared work before continuation")
        .stage_kind(AutomationNodeStageKind::Approval)
        .metadata(json!({
            "builder": {
                "title": "Consequential work approval",
                "role": "approver"
            }
        }))
        .build();
    approval.gate = Some(AutomationApprovalGate {
        required: true,
        decisions: vec![
            "approve".to_string(),
            "rework".to_string(),
            "cancel".to_string(),
        ],
        rework_targets: Vec::new(),
        instructions: Some("Approve the consequential work before it can proceed.".to_string()),
    });
    let mut collect_inputs = AutomationNodeBuilder::new("collect_inputs")
        .objective("Materialize the approved handoff inputs")
        .depends_on(vec!["approve_consequential_delivery"])
        .stage_kind(AutomationNodeStageKind::Workstream)
        .metadata(json!({
            "inputs": {
                "target_contact": "customer@example.test",
                "approval_required": true,
                "approved_action": "customer_update"
            }
        }))
        .build();
    collect_inputs.output_contract = Some(AutomationFlowOutputContract {
        kind: "brief".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    AutomationSpecBuilder::new(automation_id)
        .name(format!("{automation_id} restart approval test"))
        .nodes(vec![approval, collect_inputs])
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .build()
}

fn consequential_restart_automation(
    automation_id: &str,
    workspace_root: &std::path::Path,
) -> AutomationV2Spec {
    let send_node = AutomationNodeBuilder::new("send_customer_update")
        .objective("Send the customer update")
        .stage_kind(AutomationNodeStageKind::Workstream)
        .metadata(json!({
            "builder": {
                "task_kind": "consequential_delivery",
                "write_scope": "external:customer_update"
            },
            "delivery": {
                "method": "email",
                "to": "customer@example.test"
            }
        }))
        .build();
    AutomationSpecBuilder::new(automation_id)
        .name(format!("{automation_id} consequential restart test"))
        .nodes(vec![send_node])
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .build()
}

async fn create_persisted_restart_run(
    state: &AppState,
    automation: &AutomationV2Spec,
) -> AutomationV2RunRecord {
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist restart automation");
    state
        .create_automation_v2_run(automation, "manual")
        .await
        .expect("create restart run")
}

async fn queued_restart_terminal_golden(restart: bool) -> RestartResumeGolden {
    let workspace_root = restart_test_workspace("tandem-restart-queued");
    let state = ready_test_state().await;
    let automation = empty_restart_automation("automation-restart-queued", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;
    let active_state = if restart {
        let (reloaded, recovered) = reload_automation_state_after_restart(&state).await;
        assert_eq!(recovered, 0);
        reloaded
    } else {
        state.clone()
    };

    let terminal = claim_and_drain_restart_run(&active_state, &run.run_id).await;
    assert_eq!(terminal.status, AutomationRunStatus::Completed);
    let golden = restart_resume_golden(&terminal);
    let _ = std::fs::remove_dir_all(&workspace_root);
    golden
}

async fn approve_restart_gate_once(
    state: &AppState,
    automation: &AutomationV2Spec,
    run_id: &str,
    assert_duplicate_is_guarded: bool,
) -> AutomationV2RunRecord {
    let current = state
        .get_automation_v2_run(run_id)
        .await
        .expect("awaiting approval run");
    let gate = current
        .checkpoint
        .awaiting_gate
        .clone()
        .expect("pending approval gate");

    let mut applied = false;
    let approved = state
        .update_automation_v2_run(run_id, |row| {
            match crate::app::state::apply_automation_gate_decision(
                row,
                automation,
                &gate,
                "approve",
                Some("approved after restart".to_string()),
                None,
            ) {
                crate::app::state::AutomationGateDecisionOutcome::Applied => {
                    applied = true;
                }
                crate::app::state::AutomationGateDecisionOutcome::AlreadyDecided(_) => {
                    panic!("first gate decision was treated as already decided");
                }
            }
        })
        .await
        .expect("approve restart gate");
    assert!(applied);

    if assert_duplicate_is_guarded {
        let pending_after_first_decision = approved.checkpoint.pending_nodes.clone();
        let completed_after_first_decision = approved.checkpoint.completed_nodes.clone();
        let mut duplicate_guarded = false;
        let after_duplicate = state
            .update_automation_v2_run(run_id, |row| {
                match crate::app::state::apply_automation_gate_decision(
                    row,
                    automation,
                    &gate,
                    "approve",
                    Some("duplicate approval click".to_string()),
                    None,
                ) {
                    crate::app::state::AutomationGateDecisionOutcome::Applied => {
                        panic!("duplicate gate decision was applied");
                    }
                    crate::app::state::AutomationGateDecisionOutcome::AlreadyDecided(winner) => {
                        assert_eq!(
                            winner.as_ref().map(|record| record.decision.as_str()),
                            Some("approve")
                        );
                        duplicate_guarded = true;
                    }
                }
            })
            .await
            .expect("duplicate gate decision readback");
        assert!(duplicate_guarded);
        assert_eq!(after_duplicate.checkpoint.gate_history.len(), 1);
        assert_eq!(
            after_duplicate.checkpoint.pending_nodes,
            pending_after_first_decision
        );
        assert_eq!(
            after_duplicate.checkpoint.completed_nodes,
            completed_after_first_decision
        );
    }

    approved
}

async fn approval_restart_requeued_golden(
    restart: bool,
    assert_duplicate_is_guarded: bool,
) -> RestartResumeGolden {
    let workspace_root = restart_test_workspace("tandem-restart-approval");
    let state = ready_test_state().await;
    let automation = approval_restart_automation("automation-restart-approval", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;

    let awaiting = claim_and_drain_restart_run(&state, &run.run_id).await;
    assert_eq!(awaiting.status, AutomationRunStatus::AwaitingApproval);
    assert_eq!(
        awaiting
            .checkpoint
            .awaiting_gate
            .as_ref()
            .map(|gate| gate.node_id.as_str()),
        Some("approve_consequential_delivery")
    );

    let active_state = if restart {
        let (reloaded, recovered) = reload_automation_state_after_restart(&state).await;
        assert_eq!(recovered, 0);
        let reloaded_run = reloaded
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("reloaded approval run");
        assert_eq!(reloaded_run.status, AutomationRunStatus::AwaitingApproval);
        assert_eq!(
            reloaded_run
                .checkpoint
                .awaiting_gate
                .as_ref()
                .map(|gate| gate.node_id.as_str()),
            Some("approve_consequential_delivery")
        );
        reloaded
    } else {
        state.clone()
    };

    let approved = approve_restart_gate_once(
        &active_state,
        &automation,
        &run.run_id,
        assert_duplicate_is_guarded,
    )
    .await;
    assert_eq!(approved.status, AutomationRunStatus::Queued);
    let golden = restart_resume_golden(&approved);
    let _ = std::fs::remove_dir_all(&workspace_root);
    golden
}

fn run_restart_resume_test_with_large_stack<F, Fut>(future_factory: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let handle = std::thread::Builder::new()
        .name("automation-restart-resume-test".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("restart resume test runtime");
            runtime.block_on(future_factory());
        })
        .expect("spawn restart resume test thread");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test]
async fn restart_resume_golden_completes_queued_run_after_reload() {
    let uninterrupted = queued_restart_terminal_golden(false).await;
    let restarted = queued_restart_terminal_golden(true).await;

    assert_eq!(restarted, uninterrupted);
}

#[test]
fn restart_resume_golden_approval_gate_resumes_once_after_reload() {
    run_restart_resume_test_with_large_stack(|| async {
        let uninterrupted = approval_restart_requeued_golden(false, false).await;
        let restarted = approval_restart_requeued_golden(true, true).await;

        assert_eq!(restarted, uninterrupted);
        assert_eq!(restarted.status, AutomationRunStatus::Queued);
        assert_eq!(restarted.pending_nodes, vec!["collect_inputs".to_string()]);
        assert_eq!(
            restarted.gate_decisions,
            vec![(
                "approve_consequential_delivery".to_string(),
                "approve".to_string()
            )]
        );
        assert_eq!(
            restarted.node_outputs.get("approve_consequential_delivery"),
            Some(&"status=completed;contract=approval_gate;decision=approve".to_string())
        );
    });
}

#[tokio::test]
async fn restart_recovery_preserves_blocked_run_golden_after_reload() {
    let workspace_root = restart_test_workspace("tandem-restart-blocked");
    let state = ready_test_state().await;
    let automation = empty_restart_automation("automation-restart-blocked", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = AutomationRunStatus::Blocked;
            row.detail = Some("blocked by missing enterprise connector approval".to_string());
            row.checkpoint.blocked_nodes = vec!["enterprise_connector_approval".to_string()];
            row.checkpoint.last_failure = Some(AutomationFailureRecord {
                node_id: "enterprise_connector_approval".to_string(),
                reason: "missing enterprise connector approval".to_string(),
                failed_at_ms: crate::now_ms(),
            });
        })
        .await
        .expect("mark blocked restart run");
    let expected = restart_resume_golden(
        &state
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("blocked run before reload"),
    );

    let (reloaded, recovered) = reload_automation_state_after_restart(&state).await;
    assert_eq!(recovered, 0);
    assert!(reloaded
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .is_none());
    let actual = restart_resume_golden(
        &reloaded
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("blocked run after reload"),
    );

    assert_eq!(actual, expected);
    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn restart_recovery_fails_running_consequential_run_without_replay() {
    let workspace_root = restart_test_workspace("tandem-restart-running-consequential");
    let state = ready_test_state().await;
    let automation =
        consequential_restart_automation("automation-restart-running", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = AutomationRunStatus::Running;
            row.started_at_ms = Some(crate::now_ms());
            row.active_session_ids = vec!["session-in-flight-before-restart".to_string()];
            row.latest_session_id = Some("session-in-flight-before-restart".to_string());
        })
        .await
        .expect("mark running restart run");

    let (reloaded, recovered) = reload_automation_state_after_restart(&state).await;
    assert_eq!(recovered, 1);
    assert!(reloaded
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .is_none());
    let recovered_run = reloaded
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("recovered running run");
    let golden = restart_resume_golden(&recovered_run);

    assert_eq!(golden.status, AutomationRunStatus::Failed);
    assert_eq!(golden.stop_kind, Some(AutomationStopKind::ServerRestart));
    assert_eq!(
        golden.detail.as_deref(),
        Some("automation run interrupted by server restart")
    );
    assert!(golden.node_attempts.is_empty());
    assert!(golden.node_outputs.is_empty());
    assert_eq!(
        golden.pending_nodes,
        vec!["send_customer_update".to_string()]
    );
    assert_eq!(
        golden.last_failure,
        None,
        "restart recovery must fail in-flight consequential work without fabricating a node outcome"
    );
    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn restart_recovery_preserves_queued_and_paused_runs() {
    let paused_workspace =
        std::env::temp_dir().join(format!("tandem-recovery-paused-{}", uuid::Uuid::new_v4()));
    let queued_workspace =
        std::env::temp_dir().join(format!("tandem-recovery-queued-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&paused_workspace).expect("create paused workspace");
    std::fs::create_dir_all(&queued_workspace).expect("create queued workspace");

    let state = ready_test_state().await;
    let paused_automation = automation_with_single_node(
        "automation-paused-recovery",
        brief_research_node("paused_node", ".tandem/artifacts/paused.md", false),
        &paused_workspace,
        vec!["read".to_string()],
    );
    let queued_automation = automation_with_single_node(
        "automation-queued-recovery",
        brief_research_node("queued_node", ".tandem/artifacts/queued.md", false),
        &queued_workspace,
        vec!["read".to_string()],
    );

    let paused_run = state
        .create_automation_v2_run(&paused_automation, "manual")
        .await
        .expect("create paused run");
    let queued_run = state
        .create_automation_v2_run(&queued_automation, "manual")
        .await
        .expect("create queued run");

    state
        .update_automation_v2_run(&paused_run.run_id, |row| {
            row.status = AutomationRunStatus::Paused;
            row.pause_reason = Some("paused for recovery test".to_string());
            row.detail = Some("paused for recovery test".to_string());
            row.active_session_ids.clear();
            row.active_instance_ids.clear();
        })
        .await
        .expect("mark paused");

    let recovered = state.recover_in_flight_runs().await;
    assert_eq!(recovered, 0);

    let scheduler = state.automation_scheduler.read().await;
    assert!(!scheduler
        .locked_workspaces
        .contains_key(&paused_workspace.to_string_lossy().to_string()));
    assert!(!scheduler
        .locked_workspaces
        .contains_key(&queued_workspace.to_string_lossy().to_string()));
    drop(scheduler);

    let paused_persisted = state
        .get_automation_v2_run(&paused_run.run_id)
        .await
        .expect("paused run");
    let queued_persisted = state
        .get_automation_v2_run(&queued_run.run_id)
        .await
        .expect("queued run");
    assert_eq!(paused_persisted.status, AutomationRunStatus::Paused);
    assert_eq!(queued_persisted.status, AutomationRunStatus::Queued);

    let _ = std::fs::remove_dir_all(&paused_workspace);
    let _ = std::fs::remove_dir_all(&queued_workspace);
}

#[tokio::test]
async fn provider_usage_is_attributed_from_correlation_id_without_session_mapping() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-usage-correlation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut state = ready_test_state().await;
    state.token_cost_per_1k_usd = 12.5;

    let usage_aggregator = tokio::spawn(run_usage_aggregator(state.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let automation = automation_with_single_node(
        "automation-usage-correlation",
        brief_research_node("usage_node", ".tandem/artifacts/usage.md", false),
        &workspace_root,
        vec!["read".to_string()],
    );
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    state.event_bus.publish(EngineEvent::new(
        "provider.usage",
        json!({
            "sessionID": "session-unused",
            "correlationID": format!("automation-v2:{}", run.run_id),
            "messageID": "message-usage",
            "promptTokens": 11,
            "completionTokens": 19,
            "totalTokens": 30,
        }),
    ));

    let updated = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Some(run) = state.get_automation_v2_run(&run.run_id).await {
                if run.total_tokens == 30 {
                    return run;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("usage attribution timeout");

    assert_eq!(updated.prompt_tokens, 11);
    assert_eq!(updated.completion_tokens, 19);
    assert_eq!(updated.total_tokens, 30);
    assert!(updated.estimated_cost_usd > 0.0);
    assert!(
        (updated.estimated_cost_usd - 0.375).abs() < 0.000_001,
        "expected estimated cost to be derived from usage"
    );

    usage_aggregator.abort();
    let _ = usage_aggregator.await;
    let _ = std::fs::remove_dir_all(&workspace_root);
}
