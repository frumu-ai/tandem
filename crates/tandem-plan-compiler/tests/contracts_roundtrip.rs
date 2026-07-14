// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::{json, Value};
use tandem_plan_compiler::api::{
    compile_mission_blueprint_preview, compile_workflow_plan_preview_package,
    default_fallback_schedule_json, default_fallback_step_json, export_plan_package_bundle,
    preview_plan_package_import_bundle, ApprovalMatrix, ApprovalMode, ConnectorBinding,
    ManualTriggerRecord, ManualTriggerSource, MissionBlueprintPreview, PlanPackageExportBundle,
    PlanPackageImportBundle, PlanPackageImportPreview, PlannerBuildRequestJson,
    PlannerBuildResultJson, WorkflowPlanDraftRecordJson, WorkflowPlanJson,
};
use tandem_workflows::plan_package::{WorkflowPlanConversation, WorkflowPlanDraftRecord};
use tandem_workflows::{
    MissionBlueprint, MissionTeamBlueprint, OutputContractBlueprint, WorkstreamBlueprint,
};

#[test]
fn mission_blueprint_preview_roundtrips_as_json() {
    let blueprint = MissionBlueprint {
        mission_id: "mission_1".to_string(),
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
        review_stages: Vec::new(),
        metadata: None,
    };

    let preview = compile_mission_blueprint_preview(blueprint).unwrap();
    let encoded = serde_json::to_value(&preview).unwrap();
    let decoded = serde_json::from_value::<MissionBlueprintPreview>(encoded.clone()).unwrap();
    let reencoded = serde_json::to_value(&decoded).unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn planner_contracts_roundtrip_without_server_types() {
    let request: PlannerBuildRequestJson = PlannerBuildRequestJson {
        plan_id: "plan_1".to_string(),
        planner_version: "planner_v1".to_string(),
        plan_source: "test".to_string(),
        progress_id: None,
        prompt: "Build a workflow.".to_string(),
        normalized_prompt: "Build a workflow.".to_string(),
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

    let encoded = serde_json::to_value(&request).unwrap();
    let decoded = serde_json::from_value::<PlannerBuildRequestJson>(encoded.clone()).unwrap();
    let reencoded = serde_json::to_value(&decoded).unwrap();
    assert_eq!(encoded, reencoded);

    let plan: WorkflowPlanJson = WorkflowPlanJson {
        plan_id: "plan_1".to_string(),
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

    let preview_package = compile_workflow_plan_preview_package(&plan, Some("previewer"));
    let export_bundle = export_plan_package_bundle(&preview_package);

    assert!(export_bundle.plan.inter_routine_policy.is_some());
    assert_eq!(
        export_bundle.scope_snapshot.plan_revision,
        export_bundle.plan.plan_revision
    );
    assert_eq!(
        export_bundle.scope_snapshot.output_roots,
        export_bundle.plan.output_roots
    );
    assert_eq!(
        export_bundle.scope_snapshot.connector_binding_resolution,
        export_bundle.plan.connector_binding_resolution
    );
    assert!(!export_bundle.plan.credential_envelopes.is_empty());
    assert!(!export_bundle.plan.context_objects.is_empty());

    let encoded = serde_json::to_value(&export_bundle).unwrap();
    let decoded = serde_json::from_value::<PlanPackageExportBundle>(encoded.clone()).unwrap();
    let reencoded = serde_json::to_value(&decoded).unwrap();
    assert_eq!(encoded, reencoded);

    assert!(decoded.plan.inter_routine_policy.is_some());
    assert!(!decoded.plan.credential_envelopes.is_empty());
    assert!(!decoded.plan.context_objects.is_empty());

    let import_bundle = PlanPackageImportBundle {
        bundle_version: export_bundle.bundle_version.clone(),
        plan: export_bundle.plan.clone(),
        scope_snapshot: Some(export_bundle.scope_snapshot.clone()),
    };
    let encoded = serde_json::to_value(&import_bundle).unwrap();
    let decoded = serde_json::from_value::<PlanPackageImportBundle>(encoded.clone()).unwrap();
    let reencoded = serde_json::to_value(decoded).unwrap();
    assert_eq!(encoded, reencoded);

    let import_preview =
        preview_plan_package_import_bundle(&import_bundle, "/workspace", "operator_1");
    let encoded = serde_json::to_value(&import_preview).unwrap();
    let decoded = serde_json::from_value::<PlanPackageImportPreview>(encoded.clone()).unwrap();
    let reencoded = serde_json::to_value(decoded).unwrap();
    assert_eq!(encoded, reencoded);
    assert_eq!(import_preview.plan_package.plan_revision, 1);
    assert_eq!(
        import_preview.plan_package.lifecycle_state,
        tandem_plan_compiler::api::PlanLifecycleState::Preview
    );
    assert_eq!(
        import_preview.derived_scope_snapshot,
        tandem_plan_compiler::api::PlanScopeSnapshot {
            plan_id: import_preview.plan_package.plan_id.clone(),
            plan_revision: import_preview.plan_package.plan_revision,
            output_roots: import_preview.plan_package.output_roots.clone(),
            inter_routine_policy: import_preview.plan_package.inter_routine_policy.clone(),
            budget_enforcement: import_preview.plan_package.budget_enforcement.clone(),
            connector_binding_resolution: import_preview
                .plan_package
                .connector_binding_resolution
                .clone(),
            model_routing_resolution: import_preview.plan_package.model_routing_resolution.clone(),
            credential_envelopes: import_preview.plan_package.credential_envelopes.clone(),
            context_objects: import_preview.plan_package.context_objects.clone(),
            routine_scopes: import_preview.derived_scope_snapshot.routine_scopes.clone(),
        }
    );
    assert_eq!(
        import_preview
            .plan_package
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("import"))
            .and_then(|value| value.get("mode"))
            .and_then(Value::as_str),
        Some("sanitized_local_preview")
    );

    let result: PlannerBuildResultJson = PlannerBuildResultJson {
        plan: plan.clone(),
        assistant_text: Some("Ok.".to_string()),
        clarifier: Value::Null,
        planner_diagnostics: None,
    };

    let encoded = serde_json::to_value(&result).unwrap();
    let decoded = serde_json::from_value::<PlannerBuildResultJson>(encoded.clone()).unwrap();
    let reencoded = serde_json::to_value(decoded).unwrap();
    assert_eq!(encoded, reencoded);

    let draft: WorkflowPlanDraftRecordJson = WorkflowPlanDraftRecord {
        initial_plan: plan.clone(),
        current_plan: plan,
        plan_revision: 1,
        conversation: WorkflowPlanConversation {
            conversation_id: "conv_1".to_string(),
            plan_id: "plan_1".to_string(),
            created_at_ms: 1,
            updated_at_ms: 1,
            messages: Vec::new(),
        },
        planner_diagnostics: None,
        last_success_materialization: Some(json!({
            "plan_id": "plan_1",
            "routine_count": 1,
            "step_count": 1,
            "context_object_count": 0
        })),
        review: None,
    };

    let encoded = serde_json::to_value(&draft).unwrap();
    let decoded = serde_json::from_value::<WorkflowPlanDraftRecordJson>(encoded.clone()).unwrap();
    let reencoded = serde_json::to_value(decoded).unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn manual_trigger_record_roundtrips_as_json() {
    let record = ManualTriggerRecord {
        trigger_id: "mt_01HZY".to_string(),
        plan_id: "plan_0f6e8c".to_string(),
        plan_revision: 3,
        routine_id: "founder_brief_daily".to_string(),
        triggered_by: "user_123".to_string(),
        trigger_source: ManualTriggerSource::Calendar,
        dry_run: true,
        approval_policy_snapshot: Some(ApprovalMatrix {
            internal_reports: Some(ApprovalMode::AutoApproved),
            public_posts: Some(ApprovalMode::ApprovalRequired),
            ..ApprovalMatrix::default()
        }),
        connector_binding_snapshot: vec![ConnectorBinding {
            capability: "gmail".to_string(),
            binding_type: "oauth_integration".to_string(),
            binding_id: "gmail-prod".to_string(),
            allowlist_pattern: Some("gmail.send".to_string()),
            status: "mapped".to_string(),
        }],
        triggered_at: "2026-03-27T09:15:00Z".to_string(),
        run_id: Some("run_abc123".to_string()),
        outcome: Some("paused_after_validation".to_string()),
        artifacts_produced: vec!["founder_brief_draft.md".to_string()],
        notes: Some("Dry-run from calendar entry".to_string()),
    };

    let encoded = serde_json::to_value(&record).unwrap();
    let decoded = serde_json::from_value::<ManualTriggerRecord>(encoded.clone()).unwrap();
    let reencoded = serde_json::to_value(decoded).unwrap();
    assert_eq!(encoded, reencoded);
}
