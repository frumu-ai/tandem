// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_workflows::plan_package::{
    AutomationV2Schedule, WorkflowPlan, WorkflowPlanChatMessage, WorkflowPlanDraftRecord,
    WorkflowPlanStep,
};

use crate::host::{Clock, PlanStore, PlannerLoopHost};
use crate::planner_loop::{revise_workflow_plan_with_planner_loop, PlannerLoopConfig};
use crate::workflow_plan::WorkflowInputRefLike;

const SUCCESS_MEMORY_PREFIX: &str = "Previous successful materialization:";

#[derive(Debug)]
pub enum PlannerDraftError {
    NotFound,
    InvalidState(String),
    Store(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerDraftRevisionResult<M, I, O>
where
    I: Default,
    O: Default,
{
    pub draft:
        WorkflowPlanDraftRecord<WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>>,
    pub assistant_text: String,
    pub change_summary: Vec<String>,
    pub clarifier: Value,
    pub planner_diagnostics: Option<Value>,
}

pub async fn load_workflow_plan_draft<M, I, O, H>(
    host: &H,
    plan_id: &str,
) -> Result<
    WorkflowPlanDraftRecord<WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>>,
    PlannerDraftError,
>
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone + Default + WorkflowInputRefLike + serde::Serialize + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
    H: PlanStore,
{
    let Some(draft_value) = host
        .get_draft(plan_id)
        .await
        .map_err(PlannerDraftError::Store)?
    else {
        return Err(PlannerDraftError::NotFound);
    };

    serde_json::from_value(draft_value)
        .map_err(|error| PlannerDraftError::InvalidState(error.to_string()))
}

pub async fn store_preview_draft<M, I, O, H>(
    host: &H,
    plan: WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    planner_diagnostics: Option<Value>,
) -> Result<
    WorkflowPlanDraftRecord<WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>>,
    PlannerDraftError,
>
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone + Default + WorkflowInputRefLike + serde::Serialize + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
    H: PlanStore + Clock,
{
    let draft = crate::workflow_plan::workflow_plan_draft_record(
        plan.clone(),
        plan.plan_id.clone(),
        planner_diagnostics,
        format!("wfchat-{}", host.now_ms()),
        host.now_ms(),
    );
    let draft_value = serde_json::to_value(&draft)
        .map_err(|error| PlannerDraftError::InvalidState(error.to_string()))?;
    host.put_draft(&plan.plan_id, draft_value)
        .await
        .map_err(PlannerDraftError::Store)?;
    Ok(draft)
}

pub async fn store_chat_start_draft<M, I, O, H>(
    host: &H,
    plan: WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>,
    planner_diagnostics: Option<Value>,
    assistant_text: Option<String>,
) -> Result<
    WorkflowPlanDraftRecord<WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>>,
    PlannerDraftError,
>
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone + Default + WorkflowInputRefLike + serde::Serialize + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
    H: PlanStore + Clock,
{
    let preserved_draft = load_workflow_plan_draft::<M, I, O, H>(host, &plan.plan_id)
        .await
        .ok();
    let preserved_success_materialization = preserved_draft
        .as_ref()
        .and_then(|draft| draft.last_success_materialization.clone());
    let preserved_revision = preserved_draft.as_ref().map(|draft| draft.plan_revision);
    let mut draft = store_preview_draft(host, plan, planner_diagnostics).await?;
    if let Some(revision) = preserved_revision {
        draft.plan_revision = draft.plan_revision.max(revision);
        let draft_value = serde_json::to_value(&draft)
            .map_err(|error| PlannerDraftError::InvalidState(error.to_string()))?;
        host.put_draft(&draft.current_plan.plan_id, draft_value)
            .await
            .map_err(PlannerDraftError::Store)?;
    }
    if preserved_success_materialization.is_some() {
        draft.last_success_materialization = preserved_success_materialization;
        let draft_value = serde_json::to_value(&draft)
            .map_err(|error| PlannerDraftError::InvalidState(error.to_string()))?;
        host.put_draft(&draft.current_plan.plan_id, draft_value)
            .await
            .map_err(PlannerDraftError::Store)?;
    }
    if let Some(text) = assistant_text {
        draft.conversation.messages.push(WorkflowPlanChatMessage {
            role: "assistant".to_string(),
            text,
            created_at_ms: host.now_ms(),
        });
        draft.conversation.updated_at_ms = host.now_ms();
        let draft_value = serde_json::to_value(&draft)
            .map_err(|error| PlannerDraftError::InvalidState(error.to_string()))?;
        host.put_draft(&draft.current_plan.plan_id, draft_value)
            .await
            .map_err(PlannerDraftError::Store)?;
    }
    Ok(draft)
}

pub async fn revise_workflow_plan_draft<M, I, O, H>(
    host: &H,
    plan_id: &str,
    message: &str,
    config: PlannerLoopConfig,
    mut normalize_step: impl FnMut(&mut WorkflowPlanStep<I, O>),
) -> Result<PlannerDraftRevisionResult<M, I, O>, PlannerDraftError>
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone + Default + WorkflowInputRefLike + serde::Serialize + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
    H: PlannerLoopHost + PlanStore + Clock,
{
    let mut draft = load_workflow_plan_draft::<M, I, O, H>(host, plan_id).await?;

    if let Some(success_memory_message) =
        success_memory_message(draft.last_success_materialization.as_ref())
    {
        draft.conversation.messages.retain(|entry| {
            !(entry.role == "system" && entry.text.starts_with(SUCCESS_MEMORY_PREFIX))
        });
        draft.conversation.messages.push(WorkflowPlanChatMessage {
            role: "system".to_string(),
            text: success_memory_message,
            created_at_ms: host.now_ms(),
        });
    }

    let user_message = WorkflowPlanChatMessage {
        role: "user".to_string(),
        text: message.to_string(),
        created_at_ms: host.now_ms(),
    };
    draft.conversation.updated_at_ms = user_message.created_at_ms;
    draft.conversation.messages.push(user_message);

    let (revised_plan, assistant_text, change_summary, clarifier, planner_diagnostics) =
        revise_workflow_plan_with_planner_loop(
            host,
            &draft.current_plan,
            &draft.conversation,
            message,
            config,
            &mut normalize_step,
        )
        .await;

    draft.plan_revision = draft.plan_revision.saturating_add(1);
    draft.current_plan = revised_plan;
    draft.planner_diagnostics = planner_diagnostics.clone();
    draft.conversation.messages.push(WorkflowPlanChatMessage {
        role: "assistant".to_string(),
        text: assistant_text.clone(),
        created_at_ms: host.now_ms(),
    });
    draft.conversation.updated_at_ms = host.now_ms();

    let draft_value = serde_json::to_value(&draft)
        .map_err(|error| PlannerDraftError::InvalidState(error.to_string()))?;
    host.put_draft(plan_id, draft_value)
        .await
        .map_err(PlannerDraftError::Store)?;

    Ok(PlannerDraftRevisionResult {
        draft,
        assistant_text,
        change_summary,
        clarifier,
        planner_diagnostics,
    })
}

pub async fn reset_workflow_plan_draft<M, I, O, H>(
    host: &H,
    plan_id: &str,
) -> Result<
    WorkflowPlanDraftRecord<WorkflowPlan<AutomationV2Schedule<M>, WorkflowPlanStep<I, O>>>,
    PlannerDraftError,
>
where
    M: Clone + serde::Serialize + DeserializeOwned,
    I: Clone + Default + WorkflowInputRefLike + serde::Serialize + DeserializeOwned,
    O: Clone + Default + serde::Serialize + DeserializeOwned,
    H: PlanStore + Clock,
{
    let mut draft = load_workflow_plan_draft::<M, I, O, H>(host, plan_id).await?;
    draft.current_plan = draft.initial_plan.clone();
    draft.conversation.messages.push(WorkflowPlanChatMessage {
        role: "system".to_string(),
        text: "Plan reset to the initial preview.".to_string(),
        created_at_ms: host.now_ms(),
    });
    draft.conversation.updated_at_ms = host.now_ms();

    let draft_value = serde_json::to_value(&draft)
        .map_err(|error| PlannerDraftError::InvalidState(error.to_string()))?;
    host.put_draft(plan_id, draft_value)
        .await
        .map_err(PlannerDraftError::Store)?;
    Ok(draft)
}

pub fn draft_not_found_response(plan_id: &str) -> Value {
    json!({
        "error": "workflow plan not found",
        "code": "WORKFLOW_PLAN_NOT_FOUND",
        "plan_id": plan_id,
    })
}

fn success_memory_message(success_memory: Option<&Value>) -> Option<String> {
    let success_memory = success_memory?;
    let plan_id = success_memory.get("plan_id")?.as_str()?;
    let routine_count = success_memory.get("routine_count")?.as_u64()?;
    let step_count = success_memory.get("step_count")?.as_u64()?;
    let context_object_count = success_memory.get("context_object_count")?.as_u64()?;
    Some(format!(
        "{SUCCESS_MEMORY_PREFIX} plan_id={plan_id}, routine_count={routine_count}, step_count={step_count}, context_object_count={context_object_count}."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::host::{
        Clock, McpToolCatalog, PlanStore, PlannerLlmInvocation, PlannerLlmInvoker,
        PlannerModelRegistry, TelemetrySink,
    };
    use crate::planner_types::PlannerInvocationFailure;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
    struct TestInputRef {
        from_step_id: String,
        alias: String,
    }

    impl WorkflowInputRefLike for TestInputRef {
        fn from_step_id(&self) -> &str {
            self.from_step_id.as_str()
        }
    }

    #[derive(Clone, Default)]
    struct MockHost {
        drafts: Arc<Mutex<std::collections::HashMap<String, Value>>>,
        llm_payload: Arc<Mutex<Option<Value>>>,
        now_ms: u64,
    }

    #[async_trait::async_trait]
    impl PlanStore for MockHost {
        async fn get_draft(&self, plan_id: &str) -> Result<Option<Value>, String> {
            Ok(self
                .drafts
                .lock()
                .expect("draft lock")
                .get(plan_id)
                .cloned())
        }

        async fn put_draft(&self, plan_id: &str, draft: Value) -> Result<(), String> {
            self.drafts
                .lock()
                .expect("draft lock")
                .insert(plan_id.to_string(), draft);
            Ok(())
        }
    }

    impl Clock for MockHost {
        fn now_ms(&self) -> u64 {
            self.now_ms
        }
    }

    #[async_trait::async_trait]
    impl PlannerModelRegistry for MockHost {
        async fn is_provider_configured(&self, _provider_id: &str) -> bool {
            true
        }
    }

    #[async_trait::async_trait]
    impl McpToolCatalog for MockHost {
        async fn capability_summary(&self, _allowed_mcp_servers: &[String]) -> Value {
            json!({})
        }
    }

    #[async_trait::async_trait]
    impl PlannerLlmInvoker for MockHost {
        async fn invoke_planner_llm(
            &self,
            _invocation: PlannerLlmInvocation,
        ) -> Result<Value, PlannerInvocationFailure> {
            self.llm_payload
                .lock()
                .expect("payload lock")
                .clone()
                .ok_or(PlannerInvocationFailure {
                    reason: "missing_payload".to_string(),
                    detail: None,
                })
        }
    }

    impl TelemetrySink for MockHost {}

    fn test_schedule() -> AutomationV2Schedule<Value> {
        AutomationV2Schedule {
            schedule_type: tandem_workflows::plan_package::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: Value::String("run_once".to_string()),
        }
    }

    fn test_plan(
    ) -> WorkflowPlan<AutomationV2Schedule<Value>, WorkflowPlanStep<TestInputRef, Value>> {
        WorkflowPlan {
            plan_id: "wfplan-test".to_string(),
            planner_version: "v1".to_string(),
            plan_source: "test".to_string(),
            original_prompt: "test prompt".to_string(),
            normalized_prompt: "test prompt".to_string(),
            confidence: "medium".to_string(),
            title: "Initial Title".to_string(),
            description: Some("Initial Description".to_string()),
            schedule: test_schedule(),
            execution_target: "automation_v2".to_string(),
            workspace_root: "/tmp/project".to_string(),
            steps: vec![WorkflowPlanStep {
                step_id: "execute_goal".to_string(),
                kind: "execute".to_string(),
                objective: "Do the thing".to_string(),
                depends_on: Vec::new(),
                agent_role: "worker".to_string(),
                input_refs: Vec::new(),
                output_contract: Some(json!({
                    "kind": "structured_json",
                    "validator": "structured_json"
                })),
                metadata: None,
            }],
            requires_integrations: Vec::new(),
            allowed_mcp_servers: Vec::new(),
            operator_preferences: Some(json!({
                "model_provider": "test-provider",
                "model_id": "test-model"
            })),
            save_options: json!({}),
        }
    }

    #[tokio::test]
    async fn stores_preview_draft_via_plan_store() {
        let host = MockHost {
            now_ms: 1000,
            ..Default::default()
        };
        let draft = store_preview_draft(&host, test_plan(), Some(json!({"reason": "ok"})))
            .await
            .expect("preview draft");

        assert_eq!(draft.current_plan.title, "Initial Title");
        assert_eq!(draft.conversation.plan_id, "wfplan-test");
        assert!(draft.conversation.messages.is_empty());

        let persisted = host
            .drafts
            .lock()
            .expect("draft lock")
            .get("wfplan-test")
            .cloned()
            .expect("persisted draft");
        let persisted: WorkflowPlanDraftRecord<
            WorkflowPlan<AutomationV2Schedule<Value>, WorkflowPlanStep<TestInputRef, Value>>,
        > = serde_json::from_value(persisted).expect("decode draft");
        assert_eq!(persisted.current_plan.title, "Initial Title");
        assert_eq!(persisted.plan_revision, 1);
    }

    #[tokio::test]
    async fn stores_chat_start_draft_with_assistant_message() {
        let host = MockHost {
            now_ms: 2000,
            ..Default::default()
        };
        let draft = store_chat_start_draft(
            &host,
            test_plan(),
            Some(json!({"reason": "ok"})),
            Some("Hello from planner".to_string()),
        )
        .await
        .expect("chat start draft");

        assert_eq!(draft.conversation.messages.len(), 1);
        assert_eq!(draft.conversation.messages[0].role, "assistant");
        assert_eq!(draft.conversation.messages[0].text, "Hello from planner");
    }

    #[tokio::test]
    async fn stores_chat_start_draft_preserves_success_memory_hint() {
        let host = MockHost {
            now_ms: 2100,
            ..Default::default()
        };

        let mut seeded = store_preview_draft(&host, test_plan(), None)
            .await
            .expect("seed draft");
        seeded.last_success_materialization = Some(json!({
            "plan_id": "wfplan-test",
            "routine_count": 2,
            "step_count": 3,
            "context_object_count": 1
        }));
        host.put_draft("wfplan-test", serde_json::to_value(&seeded).unwrap())
            .await
            .expect("persist seeded draft");

        let draft = store_chat_start_draft(
            &host,
            test_plan(),
            Some(json!({"reason": "ok"})),
            Some("Hello from planner".to_string()),
        )
        .await
        .expect("chat start draft");

        assert!(draft.last_success_materialization.is_some());
        assert_eq!(
            draft
                .last_success_materialization
                .as_ref()
                .and_then(|value| value.get("plan_id"))
                .and_then(Value::as_str),
            Some("wfplan-test")
        );
    }

    #[tokio::test]
    async fn stores_chat_start_draft_preserves_existing_revision() {
        let host = MockHost {
            now_ms: 2150,
            ..Default::default()
        };

        let mut seeded = store_preview_draft(&host, test_plan(), None)
            .await
            .expect("seed draft");
        seeded.plan_revision = 3;
        host.put_draft("wfplan-test", serde_json::to_value(&seeded).unwrap())
            .await
            .expect("persist seeded draft");

        let draft = store_chat_start_draft(&host, test_plan(), Some(json!({"reason": "ok"})), None)
            .await
            .expect("chat start draft");

        assert_eq!(draft.plan_revision, 3);
        let persisted = host
            .drafts
            .lock()
            .expect("draft lock")
            .get("wfplan-test")
            .cloned()
            .expect("persisted draft");
        let persisted: WorkflowPlanDraftRecord<
            WorkflowPlan<AutomationV2Schedule<Value>, WorkflowPlanStep<TestInputRef, Value>>,
        > = serde_json::from_value(persisted).expect("decode draft");
        assert_eq!(persisted.plan_revision, 3);
    }

    #[tokio::test]
    async fn revises_existing_draft_through_llm_and_persists_result() {
        let host = MockHost {
            now_ms: 3000,
            llm_payload: Arc::new(Mutex::new(Some(json!({
                "action": "revise",
                "assistant_text": "Updated the workflow plan.",
                "change_summary": ["retitled plan"],
                "plan": {
                    "plan_id": "wfplan-test",
                    "planner_version": "v1",
                    "plan_source": "test",
                    "original_prompt": "test prompt",
                    "normalized_prompt": "test prompt",
                    "confidence": "medium",
                    "title": "Updated Title",
                    "description": "Initial Description",
                    "schedule": {
                        "type": "manual",
                        "timezone": "UTC",
                        "misfire_policy": "run_once"
                    },
                    "execution_target": "automation_v2",
                    "workspace_root": "/tmp/project",
                    "steps": [{
                        "step_id": "execute_goal",
                        "kind": "execute",
                        "objective": "Do the thing",
                        "agent_role": "worker",
                        "depends_on": [],
                        "input_refs": [],
                        "output_contract": {
                            "kind": "structured_json",
                            "validator": "structured_json"
                        }
                    }],
                    "requires_integrations": [],
                    "allowed_mcp_servers": [],
                    "operator_preferences": {
                        "model_provider": "test-provider",
                        "model_id": "test-model"
                    },
                    "save_options": {}
                }
            })))),
            ..Default::default()
        };

        store_preview_draft(&host, test_plan(), None)
            .await
            .expect("seed draft");

        let revision = revise_workflow_plan_draft::<Value, TestInputRef, Value, _>(
            &host,
            "wfplan-test",
            "Please retitle it",
            PlannerLoopConfig {
                session_title: "test".to_string(),
                timeout_ms: 1000,
                override_env: "IGNORED".to_string(),
            },
            |_step: &mut WorkflowPlanStep<TestInputRef, Value>| {},
        )
        .await
        .expect("revision");

        assert_eq!(revision.draft.current_plan.title, "Updated Title");
        assert_eq!(revision.draft.plan_revision, 2);
        assert_eq!(revision.change_summary, vec!["retitled plan".to_string()]);
        assert_eq!(revision.draft.conversation.messages.len(), 2);
        assert_eq!(revision.draft.conversation.messages[0].role, "user");
        assert_eq!(revision.draft.conversation.messages[1].role, "assistant");
    }

    #[tokio::test]
    async fn revises_existing_draft_with_success_memory_hint() {
        let host = MockHost {
            now_ms: 4000,
            llm_payload: Arc::new(Mutex::new(Some(json!({
                "action": "keep",
                "assistant_text": "Keeping the current plan."
            })))),
            ..Default::default()
        };

        let mut draft = store_preview_draft(&host, test_plan(), None)
            .await
            .expect("seed draft");
        draft.last_success_materialization = Some(json!({
            "plan_id": "wfplan-test",
            "routine_count": 2,
            "step_count": 3,
            "context_object_count": 1
        }));
        host.put_draft("wfplan-test", serde_json::to_value(&draft).unwrap())
            .await
            .expect("persist seeded draft");

        let revision = revise_workflow_plan_draft::<Value, TestInputRef, Value, _>(
            &host,
            "wfplan-test",
            "Please keep it as-is.",
            PlannerLoopConfig {
                session_title: "test".to_string(),
                timeout_ms: 1000,
                override_env: "IGNORED".to_string(),
            },
            |_step: &mut WorkflowPlanStep<TestInputRef, Value>| {},
        )
        .await
        .expect("revision");

        assert_eq!(revision.draft.conversation.messages.len(), 3);
        assert_eq!(revision.draft.conversation.messages[0].role, "system");
        assert!(revision.draft.conversation.messages[0]
            .text
            .starts_with(SUCCESS_MEMORY_PREFIX));
        assert_eq!(revision.draft.conversation.messages[1].role, "user");
        assert_eq!(revision.draft.conversation.messages[2].role, "assistant");
        assert_eq!(revision.draft.plan_revision, 2);
    }
}
