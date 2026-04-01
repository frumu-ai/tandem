// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{json, Value};
use tandem_plan_compiler::api::{
    build_workflow_plan_with_planner_json, compare_workflow_plan_preview_replay_with_revision,
    compile_mission_blueprint_preview, compile_workflow_plan_preview_package_with_revision,
    default_fallback_schedule_json, default_fallback_step_json, load_workflow_plan_draft,
    revise_workflow_plan_draft_json, store_preview_draft, summarize_mission_coder_run_handoffs,
    summarize_mission_execution_boundary, workflow_plan_to_json, ApprovalMode, Clock,
    CommunicationModel, ContextObject, ContextObjectProvenance, ContextObjectScope,
    ContextValidationStatus, CredentialBindingRef, CredentialEnvelope, InterRoutinePolicy,
    McpToolCatalog, MissionBlueprintPreview, MissionDefinition, PeerVisibility, PlanLifecycleState,
    PlanOwner, PlanPackage, PlanReplayReport, PlanStore, PlannerBuildConfig,
    PlannerBuildRequestJson, PlannerDraftRevisionResultJson, PlannerLlmInvocation,
    PlannerLlmInvoker, PlannerLoopConfig, PlannerModelRegistry, PlannerSessionStore,
    RoutineSemanticKind, TelemetrySink, WorkflowPlanJson, WorkspaceResolver,
};
use tandem_workflows::{
    ApprovalDecision, HumanApprovalGate, MissionBlueprint, MissionTeamBlueprint,
    OutputContractBlueprint, ReviewStage, ReviewStageKind, WorkstreamBlueprint,
};

struct ApiTestHost {
    drafts: Mutex<HashMap<String, Value>>,
    provider_configured: bool,
    resolved_workspace_root: String,
    capability_summary: Value,
    llm_response: Mutex<Option<Value>>,
    now_ms: AtomicU64,
}

impl ApiTestHost {
    fn new() -> Self {
        Self {
            drafts: Mutex::new(HashMap::new()),
            provider_configured: true,
            resolved_workspace_root: "/repo".to_string(),
            capability_summary: json!({}),
            llm_response: Mutex::new(None),
            now_ms: AtomicU64::new(1),
        }
    }

    fn with_llm_response(self, response: Value) -> Self {
        *self.llm_response.lock().unwrap() = Some(response);
        self
    }
}

#[async_trait]
impl WorkspaceResolver for ApiTestHost {
    async fn resolve_workspace_root(&self, _requested: Option<&str>) -> Result<String, String> {
        Ok(self.resolved_workspace_root.clone())
    }
}

#[async_trait]
impl PlanStore for ApiTestHost {
    async fn get_draft(&self, plan_id: &str) -> Result<Option<Value>, String> {
        Ok(self.drafts.lock().unwrap().get(plan_id).cloned())
    }

    async fn put_draft(&self, plan_id: &str, draft: Value) -> Result<(), String> {
        self.drafts
            .lock()
            .unwrap()
            .insert(plan_id.to_string(), draft);
        Ok(())
    }
}

#[async_trait]
impl PlannerSessionStore for ApiTestHost {
    async fn create_planner_session(
        &self,
        _title: &str,
        _workspace_root: &str,
    ) -> Result<String, String> {
        Ok("session_1".to_string())
    }

    async fn append_planner_user_prompt(
        &self,
        _session_id: &str,
        _prompt: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn append_planner_assistant_response(
        &self,
        _session_id: &str,
        _response: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

impl Clock for ApiTestHost {
    fn now_ms(&self) -> u64 {
        self.now_ms.fetch_add(1, Ordering::SeqCst)
    }
}

#[async_trait]
impl PlannerModelRegistry for ApiTestHost {
    async fn is_provider_configured(&self, _provider_id: &str) -> bool {
        self.provider_configured
    }
}

#[async_trait]
impl McpToolCatalog for ApiTestHost {
    async fn capability_summary(&self, _allowed_mcp_servers: &[String]) -> Value {
        self.capability_summary.clone()
    }
}

#[async_trait]
impl PlannerLlmInvoker for ApiTestHost {
    async fn invoke_planner_llm(
        &self,
        _invocation: PlannerLlmInvocation,
    ) -> Result<Value, tandem_plan_compiler::api::PlannerInvocationFailure> {
        self.llm_response.lock().unwrap().take().ok_or_else(|| {
            tandem_plan_compiler::api::PlannerInvocationFailure {
                reason: "no_test_llm_response".to_string(),
                detail: None,
            }
        })
    }
}

impl TelemetrySink for ApiTestHost {}

#[tokio::test]
async fn curated_api_supports_json_build_and_revision_flow() {
    let host = ApiTestHost::new().with_llm_response(json!({
        "action": "clarify",
        "assistant_text": "Need one more detail.",
        "clarifier": {
            "field": "general",
            "question": "What is the primary goal?",
            "options": []
        }
    }));

    let request = PlannerBuildRequestJson {
        plan_id: "plan_api".to_string(),
        planner_version: "test_planner_v1".to_string(),
        plan_source: "test".to_string(),
        prompt: "Build a workflow.".to_string(),
        normalized_prompt: "build a workflow".to_string(),
        title: "Example".to_string(),
        fallback_schedule: default_fallback_schedule_json(),
        explicit_schedule: None,
        requested_workspace_root: Some("/repo".to_string()),
        allowed_mcp_servers: vec!["github".to_string()],
        operator_preferences: Some(json!({
            "model_provider": "test_provider",
            "model_id": "test_model"
        })),
    };

    let build = build_workflow_plan_with_planner_json(
        &host,
        request,
        PlannerBuildConfig {
            session_title: "test".to_string(),
            timeout_ms: 10_000,
            override_env: "".to_string(),
        },
    )
    .await;

    assert_eq!(build.plan.plan_id, "plan_api");
    assert_eq!(build.plan.workspace_root, "/repo");
    assert_eq!(
        build.assistant_text.as_deref(),
        Some("Need one more detail.")
    );

    store_preview_draft(&host, build.plan.clone(), build.planner_diagnostics.clone())
        .await
        .unwrap();
    let loaded = load_workflow_plan_draft::<Value, Value, Value, _>(&host, "plan_api")
        .await
        .unwrap();
    assert_eq!(loaded.current_plan.plan_id, "plan_api");

    let revising_host = ApiTestHost::new().with_llm_response(json!({
        "action": "keep",
        "assistant_text": "Keeping the current plan."
    }));
    revising_host
        .put_draft("plan_api", serde_json::to_value(loaded).unwrap())
        .await
        .unwrap();

    let revision: PlannerDraftRevisionResultJson = revise_workflow_plan_draft_json(
        &revising_host,
        "plan_api",
        "Keep it the same.",
        PlannerLoopConfig {
            session_title: "test".to_string(),
            timeout_ms: 10_000,
            override_env: "".to_string(),
        },
    )
    .await
    .unwrap();

    assert_eq!(revision.draft.current_plan.plan_id, "plan_api");
    assert_eq!(revision.assistant_text, "Keeping the current plan.");
}

#[test]
fn curated_api_exports_revisioned_preview_helpers() {
    let plan: WorkflowPlanJson = WorkflowPlanJson {
        plan_id: "plan_revision_api".to_string(),
        planner_version: "planner_v1".to_string(),
        plan_source: "test".to_string(),
        original_prompt: "Build a workflow.".to_string(),
        normalized_prompt: "Build a workflow.".to_string(),
        confidence: "medium".to_string(),
        title: "Example".to_string(),
        description: None,
        schedule: default_fallback_schedule_json(),
        execution_target: "automation_v2".to_string(),
        workspace_root: "/repo".to_string(),
        steps: vec![default_fallback_step_json()],
        requires_integrations: Vec::new(),
        allowed_mcp_servers: vec!["github".to_string()],
        operator_preferences: Some(json!({})),
        save_options: json!({}),
    };

    let current = compile_workflow_plan_preview_package_with_revision(&plan, Some("previewer"), 2);
    let previous = compile_workflow_plan_preview_package_with_revision(&plan, Some("previewer"), 1);
    let workflow_json = workflow_plan_to_json(&plan).unwrap();
    let report: PlanReplayReport =
        compare_workflow_plan_preview_replay_with_revision(&plan, 2, &plan, 1);

    assert_eq!(current.plan_revision, 2);
    assert_eq!(previous.plan_revision, 1);
    assert_eq!(workflow_json.plan_id, plan.plan_id);
    assert!(report.compatible);
    assert_eq!(report.previous_plan_revision, Some(1));
    assert_eq!(report.next_plan_revision, Some(2));
}

#[test]
fn curated_api_supports_mission_preview_types() {
    let blueprint = MissionBlueprint {
        mission_id: "mission_api".to_string(),
        title: "Example Mission".to_string(),
        goal: "Ship a small feature.".to_string(),
        success_criteria: vec!["Tests pass".to_string()],
        shared_context: None,
        workspace_root: "/repo".to_string(),
        orchestrator_template_id: None,
        phases: Vec::new(),
        milestones: Vec::new(),
        team: MissionTeamBlueprint::default(),
        workstreams: vec![WorkstreamBlueprint {
            workstream_id: "workstream_1".to_string(),
            title: "Implement".to_string(),
            objective: "Add the feature".to_string(),
            role: "worker".to_string(),
            priority: None,
            phase_id: None,
            lane: None,
            milestone: None,
            template_id: None,
            prompt: "Do the work.".to_string(),
            model_override: None,
            tool_allowlist_override: Vec::new(),
            mcp_servers_override: Vec::new(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: OutputContractBlueprint {
                kind: "report_markdown".to_string(),
                schema: None,
                summary_guidance: None,
            },
            retry_policy: None,
            timeout_ms: None,
            metadata: None,
        }],
        review_stages: vec![ReviewStage {
            stage_id: "approval_1".to_string(),
            stage_kind: ReviewStageKind::Approval,
            title: "Approve".to_string(),
            priority: None,
            phase_id: None,
            lane: None,
            milestone: None,
            target_ids: vec!["workstream_1".to_string()],
            role: None,
            template_id: None,
            prompt: "".to_string(),
            checklist: Vec::new(),
            model_override: None,
            tool_allowlist_override: Vec::new(),
            mcp_servers_override: Vec::new(),
            gate: Some(HumanApprovalGate {
                required: true,
                decisions: vec![
                    ApprovalDecision::Approve,
                    ApprovalDecision::Rework,
                    ApprovalDecision::Cancel,
                ],
                rework_targets: vec!["workstream_1".to_string()],
                instructions: Some("Approve the completed workstream.".to_string()),
            }),
        }],
        metadata: None,
    };

    let preview = compile_mission_blueprint_preview(blueprint).unwrap();
    let roundtrip =
        serde_json::from_value::<MissionBlueprintPreview>(serde_json::to_value(preview).unwrap())
            .unwrap();
    assert_eq!(roundtrip.work_items.len(), 2);
    assert!(roundtrip
        .node_previews
        .iter()
        .any(|node| node.execution_kind == "coder_run"));
    assert!(roundtrip
        .node_previews
        .iter()
        .any(|node| node.execution_kind == "governance"));
    assert_eq!(
        roundtrip
            .work_items
            .iter()
            .find(|item| item.work_item_id == "workstream_1")
            .and_then(|item| item.metadata.as_ref())
            .and_then(|metadata| metadata.get("execution_kind"))
            .and_then(Value::as_str),
        Some("coder_run")
    );
    assert_eq!(
        roundtrip
            .work_items
            .iter()
            .find(|item| item.work_item_id == "approval_1")
            .and_then(|item| item.metadata.as_ref())
            .and_then(|metadata| metadata.get("execution_kind"))
            .and_then(Value::as_str),
        Some("governance")
    );

    let summary = summarize_mission_execution_boundary(&roundtrip);
    assert_eq!(summary.total_nodes, 2);
    assert_eq!(summary.coder_run_node_ids, vec!["workstream_1".to_string()]);
    assert_eq!(summary.governance_node_ids, vec!["approval_1".to_string()]);

    let coder_handoffs = summarize_mission_coder_run_handoffs(&roundtrip);
    assert_eq!(coder_handoffs.len(), 1);
    let handoff = &coder_handoffs[0];
    assert_eq!(handoff.mission_id, "mission_api");
    assert_eq!(handoff.node_id, "workstream_1");
    assert_eq!(handoff.role, "worker");
    assert_eq!(handoff.objective, "Add the feature");
    assert_eq!(handoff.workspace_root, "/repo");
    assert_eq!(handoff.execution_kind, "coder_run");
}

#[test]
fn curated_api_hardens_mission_coder_launch_contract() {
    let blueprint = MissionBlueprint {
        mission_id: "mission_hardened".to_string(),
        title: "Grouped Mission".to_string(),
        goal: "Ship a grouped slice.".to_string(),
        success_criteria: vec!["Grouped work is ready to hand off.".to_string()],
        shared_context: Some("Clustered by release.".to_string()),
        workspace_root: "/repo".to_string(),
        orchestrator_template_id: Some("orchestrator_template".to_string()),
        phases: vec![tandem_workflows::MissionPhaseBlueprint {
            phase_id: "phase_coding".to_string(),
            title: "Coding".to_string(),
            description: Some("Coder execution phase".to_string()),
            execution_mode: Some(tandem_workflows::MissionPhaseExecutionMode::Soft),
        }],
        milestones: vec![tandem_workflows::MissionMilestoneBlueprint {
            milestone_id: "milestone_alpha".to_string(),
            title: "Alpha".to_string(),
            description: Some("First grouped slice milestone".to_string()),
            phase_id: Some("phase_coding".to_string()),
            required_stage_ids: vec!["approval_1".to_string()],
        }],
        team: MissionTeamBlueprint::default(),
        workstreams: vec![WorkstreamBlueprint {
            workstream_id: "workstream_1".to_string(),
            title: "Implement".to_string(),
            objective: "Add the feature".to_string(),
            role: "worker".to_string(),
            priority: Some(2),
            phase_id: Some("phase_coding".to_string()),
            lane: Some("coding".to_string()),
            milestone: Some("milestone_alpha".to_string()),
            template_id: Some("template_worker".to_string()),
            prompt: "Do the work.".to_string(),
            model_override: None,
            tool_allowlist_override: vec!["read".to_string(), "write".to_string()],
            mcp_servers_override: vec!["github".to_string()],
            depends_on: vec!["approval_1".to_string()],
            input_refs: Vec::new(),
            output_contract: OutputContractBlueprint {
                kind: "report_markdown".to_string(),
                schema: None,
                summary_guidance: Some("Summarize the coder launch contract.".to_string()),
            },
            retry_policy: None,
            timeout_ms: Some(15_000),
            metadata: Some(json!({
                "surface": "tandem",
                "workflow_kind": "coder_run",
                "preset_id": "issue_fix",
                "launch_source": "mission",
            })),
        }],
        review_stages: vec![ReviewStage {
            stage_id: "approval_1".to_string(),
            stage_kind: ReviewStageKind::Approval,
            title: "Approve".to_string(),
            priority: Some(1),
            phase_id: Some("phase_coding".to_string()),
            lane: Some("governance".to_string()),
            milestone: Some("milestone_alpha".to_string()),
            target_ids: vec!["workstream_1".to_string()],
            role: Some("orchestrator".to_string()),
            template_id: Some("template_approval".to_string()),
            prompt: "Approve the completed workstream.".to_string(),
            checklist: vec!["Contract preserved".to_string()],
            model_override: None,
            tool_allowlist_override: Vec::new(),
            mcp_servers_override: Vec::new(),
            gate: Some(HumanApprovalGate {
                required: true,
                decisions: vec![ApprovalDecision::Approve, ApprovalDecision::Rework],
                rework_targets: vec!["workstream_1".to_string()],
                instructions: Some("Approve the launch contract.".to_string()),
            }),
        }],
        metadata: Some(json!({
            "coder": {
                "surface": "tandem",
                "workflow_kind": "coder_run",
                "preset_id": "issue_fix",
                "launch_source": "mission",
            }
        })),
    };

    let preview = compile_mission_blueprint_preview(blueprint).unwrap();
    let summary = summarize_mission_execution_boundary(&preview);
    assert_eq!(summary.coder_run_node_ids, vec!["workstream_1".to_string()]);
    assert_eq!(summary.governance_node_ids, vec!["approval_1".to_string()]);

    let coder_handoffs = summarize_mission_coder_run_handoffs(&preview);
    assert_eq!(coder_handoffs.len(), 1);
    let handoff = &coder_handoffs[0];
    assert_eq!(handoff.node_id, "workstream_1");
    assert_eq!(handoff.execution_kind, "coder_run");
    assert_eq!(handoff.phase_id.as_deref(), Some("phase_coding"));
    assert_eq!(handoff.lane.as_deref(), Some("coding"));
    assert_eq!(handoff.milestone.as_deref(), Some("milestone_alpha"));
    assert_eq!(handoff.template_id.as_deref(), Some("template_worker"));
    assert_eq!(handoff.depends_on, vec!["approval_1".to_string()]);
    assert_eq!(
        handoff.tool_allowlist,
        vec!["read".to_string(), "write".to_string()]
    );
    assert_eq!(handoff.mcp_servers, vec!["github".to_string()]);
    assert_eq!(
        handoff
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("surface"))
            .and_then(Value::as_str),
        Some("tandem")
    );
    assert_eq!(
        handoff
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("workflow_kind"))
            .and_then(Value::as_str),
        Some("coder_run")
    );
    assert_eq!(
        handoff
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("launch_source"))
            .and_then(Value::as_str),
        Some("mission")
    );
}

#[test]
fn curated_api_supports_plan_package_types() {
    let package = PlanPackage {
        plan_id: "plan_api".to_string(),
        plan_revision: 1,
        lifecycle_state: PlanLifecycleState::Preview,
        owner: PlanOwner {
            owner_id: "evan".to_string(),
            scope: "workspace".to_string(),
            audience: "internal".to_string(),
        },
        mission: MissionDefinition {
            goal: "Operationalize a mission.".to_string(),
            summary: Some("Preview a governed plan package".to_string()),
            domain: Some("mixed".to_string()),
        },
        success_criteria: Default::default(),
        budget_policy: None,
        budget_enforcement: None,
        approval_policy: None,
        inter_routine_policy: Some(InterRoutinePolicy {
            communication_model: CommunicationModel::ArtifactOnly,
            shared_memory_access: false,
            shared_memory_justification: None,
            peer_visibility: PeerVisibility::DeclaredOutputsOnly,
            artifact_handoff_validation: true,
        }),
        trigger_policy: None,
        output_roots: None,
        precedence_log: Vec::new(),
        plan_diff: None,
        manual_trigger_record: None,
        validation_state: None,
        overlap_policy: None,
        routine_graph: vec![tandem_plan_compiler::api::RoutinePackage {
            routine_id: "routine_1".to_string(),
            semantic_kind: RoutineSemanticKind::Research,
            trigger: tandem_plan_compiler::api::TriggerDefinition {
                trigger_type: tandem_plan_compiler::api::TriggerKind::Manual,
                schedule: None,
                timezone: None,
            },
            dependencies: Vec::new(),
            dependency_resolution: tandem_plan_compiler::api::DependencyResolution {
                strategy:
                    tandem_plan_compiler::api::DependencyResolutionStrategy::TopologicalSequential,
                partial_failure_mode:
                    tandem_plan_compiler::api::PartialFailureMode::PauseDownstreamOnly,
                reentry_point: tandem_plan_compiler::api::ReentryPoint::FailedStep,
                mid_routine_connector_failure:
                    tandem_plan_compiler::api::MidRoutineConnectorFailureMode::SurfaceAndPause,
            },
            connector_resolution: Default::default(),
            data_scope: tandem_plan_compiler::api::DataScope {
                readable_paths: vec!["mission.goal".to_string()],
                writable_paths: vec!["knowledge/workflows/drafts/routine_1/**".to_string()],
                denied_paths: vec!["credentials/**".to_string()],
                cross_routine_visibility: tandem_plan_compiler::api::CrossRoutineVisibility::None,
                mission_context_scope:
                    tandem_plan_compiler::api::MissionContextScope::GoalAndOwnRoutine,
                mission_context_justification: None,
            },
            audit_scope: tandem_plan_compiler::api::AuditScope {
                run_history_visibility: tandem_plan_compiler::api::RunHistoryVisibility::PlanOwner,
                named_audit_roles: Vec::new(),
                intermediate_artifact_visibility:
                    tandem_plan_compiler::api::IntermediateArtifactVisibility::RoutineOnly,
                final_artifact_visibility:
                    tandem_plan_compiler::api::FinalArtifactVisibility::DeclaredConsumers,
            },
            success_criteria: Default::default(),
            steps: vec![tandem_plan_compiler::api::StepPackage {
                step_id: "step_1".to_string(),
                label: "Collect".to_string(),
                kind: "research".to_string(),
                action: "collect inputs".to_string(),
                inputs: Vec::new(),
                outputs: vec!["artifact".to_string()],
                dependencies: Vec::new(),
                context_reads: Vec::new(),
                context_writes: vec!["ctx:routine_1:step_1:artifact.md".to_string()],
                connector_requirements: Vec::new(),
                model_policy: Default::default(),
                approval_policy: ApprovalMode::InternalOnly,
                success_criteria: Default::default(),
                failure_policy: Default::default(),
                retry_policy: Default::default(),
                artifacts: vec!["artifact.md".to_string()],
                provenance: None,
                notes: None,
            }],
        }],
        connector_intents: Vec::new(),
        connector_bindings: Vec::new(),
        connector_binding_resolution: None,
        model_routing_resolution: None,
        credential_envelopes: vec![CredentialEnvelope {
            routine_id: "routine_1".to_string(),
            entitled_connectors: vec![CredentialBindingRef {
                capability: "github".to_string(),
                binding_id: "binding_1".to_string(),
            }],
            denied_connectors: Vec::new(),
            envelope_issued_at: None,
            envelope_expires_at: None,
            issuing_authority: Some("engine".to_string()),
        }],
        context_objects: vec![ContextObject {
            context_object_id: "ctx:routine_1:step_1:artifact.md".to_string(),
            name: "Step 1 handoff".to_string(),
            kind: "step_output_handoff".to_string(),
            scope: ContextObjectScope::Handoff,
            owner_routine_id: "routine_1".to_string(),
            producer_step_id: Some("step_1".to_string()),
            declared_consumers: vec!["routine_1".to_string()],
            artifact_ref: Some("artifact.md".to_string()),
            data_scope_refs: vec!["knowledge/workflows/drafts/routine_1/**".to_string()],
            freshness_window_hours: None,
            validation_status: ContextValidationStatus::Pending,
            provenance: ContextObjectProvenance {
                plan_id: "plan_api".to_string(),
                routine_id: "routine_1".to_string(),
                step_id: Some("step_1".to_string()),
            },
            summary: None,
        }],
        metadata: None,
    };

    let roundtrip: PlanPackage =
        serde_json::from_value(serde_json::to_value(package).unwrap()).unwrap();
    assert_eq!(roundtrip.plan_id, "plan_api");
    assert_eq!(roundtrip.routine_graph.len(), 1);
    assert!(roundtrip.inter_routine_policy.is_some());
    assert!(
        roundtrip
            .inter_routine_policy
            .as_ref()
            .unwrap()
            .artifact_handoff_validation
    );
    assert_eq!(roundtrip.credential_envelopes.len(), 1);
    assert_eq!(
        roundtrip.credential_envelopes[0].entitled_connectors.len(),
        1
    );
    assert_eq!(roundtrip.context_objects.len(), 1);
    assert_eq!(
        roundtrip.context_objects[0].context_object_id,
        "ctx:routine_1:step_1:artifact.md"
    );
    assert!(tandem_plan_compiler::api::can_transition_plan_lifecycle(
        PlanLifecycleState::Preview,
        PlanLifecycleState::AwaitingApproval
    ));
}
