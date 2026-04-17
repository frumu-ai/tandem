use chrono::Utc;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tandem_providers::{ChatMessage, ProviderRegistry, StreamChunk, TokenUsage};
use tandem_tools::{validate_tool_schemas, ToolRegistry};
use tandem_types::{
    ContextMode, EngineEvent, HostRuntimeContext, Message, MessagePart, MessagePartInput,
    MessageRole, ModelSpec, SendMessageRequest, SharedToolProgressSink, ToolMode, ToolSchema,
};
use tandem_wire::WireMessagePart;
use tokio_util::sync::CancellationToken;
use tracing::Level;

mod loop_guards;
mod loop_tuning;
mod prewrite_gate;
mod prewrite_mode;
mod prompt_context;
mod tool_execution;
mod prompt_execution;
mod prompt_runtime;
mod prompt_helpers;
mod tool_output;
mod tool_parsing;
mod types;

use loop_guards::{
    duplicate_signature_limit_for, tool_budget_for, websearch_duplicate_signature_limit,
};
use loop_tuning::{
    max_tool_iterations, permission_wait_timeout_ms, prompt_context_hook_timeout_ms,
    provider_stream_connect_timeout_ms, provider_stream_idle_timeout_ms,
    strict_write_retry_max_attempts, tool_exec_timeout_ms,
};
use prewrite_gate::{evaluate_prewrite_gate, PrewriteProgress};
use prewrite_mode::*;
use prompt_context::{
    format_context_mode, mcp_catalog_in_system_prompt_enabled, semantic_tool_retrieval_enabled,
    semantic_tool_retrieval_k, tandem_runtime_system_prompt,
};
use prompt_runtime::*;
use prompt_helpers::*;
use tool_output::*;
use tool_parsing::*;
use types::{EngineToolProgressSink, StreamedToolCall, WritePathRecoveryMode};

pub use prewrite_mode::prewrite_repair_retry_max_attempts;
pub use types::{
    PromptContextHook, PromptContextHookContext, SpawnAgentHook, SpawnAgentToolContext,
    SpawnAgentToolResult, ToolPolicyContext, ToolPolicyDecision, ToolPolicyHook,
};

use crate::tool_router::{
    classify_intent, default_mode_name, is_short_simple_prompt, select_tool_subset,
    should_escalate_auto_tools, tool_router_enabled, ToolIntent, ToolRoutingDecision,
};
use crate::{
    any_policy_matches, derive_session_title_from_prompt, title_needs_repair,
    tool_name_matches_policy, AgentDefinition, AgentRegistry, CancellationRegistry, EventBus,
    PermissionAction, PermissionManager, PluginRegistry, Storage,
};
use crate::{
    build_tool_effect_ledger_record, finalize_mutation_checkpoint_record,
    mutation_checkpoint_event, prepare_mutation_checkpoint, tool_effect_ledger_event,
    MutationCheckpointOutcome, ToolEffectLedgerPhase, ToolEffectLedgerStatus,
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct EngineLoop {
    storage: std::sync::Arc<Storage>,
    event_bus: EventBus,
    providers: ProviderRegistry,
    plugins: PluginRegistry,
    agents: AgentRegistry,
    permissions: PermissionManager,
    tools: ToolRegistry,
    cancellations: CancellationRegistry,
    host_runtime_context: HostRuntimeContext,
    workspace_overrides: std::sync::Arc<RwLock<HashMap<String, u64>>>,
    session_allowed_tools: std::sync::Arc<RwLock<HashMap<String, Vec<String>>>>,
    session_auto_approve_permissions: std::sync::Arc<RwLock<HashMap<String, bool>>>,
    spawn_agent_hook: std::sync::Arc<RwLock<Option<std::sync::Arc<dyn SpawnAgentHook>>>>,
    tool_policy_hook: std::sync::Arc<RwLock<Option<std::sync::Arc<dyn ToolPolicyHook>>>>,
    prompt_context_hook: std::sync::Arc<RwLock<Option<std::sync::Arc<dyn PromptContextHook>>>>,
}

impl EngineLoop {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: std::sync::Arc<Storage>,
        event_bus: EventBus,
        providers: ProviderRegistry,
        plugins: PluginRegistry,
        agents: AgentRegistry,
        permissions: PermissionManager,
        tools: ToolRegistry,
        cancellations: CancellationRegistry,
        host_runtime_context: HostRuntimeContext,
    ) -> Self {
        Self {
            storage,
            event_bus,
            providers,
            plugins,
            agents,
            permissions,
            tools,
            cancellations,
            host_runtime_context,
            workspace_overrides: std::sync::Arc::new(RwLock::new(HashMap::new())),
            session_allowed_tools: std::sync::Arc::new(RwLock::new(HashMap::new())),
            session_auto_approve_permissions: std::sync::Arc::new(RwLock::new(HashMap::new())),
            spawn_agent_hook: std::sync::Arc::new(RwLock::new(None)),
            tool_policy_hook: std::sync::Arc::new(RwLock::new(None)),
            prompt_context_hook: std::sync::Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_spawn_agent_hook(&self, hook: std::sync::Arc<dyn SpawnAgentHook>) {
        *self.spawn_agent_hook.write().await = Some(hook);
    }

    pub async fn set_tool_policy_hook(&self, hook: std::sync::Arc<dyn ToolPolicyHook>) {
        *self.tool_policy_hook.write().await = Some(hook);
    }

    pub async fn set_prompt_context_hook(&self, hook: std::sync::Arc<dyn PromptContextHook>) {
        *self.prompt_context_hook.write().await = Some(hook);
    }

    pub async fn set_session_allowed_tools(&self, session_id: &str, allowed_tools: Vec<String>) {
        let normalized = allowed_tools
            .into_iter()
            .map(|tool| normalize_tool_name(&tool))
            .filter(|tool| !tool.trim().is_empty())
            .collect::<Vec<_>>();
        self.session_allowed_tools
            .write()
            .await
            .insert(session_id.to_string(), normalized);
    }

    pub async fn clear_session_allowed_tools(&self, session_id: &str) {
        self.session_allowed_tools.write().await.remove(session_id);
    }

    pub async fn set_session_auto_approve_permissions(&self, session_id: &str, enabled: bool) {
        if enabled {
            self.session_auto_approve_permissions
                .write()
                .await
                .insert(session_id.to_string(), true);
        } else {
            self.session_auto_approve_permissions
                .write()
                .await
                .remove(session_id);
        }
    }

    pub async fn clear_session_auto_approve_permissions(&self, session_id: &str) {
        self.session_auto_approve_permissions
            .write()
            .await
            .remove(session_id);
    }

    pub async fn grant_workspace_override_for_session(
        &self,
        session_id: &str,
        ttl_seconds: u64,
    ) -> u64 {
        // Cap the override TTL to prevent indefinite sandbox bypass.
        const MAX_WORKSPACE_OVERRIDE_TTL_SECONDS: u64 = 600; // 10 minutes
        let capped_ttl = ttl_seconds.min(MAX_WORKSPACE_OVERRIDE_TTL_SECONDS);
        if capped_ttl < ttl_seconds {
            tracing::warn!(
                session_id = %session_id,
                requested_ttl_s = %ttl_seconds,
                capped_ttl_s = %capped_ttl,
                "workspace override TTL capped to maximum allowed value"
            );
        }
        let expires_at = chrono::Utc::now()
            .timestamp_millis()
            .max(0)
            .saturating_add((capped_ttl as i64).saturating_mul(1000))
            as u64;
        self.workspace_overrides
            .write()
            .await
            .insert(session_id.to_string(), expires_at);
        self.event_bus.publish(EngineEvent::new(
            "workspace.override.activated",
            json!({
                "sessionID": session_id,
                "requestedTtlSeconds": ttl_seconds,
                "cappedTtlSeconds": capped_ttl,
                "expiresAt": expires_at,
            }),
        ));
        expires_at
    }

    pub async fn run_prompt_async(
        &self,
        session_id: String,
        req: SendMessageRequest,
    ) -> anyhow::Result<()> {
        self.run_prompt_async_with_context(session_id, req, None)
            .await
    }


    pub async fn run_oneshot(&self, prompt: String) -> anyhow::Result<String> {
        self.providers.default_complete(&prompt).await
    }

    pub async fn run_oneshot_for_provider(
        &self,
        prompt: String,
        provider_id: Option<&str>,
    ) -> anyhow::Result<String> {
        self.providers
            .complete_for_provider(provider_id, &prompt, None)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_with_permission(
        &self,
        session_id: &str,
        message_id: &str,
        tool: String,
        args: Value,
        initial_tool_call_id: Option<String>,
        equipped_skills: Option<&[String]>,
        latest_user_text: &str,
        write_required: bool,
        latest_assistant_context: Option<&str>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Option<String>> {
        let tool = normalize_tool_name(&tool);
        let raw_args = args.clone();
        let publish_tool_effect = |tool_call_id: Option<&str>,
                                   phase: ToolEffectLedgerPhase,
                                   status: ToolEffectLedgerStatus,
                                   args: &Value,
                                   metadata: Option<&Value>,
                                   output: Option<&str>,
                                   error: Option<&str>| {
            self.event_bus
                .publish(tool_effect_ledger_event(build_tool_effect_ledger_record(
                    session_id,
                    message_id,
                    tool_call_id,
                    &tool,
                    phase,
                    status,
                    args,
                    metadata,
                    output,
                    error,
                )));
        };
        let normalized = normalize_tool_args_with_mode(
            &tool,
            args,
            latest_user_text,
            latest_assistant_context.unwrap_or_default(),
            if write_required {
                WritePathRecoveryMode::OutputTargetOnly
            } else {
                WritePathRecoveryMode::Heuristic
            },
        );
        let raw_args_preview = truncate_text(&raw_args.to_string(), 2_000);
        let normalized_args_preview = truncate_text(&normalized.args.to_string(), 2_000);
        self.event_bus.publish(EngineEvent::new(
            "tool.args.normalized",
            json!({
                "sessionID": session_id,
                "messageID": message_id,
                "tool": tool,
                "argsSource": normalized.args_source,
                "argsIntegrity": normalized.args_integrity,
                "rawArgsState": normalized.raw_args_state.as_str(),
                "rawArgsPreview": raw_args_preview,
                "normalizedArgsPreview": normalized_args_preview,
                "query": normalized.query,
                "queryHash": normalized.query.as_ref().map(|q| stable_hash(q)),
                "requestID": Value::Null
            }),
        ));
        if normalized.args_integrity == "recovered" {
            self.event_bus.publish(EngineEvent::new(
                "tool.args.recovered",
                json!({
                    "sessionID": session_id,
                    "messageID": message_id,
                    "tool": tool,
                    "argsSource": normalized.args_source,
                    "rawArgsPreview": raw_args_preview,
                    "normalizedArgsPreview": normalized_args_preview,
                    "query": normalized.query,
                    "queryHash": normalized.query.as_ref().map(|q| stable_hash(q)),
                    "requestID": Value::Null
                }),
            ));
        }
        if normalized.missing_terminal {
            let missing_reason = normalized
                .missing_terminal_reason
                .clone()
                .unwrap_or_else(|| "TOOL_ARGUMENTS_MISSING".to_string());
            let latest_user_preview = truncate_text(latest_user_text, 500);
            let latest_assistant_preview =
                truncate_text(latest_assistant_context.unwrap_or_default(), 500);
            self.event_bus.publish(EngineEvent::new(
                "tool.args.missing_terminal",
                json!({
                    "sessionID": session_id,
                    "messageID": message_id,
                    "tool": tool,
                    "argsSource": normalized.args_source,
                    "argsIntegrity": normalized.args_integrity,
                    "rawArgsState": normalized.raw_args_state.as_str(),
                    "requestID": Value::Null,
                    "error": missing_reason,
                    "rawArgsPreview": raw_args_preview,
                    "normalizedArgsPreview": normalized_args_preview,
                    "latestUserPreview": latest_user_preview,
                    "latestAssistantPreview": latest_assistant_preview,
                }),
            ));
            if tool == "write" {
                tracing::warn!(
                    session_id = %session_id,
                    message_id = %message_id,
                    tool = %tool,
                    reason = %missing_reason,
                    args_source = %normalized.args_source,
                    args_integrity = %normalized.args_integrity,
                    raw_args_state = %normalized.raw_args_state.as_str(),
                    raw_args = %raw_args_preview,
                    normalized_args = %normalized_args_preview,
                    latest_user = %latest_user_preview,
                    latest_assistant = %latest_assistant_preview,
                    "write tool arguments missing terminal field"
                );
            }
            let best_effort_args = persisted_failed_tool_args(&raw_args, &normalized.args);
            let mut failed_part = WireMessagePart::tool_result(
                session_id,
                message_id,
                tool.clone(),
                Some(best_effort_args),
                json!(null),
            );
            failed_part.state = Some("failed".to_string());
            let surfaced_reason =
                provider_specific_write_reason(&tool, &missing_reason, normalized.raw_args_state)
                    .unwrap_or_else(|| missing_reason.clone());
            failed_part.error = Some(surfaced_reason.clone());
            self.event_bus.publish(EngineEvent::new(
                "message.part.updated",
                json!({"part": failed_part}),
            ));
            publish_tool_effect(
                None,
                ToolEffectLedgerPhase::Outcome,
                ToolEffectLedgerStatus::Blocked,
                &normalized.args,
                None,
                None,
                Some(&surfaced_reason),
            );
            return Ok(Some(surfaced_reason));
        }

        let args = match enforce_skill_scope(&tool, normalized.args, equipped_skills) {
            Ok(args) => args,
            Err(message) => {
                publish_tool_effect(
                    None,
                    ToolEffectLedgerPhase::Outcome,
                    ToolEffectLedgerStatus::Blocked,
                    &raw_args,
                    None,
                    None,
                    Some(&message),
                );
                return Ok(Some(message));
            }
        };
        if let Some(allowed_tools) = self
            .session_allowed_tools
            .read()
            .await
            .get(session_id)
            .cloned()
        {
            if !allowed_tools.is_empty() && !any_policy_matches(&allowed_tools, &tool) {
                let reason = format!("Tool `{tool}` is not allowed for this run.");
                publish_tool_effect(
                    None,
                    ToolEffectLedgerPhase::Outcome,
                    ToolEffectLedgerStatus::Blocked,
                    &args,
                    None,
                    None,
                    Some(&reason),
                );
                return Ok(Some(reason));
            }
        }
        if let Some(hook) = self.tool_policy_hook.read().await.clone() {
            let decision = hook
                .evaluate_tool(ToolPolicyContext {
                    session_id: session_id.to_string(),
                    message_id: message_id.to_string(),
                    tool: tool.clone(),
                    args: args.clone(),
                })
                .await?;
            if !decision.allowed {
                let reason = decision
                    .reason
                    .unwrap_or_else(|| "Tool denied by runtime policy".to_string());
                let mut blocked_part = WireMessagePart::tool_result(
                    session_id,
                    message_id,
                    tool.clone(),
                    Some(args.clone()),
                    json!(null),
                );
                blocked_part.state = Some("failed".to_string());
                blocked_part.error = Some(reason.clone());
                self.event_bus.publish(EngineEvent::new(
                    "message.part.updated",
                    json!({"part": blocked_part}),
                ));
                publish_tool_effect(
                    None,
                    ToolEffectLedgerPhase::Outcome,
                    ToolEffectLedgerStatus::Blocked,
                    &args,
                    None,
                    None,
                    Some(&reason),
                );
                return Ok(Some(reason));
            }
        }
        let mut tool_call_id: Option<String> = initial_tool_call_id;
        if let Some(violation) = self
            .workspace_sandbox_violation(session_id, &tool, &args)
            .await
        {
            let mut blocked_part = WireMessagePart::tool_result(
                session_id,
                message_id,
                tool.clone(),
                Some(args.clone()),
                json!(null),
            );
            blocked_part.state = Some("failed".to_string());
            blocked_part.error = Some(violation.clone());
            self.event_bus.publish(EngineEvent::new(
                "message.part.updated",
                json!({"part": blocked_part}),
            ));
            publish_tool_effect(
                tool_call_id.as_deref(),
                ToolEffectLedgerPhase::Outcome,
                ToolEffectLedgerStatus::Blocked,
                &args,
                None,
                None,
                Some(&violation),
            );
            return Ok(Some(violation));
        }
        let rule = self
            .plugins
            .permission_override(&tool)
            .await
            .unwrap_or(self.permissions.evaluate(&tool, &tool).await);
        if matches!(rule, PermissionAction::Deny) {
            let reason = format!("Permission denied for tool `{tool}` by policy.");
            publish_tool_effect(
                tool_call_id.as_deref(),
                ToolEffectLedgerPhase::Outcome,
                ToolEffectLedgerStatus::Blocked,
                &args,
                None,
                None,
                Some(&reason),
            );
            return Ok(Some(reason));
        }

        let mut effective_args = args.clone();
        if matches!(rule, PermissionAction::Ask) {
            let auto_approve_permissions = self
                .session_auto_approve_permissions
                .read()
                .await
                .get(session_id)
                .copied()
                .unwrap_or(false);
            if auto_approve_permissions {
                // Governance audit: if args were recovered via heuristics and the tool is
                // mutating, log a WARN so recovered writes are never silent in automation
                // mode. Does not block — operators must opt out via TANDEM_AUTO_APPROVE_RECOVERED_ARGS=false
                // if they want a hard block (reserved for strict automation policy).
                if normalized.args_integrity == "recovered" && is_workspace_write_tool(&tool) {
                    tracing::warn!(
                        session_id = %session_id,
                        message_id = %message_id,
                        tool = %tool,
                        args_source = %normalized.args_source,
                        "auto-approve granted for mutating tool with recovered args; verify intent"
                    );
                    self.event_bus.publish(EngineEvent::new(
                        "tool.args.recovered_write_auto_approved",
                        json!({
                            "sessionID": session_id,
                            "messageID": message_id,
                            "tool": tool,
                            "argsSource": normalized.args_source,
                            "argsIntegrity": normalized.args_integrity,
                        }),
                    ));
                }
                self.event_bus.publish(EngineEvent::new(
                    "permission.auto_approved",
                    json!({
                        "sessionID": session_id,
                        "messageID": message_id,
                        "tool": tool,
                    }),
                ));
                effective_args = args;
            } else {
                let pending = self
                    .permissions
                    .ask_for_session_with_context(
                        Some(session_id),
                        &tool,
                        args.clone(),
                        Some(crate::PermissionArgsContext {
                            args_source: normalized.args_source.clone(),
                            args_integrity: normalized.args_integrity.clone(),
                            query: normalized.query.clone(),
                        }),
                    )
                    .await;
                let mut pending_part = WireMessagePart::tool_invocation(
                    session_id,
                    message_id,
                    tool.clone(),
                    args.clone(),
                );
                pending_part.id = Some(pending.id.clone());
                tool_call_id = Some(pending.id.clone());
                pending_part.state = Some("pending".to_string());
                self.event_bus.publish(EngineEvent::new(
                    "message.part.updated",
                    json!({"part": pending_part}),
                ));
                let reply = self
                    .permissions
                    .wait_for_reply_with_timeout(
                        &pending.id,
                        cancel.clone(),
                        Some(Duration::from_millis(permission_wait_timeout_ms() as u64)),
                    )
                    .await;
                let (reply, timed_out) = reply;
                if cancel.is_cancelled() {
                    return Ok(None);
                }
                if timed_out {
                    let timeout_ms = permission_wait_timeout_ms();
                    self.event_bus.publish(EngineEvent::new(
                        "permission.wait.timeout",
                        json!({
                            "sessionID": session_id,
                            "messageID": message_id,
                            "tool": tool,
                            "requestID": pending.id,
                            "timeoutMs": timeout_ms,
                        }),
                    ));
                    let mut timeout_part = WireMessagePart::tool_result(
                        session_id,
                        message_id,
                        tool.clone(),
                        Some(args.clone()),
                        json!(null),
                    );
                    timeout_part.id = Some(pending.id);
                    timeout_part.state = Some("failed".to_string());
                    timeout_part.error = Some(format!(
                        "Permission request timed out after {} ms",
                        timeout_ms
                    ));
                    self.event_bus.publish(EngineEvent::new(
                        "message.part.updated",
                        json!({"part": timeout_part}),
                    ));
                    let timeout_reason = format!(
                        "Permission request for tool `{tool}` timed out after {timeout_ms} ms."
                    );
                    publish_tool_effect(
                        tool_call_id.as_deref(),
                        ToolEffectLedgerPhase::Outcome,
                        ToolEffectLedgerStatus::Blocked,
                        &args,
                        None,
                        None,
                        Some(&timeout_reason),
                    );
                    return Ok(Some(format!(
                        "Permission request for tool `{tool}` timed out after {timeout_ms} ms."
                    )));
                }
                let approved = matches!(reply.as_deref(), Some("once" | "always" | "allow"));
                if !approved {
                    let mut denied_part = WireMessagePart::tool_result(
                        session_id,
                        message_id,
                        tool.clone(),
                        Some(args.clone()),
                        json!(null),
                    );
                    denied_part.id = Some(pending.id);
                    denied_part.state = Some("denied".to_string());
                    denied_part.error = Some("Permission denied by user".to_string());
                    self.event_bus.publish(EngineEvent::new(
                        "message.part.updated",
                        json!({"part": denied_part}),
                    ));
                    let denied_reason = format!("Permission denied for tool `{tool}` by user.");
                    publish_tool_effect(
                        tool_call_id.as_deref(),
                        ToolEffectLedgerPhase::Outcome,
                        ToolEffectLedgerStatus::Blocked,
                        &args,
                        None,
                        None,
                        Some(&denied_reason),
                    );
                    return Ok(Some(format!(
                        "Permission denied for tool `{tool}` by user."
                    )));
                }
                effective_args = args;
            }
        }

        let mut args = self.plugins.inject_tool_args(&tool, effective_args).await;
        let session = self.storage.get_session(session_id).await;
        if let (Some(obj), Some(session)) = (args.as_object_mut(), session.as_ref()) {
            obj.insert(
                "__session_id".to_string(),
                Value::String(session_id.to_string()),
            );
            if let Some(project_id) = session.project_id.clone() {
                obj.insert(
                    "__project_id".to_string(),
                    Value::String(project_id.clone()),
                );
                if project_id.starts_with("channel-public::") {
                    obj.insert(
                        "__memory_max_visible_scope".to_string(),
                        Value::String("project".to_string()),
                    );
                }
            }
        }
        let tool_context = self.resolve_tool_execution_context(session_id).await;
        if let Some((workspace_root, effective_cwd, project_id)) = tool_context.as_ref() {
            args = rewrite_workspace_alias_tool_args(&tool, args, workspace_root);
            if let Some(obj) = args.as_object_mut() {
                obj.insert(
                    "__workspace_root".to_string(),
                    Value::String(workspace_root.clone()),
                );
                obj.insert(
                    "__effective_cwd".to_string(),
                    Value::String(effective_cwd.clone()),
                );
                obj.insert(
                    "__session_id".to_string(),
                    Value::String(session_id.to_string()),
                );
                if let Some(project_id) = project_id.clone() {
                    obj.insert("__project_id".to_string(), Value::String(project_id));
                }
            }
            tracing::info!(
                "tool execution context session_id={} tool={} workspace_root={} effective_cwd={} project_id={}",
                session_id,
                tool,
                workspace_root,
                effective_cwd,
                project_id.clone().unwrap_or_default()
            );
        }
        let mut invoke_part =
            WireMessagePart::tool_invocation(session_id, message_id, tool.clone(), args.clone());
        if let Some(call_id) = tool_call_id.clone() {
            invoke_part.id = Some(call_id);
        }
        let invoke_part_id = invoke_part.id.clone();
        self.event_bus.publish(EngineEvent::new(
            "message.part.updated",
            json!({"part": invoke_part}),
        ));
        let args_for_side_events = args.clone();
        let mutation_checkpoint = prepare_mutation_checkpoint(&tool, &args_for_side_events);
        let progress_sink: SharedToolProgressSink = std::sync::Arc::new(EngineToolProgressSink {
            event_bus: self.event_bus.clone(),
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            tool_call_id: invoke_part_id.clone(),
            source_tool: tool.clone(),
        });
        publish_tool_effect(
            invoke_part_id.as_deref(),
            ToolEffectLedgerPhase::Invocation,
            ToolEffectLedgerStatus::Started,
            &args_for_side_events,
            None,
            None,
            None,
        );
        let publish_mutation_checkpoint =
            |tool_call_id: Option<&str>, outcome: MutationCheckpointOutcome| {
                if let Some(baseline) = mutation_checkpoint.as_ref() {
                    self.event_bus.publish(mutation_checkpoint_event(
                        finalize_mutation_checkpoint_record(
                            session_id,
                            message_id,
                            tool_call_id,
                            baseline,
                            outcome,
                        ),
                    ));
                }
            };
        if tool == "spawn_agent" {
            let hook = self.spawn_agent_hook.read().await.clone();
            if let Some(hook) = hook {
                let spawned = hook
                    .spawn_agent(SpawnAgentToolContext {
                        session_id: session_id.to_string(),
                        message_id: message_id.to_string(),
                        tool_call_id: invoke_part_id.clone(),
                        args: args_for_side_events.clone(),
                    })
                    .await?;
                let output = self.plugins.transform_tool_output(spawned.output).await;
                let output = truncate_text(&output, 16_000);
                emit_tool_side_events(
                    self.storage.clone(),
                    &self.event_bus,
                    ToolSideEventContext {
                        session_id,
                        message_id,
                        tool: &tool,
                        args: &args_for_side_events,
                        metadata: &spawned.metadata,
                        workspace_root: tool_context.as_ref().map(|ctx| ctx.0.as_str()),
                        effective_cwd: tool_context.as_ref().map(|ctx| ctx.1.as_str()),
                    },
                )
                .await;
                let mut result_part = WireMessagePart::tool_result(
                    session_id,
                    message_id,
                    tool.clone(),
                    Some(args_for_side_events.clone()),
                    json!(output.clone()),
                );
                result_part.id = invoke_part_id.clone();
                self.event_bus.publish(EngineEvent::new(
                    "message.part.updated",
                    json!({"part": result_part}),
                ));
                publish_tool_effect(
                    invoke_part_id.as_deref(),
                    ToolEffectLedgerPhase::Outcome,
                    ToolEffectLedgerStatus::Succeeded,
                    &args_for_side_events,
                    Some(&spawned.metadata),
                    Some(&output),
                    None,
                );
                publish_mutation_checkpoint(
                    invoke_part_id.as_deref(),
                    MutationCheckpointOutcome::Succeeded,
                );
                return Ok(Some(truncate_text(
                    &format!("Tool `{tool}` result:\n{output}"),
                    16_000,
                )));
            }
            let output = "spawn_agent is unavailable in this runtime (no spawn hook installed).";
            let mut failed_part = WireMessagePart::tool_result(
                session_id,
                message_id,
                tool.clone(),
                Some(args_for_side_events.clone()),
                json!(null),
            );
            failed_part.id = invoke_part_id.clone();
            failed_part.state = Some("failed".to_string());
            failed_part.error = Some(output.to_string());
            self.event_bus.publish(EngineEvent::new(
                "message.part.updated",
                json!({"part": failed_part}),
            ));
            publish_tool_effect(
                invoke_part_id.as_deref(),
                ToolEffectLedgerPhase::Outcome,
                ToolEffectLedgerStatus::Failed,
                &args_for_side_events,
                None,
                None,
                Some(output),
            );
            publish_mutation_checkpoint(
                invoke_part_id.as_deref(),
                MutationCheckpointOutcome::Failed,
            );
            return Ok(Some(output.to_string()));
        }
        // Batch governance: validate sub-calls against engine policy and inject execution context
        // before delegating to BatchTool. This ensures sub-calls cannot bypass permissions,
        // sandbox checks, or allowed-tool lists, and that they receive the correct workspace
        // context (__workspace_root, __effective_cwd, __session_id, __project_id).
        //
        // By this point `args` already has those keys injected (see context injection above).
        if tool == "batch" {
            let allowed_tools = self
                .session_allowed_tools
                .read()
                .await
                .get(session_id)
                .cloned()
                .unwrap_or_default();

            // Extract parent execution context from already-injected batch args.
            let ctx_workspace_root = args
                .get("__workspace_root")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let ctx_effective_cwd = args
                .get("__effective_cwd")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let ctx_session_id = args
                .get("__session_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let ctx_project_id = args
                .get("__project_id")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);

            // Process each sub-call: check governance, inject context.
            let raw_calls = args
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut governed_calls: Vec<Value> = Vec::new();
            for mut call in raw_calls {
                let (sub_tool, mut sub_args) = {
                    let obj = match call.as_object() {
                        Some(o) => o,
                        None => {
                            governed_calls.push(call);
                            continue;
                        }
                    };
                    let tool_raw = non_empty_string_at(obj, "tool")
                        .or_else(|| nested_non_empty_string_at(obj, "function", "name"))
                        .or_else(|| nested_non_empty_string_at(obj, "tool", "name"))
                        .or_else(|| non_empty_string_at(obj, "name"));
                    let sub_tool = match tool_raw {
                        Some(t) => normalize_tool_name(t),
                        None => {
                            governed_calls.push(call);
                            continue;
                        }
                    };
                    let sub_args = obj.get("args").cloned().unwrap_or_else(|| json!({}));
                    (sub_tool, sub_args)
                };

                // 1. Allowed-tools check.
                if !allowed_tools.is_empty() && !any_policy_matches(&allowed_tools, &sub_tool) {
                    // Strip this sub-call: replace it with an explanatory result.
                    if let Some(obj) = call.as_object_mut() {
                        obj.insert(
                            "_blocked".to_string(),
                            Value::String(format!(
                                "batch sub-call skipped: tool `{sub_tool}` is not in the allowed list for this run"
                            )),
                        );
                    }
                    governed_calls.push(call);
                    continue;
                }

                // 2. Workspace sandbox check.
                if let Some(violation) = self
                    .workspace_sandbox_violation(session_id, &sub_tool, &sub_args)
                    .await
                {
                    if let Some(obj) = call.as_object_mut() {
                        obj.insert(
                            "_blocked".to_string(),
                            Value::String(format!("batch sub-call skipped: {violation}")),
                        );
                    }
                    governed_calls.push(call);
                    continue;
                }

                // 3. Inject parent execution context into sub-call args.
                if let Some(sub_obj) = sub_args.as_object_mut() {
                    if let Some(ref v) = ctx_workspace_root {
                        sub_obj
                            .entry("__workspace_root")
                            .or_insert_with(|| Value::String(v.clone()));
                    }
                    if let Some(ref v) = ctx_effective_cwd {
                        sub_obj
                            .entry("__effective_cwd")
                            .or_insert_with(|| Value::String(v.clone()));
                    }
                    if let Some(ref v) = ctx_session_id {
                        sub_obj
                            .entry("__session_id")
                            .or_insert_with(|| Value::String(v.clone()));
                    }
                    if let Some(ref v) = ctx_project_id {
                        sub_obj
                            .entry("__project_id")
                            .or_insert_with(|| Value::String(v.clone()));
                    }
                }

                // Write enriched args back into the call object.
                if let Some(obj) = call.as_object_mut() {
                    obj.insert("args".to_string(), sub_args);
                }
                governed_calls.push(call);
            }

            // Rebuild batch args with the governed sub-calls.
            if let Some(obj) = args.as_object_mut() {
                obj.insert("tool_calls".to_string(), Value::Array(governed_calls));
            }
        }
        let result = match self
            .execute_tool_with_timeout(&tool, args, cancel.clone(), Some(progress_sink))
            .await
        {
            Ok(result) => result,
            Err(err) => {
                let err_text = err.to_string();
                if err_text.contains("TOOL_EXEC_TIMEOUT_MS_EXCEEDED(") {
                    let timeout_ms = tool_exec_timeout_ms();
                    let timeout_output = format!(
                        "Tool `{tool}` timed out after {timeout_ms} ms. It was stopped to keep this run responsive."
                    );
                    let mut failed_part = WireMessagePart::tool_result(
                        session_id,
                        message_id,
                        tool.clone(),
                        Some(args_for_side_events.clone()),
                        json!(null),
                    );
                    failed_part.id = invoke_part_id.clone();
                    failed_part.state = Some("failed".to_string());
                    failed_part.error = Some(timeout_output.clone());
                    self.event_bus.publish(EngineEvent::new(
                        "message.part.updated",
                        json!({"part": failed_part}),
                    ));
                    publish_tool_effect(
                        invoke_part_id.as_deref(),
                        ToolEffectLedgerPhase::Outcome,
                        ToolEffectLedgerStatus::Failed,
                        &args_for_side_events,
                        None,
                        None,
                        Some(&timeout_output),
                    );
                    publish_mutation_checkpoint(
                        invoke_part_id.as_deref(),
                        MutationCheckpointOutcome::Failed,
                    );
                    return Ok(Some(timeout_output));
                }
                if let Some(auth) = extract_mcp_auth_required_from_error_text(&tool, &err_text) {
                    self.event_bus.publish(EngineEvent::new(
                        "mcp.auth.required",
                        json!({
                            "sessionID": session_id,
                            "messageID": message_id,
                            "tool": tool.clone(),
                            "server": auth.server,
                            "authorizationUrl": auth.authorization_url,
                            "message": auth.message,
                            "challengeId": auth.challenge_id
                        }),
                    ));
                    let auth_output = format!(
                        "Authorization required for `{}`.\n{}\n\nAuthorize here: {}",
                        tool, auth.message, auth.authorization_url
                    );
                    let mut result_part = WireMessagePart::tool_result(
                        session_id,
                        message_id,
                        tool.clone(),
                        Some(args_for_side_events.clone()),
                        json!(auth_output.clone()),
                    );
                    result_part.id = invoke_part_id.clone();
                    self.event_bus.publish(EngineEvent::new(
                        "message.part.updated",
                        json!({"part": result_part}),
                    ));
                    publish_tool_effect(
                        invoke_part_id.as_deref(),
                        ToolEffectLedgerPhase::Outcome,
                        ToolEffectLedgerStatus::Blocked,
                        &args_for_side_events,
                        None,
                        Some(&auth_output),
                        Some(&auth.message),
                    );
                    publish_mutation_checkpoint(
                        invoke_part_id.as_deref(),
                        MutationCheckpointOutcome::Blocked,
                    );
                    return Ok(Some(truncate_text(
                        &format!("Tool `{tool}` result:\n{auth_output}"),
                        16_000,
                    )));
                }
                let mut failed_part = WireMessagePart::tool_result(
                    session_id,
                    message_id,
                    tool.clone(),
                    Some(args_for_side_events.clone()),
                    json!(null),
                );
                failed_part.id = invoke_part_id.clone();
                failed_part.state = Some("failed".to_string());
                failed_part.error = Some(err_text.clone());
                self.event_bus.publish(EngineEvent::new(
                    "message.part.updated",
                    json!({"part": failed_part}),
                ));
                publish_tool_effect(
                    invoke_part_id.as_deref(),
                    ToolEffectLedgerPhase::Outcome,
                    ToolEffectLedgerStatus::Failed,
                    &args_for_side_events,
                    None,
                    None,
                    Some(&err_text),
                );
                publish_mutation_checkpoint(
                    invoke_part_id.as_deref(),
                    MutationCheckpointOutcome::Failed,
                );
                return Err(err);
            }
        };
        if let Some(auth) = extract_mcp_auth_required_metadata(&result.metadata) {
            let event_name = if auth.pending && auth.blocked {
                "mcp.auth.pending"
            } else {
                "mcp.auth.required"
            };
            self.event_bus.publish(EngineEvent::new(
                event_name,
                json!({
                    "sessionID": session_id,
                    "messageID": message_id,
                    "tool": tool.clone(),
                    "server": auth.server,
                    "authorizationUrl": auth.authorization_url,
                    "message": auth.message,
                    "challengeId": auth.challenge_id,
                    "pending": auth.pending,
                    "blocked": auth.blocked,
                    "retryAfterMs": auth.retry_after_ms
                }),
            ));
        }
        emit_tool_side_events(
            self.storage.clone(),
            &self.event_bus,
            ToolSideEventContext {
                session_id,
                message_id,
                tool: &tool,
                args: &args_for_side_events,
                metadata: &result.metadata,
                workspace_root: tool_context.as_ref().map(|ctx| ctx.0.as_str()),
                effective_cwd: tool_context.as_ref().map(|ctx| ctx.1.as_str()),
            },
        )
        .await;
        let output = if let Some(auth) = extract_mcp_auth_required_metadata(&result.metadata) {
            if auth.pending && auth.blocked {
                let retry_after_secs = auth.retry_after_ms.unwrap_or(0).div_ceil(1000);
                format!(
                    "Authorization pending for `{}`.\n{}\n\nAuthorize here: {}\nRetry after {}s.",
                    tool, auth.message, auth.authorization_url, retry_after_secs
                )
            } else {
                format!(
                    "Authorization required for `{}`.\n{}\n\nAuthorize here: {}",
                    tool, auth.message, auth.authorization_url
                )
            }
        } else {
            self.plugins.transform_tool_output(result.output).await
        };
        let output = truncate_text(&output, 16_000);
        let mut result_part = WireMessagePart::tool_result(
            session_id,
            message_id,
            tool.clone(),
            Some(args_for_side_events.clone()),
            json!(output.clone()),
        );
        result_part.id = invoke_part_id.clone();
        self.event_bus.publish(EngineEvent::new(
            "message.part.updated",
            json!({"part": result_part}),
        ));
        publish_tool_effect(
            invoke_part_id.as_deref(),
            ToolEffectLedgerPhase::Outcome,
            ToolEffectLedgerStatus::Succeeded,
            &args_for_side_events,
            Some(&result.metadata),
            Some(&output),
            None,
        );
        publish_mutation_checkpoint(
            invoke_part_id.as_deref(),
            MutationCheckpointOutcome::Succeeded,
        );
        Ok(Some(truncate_text(
            &format!("Tool `{tool}` result:\n{output}"),
            16_000,
        )))
    }

}

#[cfg(test)]
mod tests;
