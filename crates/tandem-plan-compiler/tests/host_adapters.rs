// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_plan_compiler::api::{
    build_workflow_plan_with_planner, revise_workflow_plan_draft, Clock, McpToolCatalog, PlanStore,
    PlannerBuildConfig, PlannerBuildRequest, PlannerInvocationFailure, PlannerLlmInvocation,
    PlannerLlmInvoker, PlannerLoopConfig, PlannerModelRegistry, PlannerSessionStore, TelemetrySink,
    WorkflowInputRefLike, WorkspaceResolver,
};
use tandem_types::ModelSpec;
use tandem_workflows::plan_package::{
    AutomationV2Schedule, AutomationV2ScheduleType, WorkflowPlan, WorkflowPlanConversation,
    WorkflowPlanDraftRecord, WorkflowPlanStep,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TestInputRef {
    from_step_id: String,
}

impl WorkflowInputRefLike for TestInputRef {
    fn from_step_id(&self) -> &str {
        self.from_step_id.as_str()
    }
}

struct TestHost {
    drafts: Mutex<HashMap<String, Value>>,
    now_ms: AtomicU64,
    provider_configured: bool,
    resolved_workspace_root: String,
    capability_summary: Value,
    llm_response: Mutex<Option<Value>>,
    invocations: Mutex<Vec<PlannerLlmInvocation>>,
    warnings: Mutex<Vec<String>>,
}

impl TestHost {
    fn new() -> Self {
        Self {
            drafts: Mutex::new(HashMap::new()),
            now_ms: AtomicU64::new(1),
            provider_configured: true,
            resolved_workspace_root: "/".to_string(),
            capability_summary: json!({}),
            llm_response: Mutex::new(None),
            invocations: Mutex::new(Vec::new()),
            warnings: Mutex::new(Vec::new()),
        }
    }

    fn with_llm_response(self, value: Value) -> Self {
        *self.llm_response.lock().unwrap() = Some(value);
        self
    }

    fn with_resolved_workspace_root(mut self, value: &str) -> Self {
        self.resolved_workspace_root = value.to_string();
        self
    }

    fn with_capability_summary(mut self, value: Value) -> Self {
        self.capability_summary = value;
        self
    }
}

#[async_trait]
impl WorkspaceResolver for TestHost {
    async fn resolve_workspace_root(&self, _requested: Option<&str>) -> Result<String, String> {
        Ok(self.resolved_workspace_root.clone())
    }
}

#[async_trait]
impl PlanStore for TestHost {
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
impl PlannerSessionStore for TestHost {
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

impl Clock for TestHost {
    fn now_ms(&self) -> u64 {
        self.now_ms.fetch_add(1, Ordering::SeqCst)
    }
}

#[async_trait]
impl PlannerModelRegistry for TestHost {
    async fn is_provider_configured(&self, _provider_id: &str) -> bool {
        self.provider_configured
    }
}

#[async_trait]
impl McpToolCatalog for TestHost {
    async fn capability_summary(&self, _allowed_mcp_servers: &[String]) -> Value {
        self.capability_summary.clone()
    }
}

#[async_trait]
impl PlannerLlmInvoker for TestHost {
    async fn invoke_planner_llm(
        &self,
        invocation: PlannerLlmInvocation,
    ) -> Result<Value, PlannerInvocationFailure> {
        self.invocations.lock().unwrap().push(invocation);
        self.llm_response
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| PlannerInvocationFailure {
                reason: "no_test_llm_response".to_string(),
                detail: None,
            })
    }
}

impl TelemetrySink for TestHost {
    fn warn(&self, message: &str) {
        self.warnings.lock().unwrap().push(message.to_string());
    }
}

fn test_model_preferences() -> Value {
    json!({
        "model_provider": "test_provider",
        "model_id": "test_model",
    })
}

fn test_model_spec() -> ModelSpec {
    ModelSpec {
        provider_id: "test_provider".to_string(),
        model_id: "test_model".to_string(),
    }
}

fn test_fallback_schedule() -> AutomationV2Schedule<Value> {
    AutomationV2Schedule {
        schedule_type: AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: json!({}),
    }
}

fn test_fallback_step() -> WorkflowPlanStep<TestInputRef, Value> {
    WorkflowPlanStep {
        step_id: "collect_inputs".to_string(),
        kind: "collect_inputs".to_string(),
        objective: "Collect inputs".to_string(),
        depends_on: Vec::new(),
        agent_role: "planner".to_string(),
        input_refs: Vec::new(),
        output_contract: None,
        metadata: None,
    }
}

fn test_plan(
    plan_id: &str,
) -> WorkflowPlan<AutomationV2Schedule<Value>, WorkflowPlanStep<TestInputRef, Value>> {
    WorkflowPlan {
        plan_id: plan_id.to_string(),
        planner_version: "test_planner_v1".to_string(),
        plan_source: "test".to_string(),
        original_prompt: "original".to_string(),
        normalized_prompt: "normalized".to_string(),
        confidence: "low".to_string(),
        title: "Test Plan".to_string(),
        description: None,
        schedule: test_fallback_schedule(),
        execution_target: "automation_v2".to_string(),
        workspace_root: "/".to_string(),
        steps: vec![test_fallback_step()],
        requires_integrations: Vec::new(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: Some(test_model_preferences()),
        save_options: json!({}),
    }
}

#[tokio::test]
async fn host_adapters_support_build_plan_clarify_flow() {
    let host = TestHost::new()
        .with_resolved_workspace_root("/repo")
        .with_capability_summary(json!({"mcp": []}))
        .with_llm_response(json!({
            "action": "clarify",
            "assistant_text": "Need one more detail.",
            "clarifier": {
                "field": "general",
                "question": "What is the primary goal?",
                "options": []
            }
        }));

    let request = PlannerBuildRequest {
        plan_id: "plan_1".to_string(),
        planner_version: "test_planner_v1".to_string(),
        plan_source: "test".to_string(),
        progress_id: Some("wizard-request-123".to_string()),
        prompt: "Build a workflow.".to_string(),
        normalized_prompt: "Build a workflow.".to_string(),
        title: "Example".to_string(),
        fallback_schedule: test_fallback_schedule(),
        explicit_schedule: None,
        requested_workspace_root: None,
        allowed_mcp_servers: vec!["github".to_string()],
        operator_preferences: Some(test_model_preferences()),
    };

    let result = build_workflow_plan_with_planner(
        &host,
        request,
        PlannerBuildConfig {
            session_title: "test".to_string(),
            timeout_ms: 10_000,
            override_env: "".to_string(),
        },
        |_| {},
        test_fallback_step(),
    )
    .await;

    assert_eq!(result.plan.plan_id, "plan_1");
    assert_eq!(result.plan.workspace_root, "/repo");
    assert_eq!(
        result.assistant_text.as_deref(),
        Some("Need one more detail.")
    );
    assert!(result.clarifier.is_object());
    assert_eq!(host.invocations.lock().unwrap().len(), 1);
    let invocation = &host.invocations.lock().unwrap()[0];
    assert_eq!(invocation.model.provider_id, test_model_spec().provider_id);
    assert_eq!(invocation.model.model_id, test_model_spec().model_id);
    assert_eq!(invocation.run_key, "workflow-plan-build:wizard-request-123");
}

#[tokio::test]
async fn host_adapters_support_draft_revision_keep_flow() {
    let plan_id = "plan_keep";
    let draft = WorkflowPlanDraftRecord {
        initial_plan: test_plan(plan_id),
        current_plan: test_plan(plan_id),
        plan_revision: 1,
        conversation: WorkflowPlanConversation {
            conversation_id: "conv1".to_string(),
            plan_id: plan_id.to_string(),
            created_at_ms: 1,
            updated_at_ms: 1,
            messages: Vec::new(),
        },
        planner_diagnostics: None,
        last_success_materialization: None,
        review: None,
    };

    let host = TestHost::new().with_llm_response(json!({
        "action": "keep",
        "assistant_text": "Keeping the current plan."
    }));

    host.drafts
        .lock()
        .unwrap()
        .insert(plan_id.to_string(), serde_json::to_value(draft).unwrap());

    let result = revise_workflow_plan_draft::<Value, TestInputRef, Value, _>(
        &host,
        plan_id,
        "Please keep it as-is.",
        PlannerLoopConfig {
            session_title: "test".to_string(),
            timeout_ms: 10_000,
            override_env: "".to_string(),
        },
        |_| {},
    )
    .await
    .unwrap();

    assert_eq!(result.draft.current_plan.plan_id, plan_id);
    assert_eq!(result.draft.conversation.messages.len(), 2);
    assert_eq!(result.draft.conversation.messages[0].role, "user");
    assert_eq!(result.draft.conversation.messages[1].role, "assistant");
    assert_eq!(result.assistant_text, "Keeping the current plan.");
    assert_eq!(host.invocations.lock().unwrap().len(), 1);
}
