use crate::types::*;
use serde_json::json;
use serde_json::Value;
use tandem_orchestrator::{KnowledgeReuseMode, KnowledgeTrustLevel};
use tandem_plan_compiler::api::{
    OutputContractSeed, ProjectedAutomationNode, ProjectedMissionInputRef,
};

fn empty_spec() -> AutomationV2Spec {
    AutomationV2Spec {
        automation_id: "auto".to_string(),
        name: "Test".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy::default(),
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

#[test]
fn resolver_with_tenant_default_uses_run_override_first() {
    use crate::execution_profile::ExecutionProfile;
    let mut spec = empty_spec();
    spec.execution.profile = Some(ExecutionProfile::Strict);
    let resolved = resolve_effective_execution_profile_with_tenant(
        &spec,
        Some(ExecutionProfile::Yolo),
        Some(ExecutionProfile::Guided),
    );
    assert_eq!(resolved, ExecutionProfile::Yolo);
}

#[test]
fn resolver_with_tenant_default_uses_workflow_policy_when_no_override() {
    use crate::execution_profile::ExecutionProfile;
    let mut spec = empty_spec();
    spec.execution.profile = Some(ExecutionProfile::Guided);
    let resolved =
        resolve_effective_execution_profile_with_tenant(&spec, None, Some(ExecutionProfile::Yolo));
    assert_eq!(resolved, ExecutionProfile::Guided);
}

#[test]
fn resolver_with_tenant_default_falls_back_to_tenant_when_workflow_unset() {
    use crate::execution_profile::ExecutionProfile;
    let spec = empty_spec();
    let resolved = resolve_effective_execution_profile_with_tenant(
        &spec,
        None,
        Some(ExecutionProfile::Guided),
    );
    assert_eq!(resolved, ExecutionProfile::Guided);
}

#[test]
fn resolver_with_tenant_default_falls_back_to_guided_when_all_unset() {
    use crate::execution_profile::ExecutionProfile;
    let spec = empty_spec();
    let resolved = resolve_effective_execution_profile_with_tenant(&spec, None, None);
    assert_eq!(resolved, ExecutionProfile::Guided);
}

#[test]
fn projected_node_metadata_lifts_knowledge_binding() {
    let projected = ProjectedAutomationNode::<ProjectedMissionInputRef, OutputContractSeed> {
        node_id: "node-a".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Map the topic".to_string(),
        depends_on: vec![],
        input_refs: vec![],
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        partial_failure_mode: Some(
            tandem_plan_compiler::api::PartialFailureMode::ContinueIndependent,
        ),
        metadata: Some(json!({
            "builder": {
                "knowledge": {
                    "enabled": true,
                    "reuse_mode": "preflight",
                    "trust_floor": "promoted",
                    "read_spaces": [{"scope": "project"}],
                    "promote_spaces": [{"scope": "project"}],
                    "subject": "Topic map"
                }
            }
        })),
    };

    let node = AutomationFlowNode::from(projected);
    assert!(node.knowledge.enabled);
    assert_eq!(node.knowledge.reuse_mode, KnowledgeReuseMode::Preflight);
    assert_eq!(node.knowledge.trust_floor, KnowledgeTrustLevel::Promoted);
    assert_eq!(node.knowledge.subject.as_deref(), Some("Topic map"));
    assert_eq!(node.knowledge.read_spaces.len(), 1);
    assert_eq!(node.knowledge.promote_spaces.len(), 1);
    assert_eq!(
        node.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("partial_failure_mode"))
            .and_then(Value::as_str),
        Some("continue_independent")
    );
}

// ── AutomationScopePolicy ────────────────────────────────────────────────

fn open_policy() -> AutomationScopePolicy {
    AutomationScopePolicy::default()
}

fn restricted_policy() -> AutomationScopePolicy {
    AutomationScopePolicy {
        readable_paths: vec!["shared/".to_string(), "job-search/reports/".to_string()],
        writable_paths: vec!["job-search/reports/".to_string()],
        denied_paths: vec!["shared/secrets/".to_string()],
        watch_paths: vec![],
    }
}

#[test]
fn scope_policy_open_allows_any_read() {
    let policy = open_policy();
    assert!(policy.check_read("anything/here.md").is_ok());
    assert!(policy.check_read("shared/secrets/token.txt").is_ok());
}

#[test]
fn scope_policy_open_allows_any_write() {
    let policy = open_policy();
    assert!(policy.check_write("anywhere/file.txt").is_ok());
}

#[test]
fn scope_policy_deny_wins_over_readable() {
    let policy = restricted_policy();
    // shared/secrets/ is explicitly denied, even though "shared/" is readable
    assert!(policy.check_read("shared/secrets/token.txt").is_err());
    assert!(policy.check_write("shared/secrets/token.txt").is_err());
}

#[test]
fn scope_policy_readable_path_allows_read() {
    let policy = restricted_policy();
    assert!(policy
        .check_read("shared/handoffs/approved/handoff.json")
        .is_ok());
}

#[test]
fn scope_policy_unreadable_path_denied() {
    let policy = restricted_policy();
    // "private/" is not in readable_paths
    assert!(policy.check_read("private/notes.md").is_err());
}

#[test]
fn scope_policy_writable_path_allows_write() {
    let policy = restricted_policy();
    assert!(policy.check_write("job-search/reports/week1.md").is_ok());
}

#[test]
fn scope_policy_non_writable_path_denied_for_write() {
    let policy = restricted_policy();
    // "shared/" is readable but not writable
    assert!(policy
        .check_write("shared/handoffs/approved/handoff.json")
        .is_err());
}

#[test]
fn scope_policy_watch_falls_back_to_readable_when_watch_paths_empty() {
    let policy = restricted_policy(); // watch_paths is empty
                                      // watched paths should follow readable_paths
    assert!(policy.check_watch("shared/handoffs/inbox/").is_ok());
    assert!(policy.check_watch("private/something").is_err());
}

#[test]
fn scope_policy_explicit_watch_paths_override_readable() {
    let policy = AutomationScopePolicy {
        readable_paths: vec!["shared/".to_string()],
        writable_paths: vec![],
        denied_paths: vec![],
        watch_paths: vec!["shared/handoffs/inbox/".to_string()],
    };
    // Only the explicit watch path is watchable
    assert!(policy
        .check_watch("shared/handoffs/inbox/alert.json")
        .is_ok());
    // "shared/other/" is readable but not in watch_paths
    assert!(policy.check_watch("shared/other/file.md").is_err());
}

#[test]
fn scope_path_prefix_matches_exact_and_children() {
    assert!(scope_path_matches_prefix("shared", "shared"));
    assert!(scope_path_matches_prefix("shared/foo/bar.json", "shared"));
    assert!(!scope_path_matches_prefix("sharedfoo", "shared")); // no slash boundary
    assert!(!scope_path_matches_prefix("other/shared", "shared"));
}

#[test]
fn scope_policy_is_open_reflects_empty_lists() {
    assert!(open_policy().is_open());
    assert!(!restricted_policy().is_open());
}
