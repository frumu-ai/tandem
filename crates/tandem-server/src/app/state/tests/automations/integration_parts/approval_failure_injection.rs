fn approval_send_restart_automation(
    automation_id: &str,
    workspace_root: &std::path::Path,
    send_retry_policy: serde_json::Value,
) -> AutomationV2Spec {
    let mut approval = AutomationNodeBuilder::new("approve_consequential_delivery")
        .objective("Review the prepared customer update before it can be sent")
        .stage_kind(AutomationNodeStageKind::Approval)
        .metadata(json!({
            "builder": {
                "title": "Customer update approval",
                "role": "approver"
            }
        }))
        .build();
    approval.output_contract = Some(AutomationFlowOutputContract {
        kind: "approval_gate".to_string(),
        validator: None,
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    approval.gate = Some(AutomationApprovalGate {
        required: true,
        decisions: vec![
            "approve".to_string(),
            "rework".to_string(),
            "cancel".to_string(),
        ],
        rework_targets: Vec::new(),
        instructions: Some("Approve the customer update before sending.".to_string()),
        expiry_policy: None,
    });

    let mut send = AutomationNodeBuilder::new("send_approved_update")
        .objective("Record the approved customer update in the outbound ledger")
        .depends_on(vec!["approve_consequential_delivery"])
        .stage_kind(AutomationNodeStageKind::Workstream)
        .metadata(json!({
            "builder": {
                "task_kind": "approved_ledger_record",
                "title": "Approved customer update ledger record"
            }
        }))
        .build();
    send.retry_policy = Some(send_retry_policy);

    let mut automation = AutomationSpecBuilder::new(automation_id)
        .name(format!("{automation_id} approval failure injection test"))
        .nodes(vec![approval, send])
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .build();
    for agent in &mut automation.agents {
        agent.template_id = None;
        agent.tool_policy.allowlist = vec!["write".to_string()];
        agent.tool_policy.denylist.clear();
        agent.mcp_policy.allowed_servers = Vec::new();
        agent.mcp_policy.allowed_tools = None;
    }
    automation
}

#[derive(Clone)]
struct WorkspaceWriteTool {
    workspace_root: std::path::PathBuf,
    calls: Arc<Mutex<Vec<Value>>>,
}

impl WorkspaceWriteTool {
    fn new(workspace_root: &std::path::Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn calls(&self) -> Vec<Value> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl Tool for WorkspaceWriteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "write",
            "Write a workspace file",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.calls.lock().await.push(args.clone());
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("write path is required"))?;
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("write content is required"))?;
        let path = self.workspace_root.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        Ok(ToolResult {
            output: "ok".to_string(),
            metadata: json!({ "path": path }),
        })
    }
}

async fn install_workspace_write_tool(
    state: &AppState,
    workspace_root: &std::path::Path,
) -> Arc<WorkspaceWriteTool> {
    let tool = Arc::new(WorkspaceWriteTool::new(workspace_root));
    state
        .tools
        .register_tool("write".to_string(), tool.clone())
        .await;
    tool
}

fn approved_send_output_path(automation: &AutomationV2Spec, run_id: &str) -> String {
    let node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "send_approved_update")
        .expect("send node");
    automation_node_required_output_path_for_run(node, Some(run_id)).expect("send output path")
}

fn approved_send_tool_script(output_path: &str) -> Vec<StreamChunk> {
    tool_turn(vec![(
        "write-approved-ledger",
        "write",
        json!({
            "path": output_path,
            "content": "{\n  \"status\": \"completed\",\n  \"approved\": true\n}\n"
        }),
    )])
}

async fn apply_gate_decision_for_test(
    state: AppState,
    automation: AutomationV2Spec,
    run_id: String,
    gate: AutomationPendingGate,
    decision: &'static str,
    reason: &'static str,
) -> &'static str {
    let mut outcome = "missing";
    state
        .update_automation_v2_run(&run_id, |row| {
            outcome = match crate::app::state::apply_automation_gate_decision(
                row,
                &automation,
                &gate,
                decision,
                Some(reason.to_string()),
                None,
            ) {
                crate::app::state::AutomationGateDecisionOutcome::Applied => "applied",
                crate::app::state::AutomationGateDecisionOutcome::AlreadyDecided(_) => {
                    "already_decided"
                }
            };
        })
        .await
        .expect("apply gate decision");
    outcome
}

#[test]
fn approval_failure_injection_concurrent_approvals_send_once() {
    run_restart_resume_test_with_large_stack(|| async {
        let workspace_root = restart_test_workspace("tandem-approval-concurrent");
        let state = ready_test_state().await;
        let provider = ScriptedProvider::new();
        install_provider_and_tools(&state, &provider, Vec::new()).await;
        let write_tool = install_workspace_write_tool(&state, &workspace_root).await;
        let automation = approval_send_restart_automation(
            "automation-approval-concurrent",
            &workspace_root,
            json!({ "max_attempts": 1 }),
        );
        let run = create_persisted_restart_run(&state, &automation).await;
        let awaiting = claim_and_drain_restart_run(&state, &run.run_id).await;
        assert_eq!(awaiting.status, AutomationRunStatus::AwaitingApproval);
        let gate = awaiting
            .checkpoint
            .awaiting_gate
            .clone()
            .expect("pending approval gate");

        let first = apply_gate_decision_for_test(
            state.clone(),
            automation.clone(),
            run.run_id.clone(),
            gate.clone(),
            "approve",
            "first concurrent approval",
        );
        let second = apply_gate_decision_for_test(
            state.clone(),
            automation.clone(),
            run.run_id.clone(),
            gate.clone(),
            "approve",
            "second concurrent approval",
        );
        let (first, second) = tokio::join!(first, second);
        let mut outcomes = vec![first, second];
        outcomes.sort();
        assert_eq!(outcomes, vec!["already_decided", "applied"]);

        let approved = state
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("approved run");
        assert_eq!(approved.status, AutomationRunStatus::Queued);
        assert_eq!(approved.checkpoint.gate_history.len(), 1);
        assert_eq!(approved.checkpoint.gate_history[0].decision, "approve");

        let output_path = approved_send_output_path(&automation, &run.run_id);
        provider
            .push_script(approved_send_tool_script(&output_path))
            .await;
        provider
            .push_script(final_turn(
                "Recorded the approved update.\n\n{\"status\":\"completed\"}",
            ))
            .await;
        let terminal = claim_and_drain_restart_run(&state, &run.run_id).await;
        assert_eq!(
            terminal.status,
            AutomationRunStatus::Completed,
            "detail={:?}, outputs={:?}, attempts={:?}, last_failure={:?}",
            terminal.detail,
            terminal.checkpoint.node_outputs,
            terminal.checkpoint.node_attempts,
            terminal.checkpoint.last_failure
        );
        assert_eq!(
            terminal
                .checkpoint
                .node_attempts
                .get("send_approved_update"),
            Some(&1)
        );
        assert_eq!(write_tool.calls().await.len(), 1);
        assert_eq!(terminal.checkpoint.gate_history.len(), 1);
        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}

#[test]
fn approval_failure_injection_provider_failure_after_approval_does_not_reask_gate() {
    run_restart_resume_test_with_large_stack(|| async {
        let workspace_root = restart_test_workspace("tandem-approval-provider-retry");
        let state = ready_test_state().await;
        let provider = ScriptedProvider::new();
        install_provider_and_tools(&state, &provider, Vec::new()).await;
        let write_tool = install_workspace_write_tool(&state, &workspace_root).await;
        let automation = approval_send_restart_automation(
            "automation-approval-provider-retry",
            &workspace_root,
            json!({ "max_attempts": 2 }),
        );
        let run = create_persisted_restart_run(&state, &automation).await;
        let awaiting = claim_and_drain_restart_run(&state, &run.run_id).await;
        assert_eq!(awaiting.status, AutomationRunStatus::AwaitingApproval);

        let approved = approve_restart_gate_once(&state, &automation, &run.run_id, true).await;
        assert_eq!(approved.status, AutomationRunStatus::Queued);
        provider
            .push_error("synthetic send provider failure after approval")
            .await;
        let output_path = approved_send_output_path(&automation, &run.run_id);
        provider
            .push_script(approved_send_tool_script(&output_path))
            .await;
        provider
            .push_script(final_turn(
                "Recorded the approved update after retry.\n\n{\"status\":\"completed\"}",
            ))
            .await;

        let terminal = claim_and_drain_restart_run(&state, &run.run_id).await;
        assert_eq!(
            terminal.status,
            AutomationRunStatus::Completed,
            "detail={:?}, outputs={:?}, attempts={:?}, last_failure={:?}",
            terminal.detail,
            terminal.checkpoint.node_outputs,
            terminal.checkpoint.node_attempts,
            terminal.checkpoint.last_failure
        );
        assert!(terminal.checkpoint.awaiting_gate.is_none());
        assert_eq!(terminal.checkpoint.gate_history.len(), 1);
        assert_eq!(terminal.checkpoint.gate_history[0].decision, "approve");
        assert_eq!(
            terminal
                .checkpoint
                .node_attempts
                .get("send_approved_update"),
            Some(&2)
        );
        assert_eq!(write_tool.calls().await.len(), 1);
        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}

#[test]
fn approval_failure_injection_restart_recovers_half_applied_gate_decision() {
    run_restart_resume_test_with_large_stack(|| async {
        let workspace_root = restart_test_workspace("tandem-approval-half-applied");
        let state = ready_test_state().await;
        let automation = approval_send_restart_automation(
            "automation-approval-half-applied",
            &workspace_root,
            json!({ "max_attempts": 1 }),
        );
        let run = create_persisted_restart_run(&state, &automation).await;
        let awaiting = claim_and_drain_restart_run(&state, &run.run_id).await;
        assert_eq!(awaiting.status, AutomationRunStatus::AwaitingApproval);
        let gate = awaiting
            .checkpoint
            .awaiting_gate
            .clone()
            .expect("pending approval gate");

        state
            .update_automation_v2_run(&run.run_id, |row| {
                row.detail = Some("simulated crash after gate decision history write".to_string());
                row.checkpoint
                    .gate_history
                    .push(AutomationGateDecisionRecord {
                        node_id: gate.node_id.clone(),
                        decision: "approve".to_string(),
                        reason: Some("recorded before simulated crash".to_string()),
                        decided_at_ms: crate::now_ms(),
                        decided_by: None,
                        metadata: gate.metadata.clone(),
                    });
                row.status = AutomationRunStatus::AwaitingApproval;
                row.checkpoint.awaiting_gate = Some(gate.clone());
            })
            .await
            .expect("write half-applied gate decision");

        let (reloaded, recovered) = reload_automation_state_after_restart(&state).await;
        assert_eq!(recovered, 1);
        let recovered_run = reloaded
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("recovered run");
        assert_eq!(recovered_run.status, AutomationRunStatus::Queued);
        assert!(recovered_run.checkpoint.awaiting_gate.is_none());
        assert_eq!(recovered_run.checkpoint.gate_history.len(), 1);
        assert_eq!(
            recovered_run
                .checkpoint
                .node_outputs
                .get("approve_consequential_delivery")
                .and_then(|output| output.get("status"))
                .and_then(Value::as_str),
            Some("completed")
        );
        assert!(recovered_run
            .checkpoint
            .completed_nodes
            .iter()
            .any(|node_id| node_id == "approve_consequential_delivery"));
        assert!(recovered_run
            .checkpoint
            .pending_nodes
            .iter()
            .any(|node_id| node_id == "send_approved_update"));

        let duplicate = apply_gate_decision_for_test(
            reloaded.clone(),
            automation,
            run.run_id.clone(),
            gate,
            "approve",
            "duplicate after recovery",
        )
        .await;
        assert_eq!(duplicate, "already_decided");
        let after_duplicate = reloaded
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("run after duplicate");
        assert_eq!(after_duplicate.checkpoint.gate_history.len(), 1);
        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}

#[test]
fn approval_failure_injection_stale_gate_decision_is_rejected() {
    run_restart_resume_test_with_large_stack(|| async {
        let workspace_root = restart_test_workspace("tandem-approval-stale-gate");
        let state = ready_test_state().await;
        let automation = approval_send_restart_automation(
            "automation-approval-stale-gate",
            &workspace_root,
            json!({ "max_attempts": 1 }),
        );
        let run = create_persisted_restart_run(&state, &automation).await;
        let awaiting = claim_and_drain_restart_run(&state, &run.run_id).await;
        assert_eq!(awaiting.status, AutomationRunStatus::AwaitingApproval);
        let gate = awaiting
            .checkpoint
            .awaiting_gate
            .clone()
            .expect("pending approval gate");

        state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Cancelled;
                row.detail =
                    Some("cancelled by operator before stale approval arrived".to_string());
                row.stop_kind = Some(AutomationStopKind::Cancelled);
                row.stop_reason = row.detail.clone();
                row.checkpoint.awaiting_gate = None;
            })
            .await
            .expect("cancel run before stale decision");

        let stale = apply_gate_decision_for_test(
            state.clone(),
            automation,
            run.run_id.clone(),
            gate,
            "approve",
            "stale approval",
        )
        .await;
        assert_eq!(stale, "already_decided");
        let after_stale = state
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("run after stale decision");
        assert_eq!(after_stale.status, AutomationRunStatus::Cancelled);
        assert_eq!(after_stale.checkpoint.gate_history.len(), 0);
        assert!(after_stale.checkpoint.awaiting_gate.is_none());
        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}

#[test]
fn approval_failure_injection_corrupt_legacy_checkpoint_is_ignored_after_sqlite_cutover() {
    run_restart_resume_test_with_large_stack(|| async {
        let workspace_root = restart_test_workspace("tandem-approval-corrupt-checkpoint");
        let state = ready_test_state().await;
        let automation = approval_send_restart_automation(
            "automation-approval-corrupt-checkpoint",
            &workspace_root,
            json!({ "max_attempts": 1 }),
        );
        let run = create_persisted_restart_run(&state, &automation).await;
        let raw = std::fs::read_to_string(&state.automation_v2_runs_path).expect("read runs file");
        let mut value: Value = serde_json::from_str(&raw).expect("parse runs file");
        value["runs"][&run.run_id]["checkpoint"] = json!("corrupted checkpoint payload");
        std::fs::write(
            &state.automation_v2_runs_path,
            serde_json::to_string_pretty(&value).expect("serialize corrupt runs file"),
        )
        .expect("write corrupt checkpoint");

        let (reloaded, recovered) = reload_automation_state_after_restart(&state).await;
        assert_eq!(recovered, 0);
        let loaded = reloaded
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("run from SQLite");
        assert_eq!(loaded.status, AutomationRunStatus::Queued);
        assert!(loaded.checkpoint.blocked_nodes.is_empty());
        assert!(loaded.detail.is_none());
        assert!(reloaded
            .claim_specific_automation_v2_run(&run.run_id)
            .await
            .is_some());
        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}
