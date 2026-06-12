// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Canonical runtime event schema (TAN-199).
//!
//! Every event published on the engine event bus carries a
//! [`RuntimeEventEnvelope`] (stamped centrally by the bus), and events whose
//! `event_type` belongs to the closed [`RuntimeEventType`] vocabulary can be
//! decoded into a typed [`RuntimeEvent`]. The full vocabulary, when each
//! event fires, and its payload fields are documented in
//! `docs/RUNTIME_EVENTS.md`.
//!
//! Schema policy:
//! - `schema_version` is bumped on any breaking envelope change.
//! - Payloads must stay free of prompt/tool content by default; carry
//!   references (message IDs, artifact refs) instead of content.
//! - New event types are added to the macro table below and to the doc in
//!   the same change; the vocabulary is otherwise closed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::TenantContext;

/// Version of the runtime event envelope contract.
pub const RUNTIME_EVENT_SCHEMA_VERSION: u32 = 1;

macro_rules! runtime_event_types {
    ($( $variant:ident => $name:literal, )+) => {
        /// Closed vocabulary of canonical runtime event types.
        ///
        /// `as_str()` returns the exact wire string emitted on the event bus;
        /// `parse()` is its inverse and returns `None` for event types outside
        /// the canonical vocabulary (e.g. externally ingested trigger events).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum RuntimeEventType {
            $( $variant, )+
        }

        impl RuntimeEventType {
            /// Every canonical event type, for exhaustive iteration in docs
            /// and tests.
            pub const ALL: &'static [RuntimeEventType] = &[ $( RuntimeEventType::$variant, )+ ];

            /// The exact wire string for this event type.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( RuntimeEventType::$variant => $name, )+
                }
            }

            /// Inverse of [`RuntimeEventType::as_str`].
            pub fn parse(value: &str) -> Option<Self> {
                match value {
                    $( $name => Some(RuntimeEventType::$variant), )+
                    _ => None,
                }
            }
        }
    };
}

runtime_event_types! {
    AgentTeamBudgetExhausted => "agent_team.budget.exhausted",
    AgentTeamBudgetUsage => "agent_team.budget.usage",
    AgentTeamCapabilityDenied => "agent_team.capability.denied",
    AgentTeamInstanceCancelled => "agent_team.instance.cancelled",
    AgentTeamInstanceCompleted => "agent_team.instance.completed",
    AgentTeamInstanceFailed => "agent_team.instance.failed",
    AgentTeamInstanceStarted => "agent_team.instance.started",
    AgentTeamMissionBudgetExhausted => "agent_team.mission.budget.exhausted",
    AgentTeamSpawnApproved => "agent_team.spawn.approved",
    AgentTeamSpawnDenied => "agent_team.spawn.denied",
    AgentTeamSpawnRequested => "agent_team.spawn.requested",
    ApprovalDecisionRecorded => "approval.decision.recorded",
    AuditExportDenied => "audit.export.denied",
    ApprovalGateToolGated => "approval.gate.tool.gated",
    AutomationReadOnlyWriteDenied => "automation.read_only_write.denied",
    AutomationUpdated => "automation.updated",
    AutomationV2RunCreated => "automation.v2.run.created",
    AutomationV2RunFailed => "automation_v2.run.failed",
    BugMonitorError => "bug_monitor.error",
    BugMonitorGithubCommentPosted => "bug_monitor.github.comment_posted",
    BugMonitorGithubIssueCreated => "bug_monitor.github.issue_created",
    BugMonitorIncidentDetected => "bug_monitor.incident.detected",
    BugMonitorIncidentDuplicateSuppressed => "bug_monitor.incident.duplicate_suppressed",
    BugMonitorIncidentTriageFailed => "bug_monitor.incident.triage_failed",
    BugMonitorIncidentTriageTimedOut => "bug_monitor.incident.triage_timed_out",
    BugMonitorTriageRunCreated => "bug_monitor.triage_run.created",
    CapabilitiesReadinessEvaluated => "capabilities.readiness.evaluated",
    ChannelCapabilityChanged => "channel.capability.changed",
    ChannelStatusChanged => "channel.status.changed",
    CoderApprovalRequired => "coder.approval.required",
    CoderArtifactAdded => "coder.artifact.added",
    CoderMemoryCandidateAdded => "coder.memory.candidate_added",
    CoderMemoryPromoted => "coder.memory.promoted",
    CoderMergeRecommended => "coder.merge.recommended",
    CoderMergeSubmitted => "coder.merge.submitted",
    CoderPrSubmitted => "coder.pr.submitted",
    CoderRunCreated => "coder.run.created",
    CoderRunPhaseChanged => "coder.run.phase_changed",
    ContextBudgetBypassed => "context.budget.bypassed",
    ContextBudgetFinal => "context.budget.final",
    ContextFullBudgetExceeded => "context.full.budget.exceeded",
    ContextFullBudgetWarning => "context.full.budget.warning",
    ContextModeFullSelected => "context.mode.full.selected",
    ContextPackBound => "context.pack.bound",
    ContextPackPolicyHook => "context.pack.policy_hook",
    ContextPackPublished => "context.pack.published",
    ContextPackRevoked => "context.pack.revoked",
    ContextPackSuperseded => "context.pack.superseded",
    ContextProfileSelected => "context.profile.selected",
    ContextRunFailed => "context.run.failed",
    ContextRunStream => "context.run.stream",
    ContextTaskBlocked => "context.task.blocked",
    ContextTaskCompleted => "context.task.completed",
    ContextTaskCreated => "context.task.created",
    ContextTaskFailed => "context.task.failed",
    EgressPreflightApprovalRequired => "egress.preflight.approval_required",
    EgressPreflightDenied => "egress.preflight.denied",
    EngineLifecycleReady => "engine.lifecycle.ready",
    EnterpriseConnectorCacheInvalidationRequired => "enterprise.connector.cache_invalidation_required",
    EnterpriseSourceBindingCacheInvalidationRequired => "enterprise.source_binding.cache_invalidation_required",
    FintechProtectedActionApproved => "fintech.protected_action.approved",
    FintechProtectedActionDenied => "fintech.protected_action.denied",
    GoalCapabilityLearningDiscovered => "goal_capability_learning.discovered",
    KbGroundingContextInjected => "kb.grounding.context.injected",
    KbGroundingRequired => "kb.grounding.required",
    KbGroundingStrictApplied => "kb.grounding.strict.applied",
    KbGroundingStrictDirectAnswer => "kb.grounding.strict.direct_answer",
    KbGroundingStrictError => "kb.grounding.strict.error",
    KnowledgePreflightInjected => "knowledge.preflight.injected",
    McpAuthPending => "mcp.auth.pending",
    McpAuthRequired => "mcp.auth.required",
    McpServerConnected => "mcp.server.connected",
    McpServerDeleted => "mcp.server.deleted",
    McpServerDisconnected => "mcp.server.disconnected",
    McpServerUpdated => "mcp.server.updated",
    McpToolsUpdated => "mcp.tools.updated",
    MemoryContextError => "memory.context.error",
    MemoryContextInjected => "memory.context.injected",
    MemoryDeleted => "memory.deleted",
    MemoryDocsContextInjected => "memory.docs.context.injected",
    MemoryPromote => "memory.promote",
    MemoryPut => "memory.put",
    MemorySearch => "memory.search",
    MemorySearchPerformed => "memory.search.performed",
    MemoryUpdated => "memory.updated",
    MessagePartUpdated => "message.part.updated",
    MissionCreated => "mission.created",
    MissionUpdated => "mission.updated",
    MutationCheckpointRecorded => "mutation.checkpoint.recorded",
    PackDetected => "pack.detected",
    PackInstallFailed => "pack.install.failed",
    PackInstallStarted => "pack.install.started",
    PackInstallSucceeded => "pack.install.succeeded",
    PackUpdateNotAvailable => "pack.update.not_available",
    PackBuilderApplyBlockedAuth => "pack_builder.apply.blocked_auth",
    PackBuilderApplyBlockedMissingSecrets => "pack_builder.apply.blocked_missing_secrets",
    PackBuilderApplyCancelled => "pack_builder.apply.cancelled",
    PackBuilderApplyCount => "pack_builder.apply.count",
    PackBuilderApplySuccess => "pack_builder.apply.success",
    PackBuilderApplyWrongPlanPrevented => "pack_builder.apply.wrong_plan_prevented",
    PackBuilderApplyBlocked => "pack_builder.apply_blocked",
    PackBuilderApplyCompleted => "pack_builder.apply_completed",
    PackBuilderApplyStarted => "pack_builder.apply_started",
    PackBuilderCancelled => "pack_builder.cancelled",
    PackBuilderError => "pack_builder.error",
    PackBuilderMetric => "pack_builder.metric",
    PackBuilderPreviewCount => "pack_builder.preview.count",
    PackBuilderPreviewReady => "pack_builder.preview_ready",
    PermissionAsked => "permission.asked",
    PermissionAutoApproved => "permission.auto_approved",
    PermissionReplied => "permission.replied",
    PermissionWaitTimeout => "permission.wait.timeout",
    PolicyDecisionRecorded => "policy.decision.recorded",
    PrewriteGateStrictModeBlocked => "prewrite.gate.strict_mode.blocked",
    PrewriteGateWaivedWriteExecuted => "prewrite.gate.waived.write_executed",
    ProviderCallIterationBudgetExhausted => "provider.call.iteration.budget_exhausted",
    ProviderCallIterationError => "provider.call.iteration.error",
    ProviderCallIterationFinish => "provider.call.iteration.finish",
    ProviderCallIterationRetry => "provider.call.iteration.retry",
    ProviderCallIterationStart => "provider.call.iteration.start",
    ProviderUsage => "provider.usage",
    QuestionAsked => "question.asked",
    QuestionReplied => "question.replied",
    RegistryUpdated => "registry.updated",
    ResourceDeleted => "resource.deleted",
    ResourceUpdated => "resource.updated",
    RoutineApprovalRequired => "routine.approval_required",
    RoutineBlocked => "routine.blocked",
    RoutineCreated => "routine.created",
    RoutineDeleted => "routine.deleted",
    RoutineFired => "routine.fired",
    RoutineRunApproved => "routine.run.approved",
    RoutineRunArtifactAdded => "routine.run.artifact_added",
    RoutineRunCompleted => "routine.run.completed",
    RoutineRunCreated => "routine.run.created",
    RoutineRunDenied => "routine.run.denied",
    RoutineRunFailed => "routine.run.failed",
    RoutineRunModelSelected => "routine.run.model_selected",
    RoutineRunPaused => "routine.run.paused",
    RoutineRunResumed => "routine.run.resumed",
    RoutineRunStarted => "routine.run.started",
    RoutineToolDenied => "routine.tool.denied",
    RoutineUpdated => "routine.updated",
    RunStreamConnected => "run.stream.connected",
    ServerConnected => "server.connected",
    SessionAttached => "session.attached",
    SessionCreated => "session.created",
    SessionDeleteDeferred => "session.delete.deferred",
    SessionError => "session.error",
    SessionRunConflict => "session.run.conflict",
    SessionRunFinished => "session.run.finished",
    SessionRunStarted => "session.run.started",
    SessionStatus => "session.status",
    SessionUpdated => "session.updated",
    SessionWorkspaceOverrideGranted => "session.workspace_override.granted",
    TodoUpdated => "todo.updated",
    ToolArgsMissingTerminal => "tool.args.missing_terminal",
    ToolArgsNormalized => "tool.args.normalized",
    ToolArgsRecovered => "tool.args.recovered",
    ToolArgsRecoveredWriteAutoApproved => "tool.args.recovered_write_auto_approved",
    ToolCallRejectedUnoffered => "tool.call.rejected_unoffered",
    ToolCallRejectedWritePolicy => "tool.call.rejected_write_policy",
    ToolEffectRecorded => "tool.effect.recorded",
    ToolExecutionDenied => "tool.execution.denied",
    ToolLoopGuardTriggered => "tool.loop_guard.triggered",
    ToolModeRequiredUnsatisfied => "tool.mode.required.unsatisfied",
    ToolRoutingDecision => "tool.routing.decision",
    WorkflowActionCompleted => "workflow.action.completed",
    WorkflowActionFailed => "workflow.action.failed",
    WorkflowActionStarted => "workflow.action.started",
    WorkflowRunAwaitingApproval => "workflow.run.awaiting_approval",
    WorkflowRunCompleted => "workflow.run.completed",
    WorkflowRunFailed => "workflow.run.failed",
    WorkflowGovernanceGateDecided => "workflow.governance.gate_decided",
    WorkflowRunStarted => "workflow.run.started",
    WorkflowLearningCandidateAutoApplied => "workflow_learning.candidate.auto_applied",
    WorkflowPlannerApprovalRequested => "workflow_planner.approval.requested",
    WorkflowPlannerCapabilityBlocked => "workflow_planner.capability.blocked",
    WorkflowPlannerDocsMcpUsed => "workflow_planner.docs_mcp.used",
    WorkflowPlannerDraftUpdated => "workflow_planner.draft.updated",
    WorkflowPlannerDraftValidated => "workflow_planner.draft.validated",
    WorkflowPlannerRequirementsMissing => "workflow_planner.requirements.missing",
    WorkflowPlannerReviewReady => "workflow_planner.review.ready",
    WorkflowPlannerSessionStarted => "workflow_planner.session.started",
    WorkspaceOverrideActivated => "workspace.override.activated",
    WorkspaceOverrideExpired => "workspace.override.expired",
}

impl std::fmt::Display for RuntimeEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for RuntimeEventType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RuntimeEventType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        RuntimeEventType::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown runtime event type `{value}`"))
        })
    }
}

/// Envelope metadata stamped onto every event published on the engine event
/// bus. Identity and ordering live here so consumers stop re-deriving them
/// from ad-hoc property keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEventEnvelope {
    /// Globally unique id for this event instance (UUID v4).
    pub event_id: String,
    /// Monotonic per-process sequence number assigned by the event bus.
    /// Detects gaps when the broadcast channel drops lagging subscribers.
    pub seq: u64,
    /// Version of the envelope contract; see [`RUNTIME_EVENT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Milliseconds since the Unix epoch at publish time.
    pub occurred_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_context: Option<TenantContext>,
}

impl RuntimeEventEnvelope {
    /// Build an envelope for an event, deriving the correlation ids from its
    /// properties (legacy emitters spell them several ways).
    pub fn derive(seq: u64, occurred_at_ms: u64, properties: &Value) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            seq,
            schema_version: RUNTIME_EVENT_SCHEMA_VERSION,
            occurred_at_ms,
            session_id: extract_session_id(properties),
            run_id: extract_run_id(properties),
            node_id: extract_node_id(properties),
            tenant_context: extract_tenant_context(properties),
        }
    }
}

/// Canonical runtime event: the envelope plus a typed event name and its
/// payload, serialized flat (`event_id`, `seq`, ... `event_type`, `payload`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEvent {
    #[serde(flatten)]
    pub envelope: RuntimeEventEnvelope,
    pub event_type: RuntimeEventType,
    #[serde(default)]
    pub payload: Value,
}

impl RuntimeEvent {
    /// Decode a bus event into the canonical form. Returns `None` when the
    /// event type is outside the closed vocabulary (such events still carry
    /// an envelope on the wire but have no typed representation).
    pub fn from_engine_event(event: &crate::EngineEvent) -> Option<Self> {
        let event_type = RuntimeEventType::parse(&event.event_type)?;
        let envelope = event
            .envelope
            .clone()
            .unwrap_or_else(|| RuntimeEventEnvelope::derive(0, 0, &event.properties));
        Some(Self {
            envelope,
            event_type,
            payload: event.properties.clone(),
        })
    }

    /// Convert back to the legacy bus shape, preserving the envelope.
    pub fn to_engine_event(&self) -> crate::EngineEvent {
        crate::EngineEvent {
            event_type: self.event_type.as_str().to_string(),
            properties: self.payload.clone(),
            envelope: Some(self.envelope.clone()),
        }
    }
}

fn extract_string(properties: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        properties
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

/// Extract the session id from a legacy properties bag, accepting the
/// historical key spellings.
pub fn extract_session_id(properties: &Value) -> Option<String> {
    extract_string(properties, &["sessionID", "sessionId", "session_id"])
}

/// Extract the run id from a legacy properties bag.
pub fn extract_run_id(properties: &Value) -> Option<String> {
    extract_string(properties, &["runID", "runId", "run_id"])
}

/// Extract the node id from a legacy properties bag.
pub fn extract_node_id(properties: &Value) -> Option<String> {
    extract_string(properties, &["nodeID", "nodeId", "node_id"])
}

/// Extract a serialized tenant context from a legacy properties bag.
pub fn extract_tenant_context(properties: &Value) -> Option<TenantContext> {
    ["tenantContext", "tenant_context"]
        .iter()
        .find_map(|key| properties.get(key))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vocabulary_round_trips_and_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for event_type in RuntimeEventType::ALL {
            let name = event_type.as_str();
            assert!(seen.insert(name), "duplicate wire string `{name}`");
            assert_eq!(
                RuntimeEventType::parse(name),
                Some(*event_type),
                "parse(as_str) must round-trip for `{name}`"
            );
        }
    }

    #[test]
    fn unknown_event_type_does_not_parse() {
        assert_eq!(RuntimeEventType::parse("definitely.not.a.real.event"), None);
        assert_eq!(RuntimeEventType::parse(""), None);
    }

    #[test]
    fn event_type_serde_uses_wire_strings() {
        let serialized =
            serde_json::to_value(RuntimeEventType::ToolArgsNormalized).expect("serialize");
        assert_eq!(serialized, json!("tool.args.normalized"));
        let parsed: RuntimeEventType =
            serde_json::from_value(json!("automation_v2.run.failed")).expect("deserialize");
        assert_eq!(parsed, RuntimeEventType::AutomationV2RunFailed);
        let error = serde_json::from_value::<RuntimeEventType>(json!("nope.nope"))
            .expect_err("unknown type rejected");
        assert!(error.to_string().contains("unknown runtime event type"));
    }

    #[test]
    fn runtime_event_serializes_flat_and_round_trips() {
        let event = RuntimeEvent {
            envelope: RuntimeEventEnvelope {
                event_id: "evt-1".to_string(),
                seq: 42,
                schema_version: RUNTIME_EVENT_SCHEMA_VERSION,
                occurred_at_ms: 1_700_000_000_000,
                session_id: Some("ses_1".to_string()),
                run_id: Some("run_1".to_string()),
                node_id: None,
                tenant_context: None,
            },
            event_type: RuntimeEventType::SessionRunStarted,
            payload: json!({"sessionID": "ses_1", "runID": "run_1"}),
        };

        let serialized = serde_json::to_value(&event).expect("serialize");
        assert_eq!(serialized["event_id"], "evt-1");
        assert_eq!(serialized["seq"], 42);
        assert_eq!(serialized["schema_version"], 1);
        assert_eq!(serialized["event_type"], "session.run.started");
        assert_eq!(serialized["session_id"], "ses_1");
        assert!(
            serialized.get("node_id").is_none(),
            "unset ids are omitted from the wire"
        );

        let round_tripped: RuntimeEvent = serde_json::from_value(serialized).expect("deserialize");
        assert_eq!(round_tripped.event_type, event.event_type);
        assert_eq!(round_tripped.envelope, event.envelope);
        assert_eq!(round_tripped.payload, event.payload);
    }

    #[test]
    fn correlation_ids_extract_from_all_historical_spellings() {
        for key in ["sessionID", "sessionId", "session_id"] {
            assert_eq!(
                extract_session_id(&json!({ key: "ses_1" })).as_deref(),
                Some("ses_1"),
                "key `{key}`"
            );
        }
        for key in ["runID", "runId", "run_id"] {
            assert_eq!(
                extract_run_id(&json!({ key: "run_1" })).as_deref(),
                Some("run_1")
            );
        }
        for key in ["nodeID", "nodeId", "node_id"] {
            assert_eq!(
                extract_node_id(&json!({ key: "node_1" })).as_deref(),
                Some("node_1")
            );
        }
        assert_eq!(extract_session_id(&json!({"sessionID": "  "})), None);
        assert_eq!(extract_session_id(&json!({})), None);
    }

    #[test]
    fn tenant_context_extracts_from_properties() {
        let properties = json!({
            "tenantContext": { "org_id": "org-1", "workspace_id": "ws-1" }
        });
        let tenant = extract_tenant_context(&properties).expect("tenant context");
        assert_eq!(tenant.org_id, "org-1");
        assert_eq!(tenant.workspace_id, "ws-1");
        assert_eq!(extract_tenant_context(&json!({})), None);
    }

    #[test]
    fn engine_event_conversion_round_trips() {
        let engine_event = crate::EngineEvent {
            event_type: "permission.asked".to_string(),
            properties: json!({"sessionID": "ses_1", "requestID": "req_1"}),
            envelope: Some(RuntimeEventEnvelope::derive(
                7,
                123,
                &json!({"sessionID": "ses_1"}),
            )),
        };

        let runtime_event =
            RuntimeEvent::from_engine_event(&engine_event).expect("canonical event");
        assert_eq!(runtime_event.event_type, RuntimeEventType::PermissionAsked);
        assert_eq!(runtime_event.envelope.seq, 7);
        assert_eq!(runtime_event.envelope.session_id.as_deref(), Some("ses_1"));

        let back = runtime_event.to_engine_event();
        assert_eq!(back.event_type, "permission.asked");
        assert_eq!(back.properties, engine_event.properties);
        assert_eq!(back.envelope, engine_event.envelope);
    }

    #[test]
    fn non_canonical_engine_event_has_no_typed_form() {
        let engine_event =
            crate::EngineEvent::new("external.webhook.ingested", json!({"source": "github"}));
        assert!(RuntimeEvent::from_engine_event(&engine_event).is_none());
    }

    #[test]
    fn envelope_derive_fills_identity_and_correlation() {
        let envelope = RuntimeEventEnvelope::derive(
            3,
            1_700_000_000_000,
            &json!({
                "session_id": "ses_1",
                "runId": "run_1",
                "node_id": "node_1",
                "tenantContext": { "org_id": "org-1", "workspace_id": "ws-1" }
            }),
        );
        assert!(!envelope.event_id.is_empty());
        assert_eq!(envelope.seq, 3);
        assert_eq!(envelope.schema_version, RUNTIME_EVENT_SCHEMA_VERSION);
        assert_eq!(envelope.session_id.as_deref(), Some("ses_1"));
        assert_eq!(envelope.run_id.as_deref(), Some("run_1"));
        assert_eq!(envelope.node_id.as_deref(), Some("node_1"));
        assert_eq!(
            envelope.tenant_context.as_ref().map(|t| t.org_id.as_str()),
            Some("org-1")
        );
    }
}
