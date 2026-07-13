use super::*;

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{bail, Context};
use tandem_automation::{
    LongRunningGoal, OrchestrationArtifactRef, OrchestrationSpec, OrchestrationStatus,
};
use tandem_types::{
    AccessPermission, ResourceKind, ToolCapabilities, ToolDomain, ToolEffect, ToolRiskTier,
    ToolSecurityDescriptor,
};

use crate::stateful_runtime::{
    GovernedTransitionRequest, GovernedTransitionResult, OrchestrationStateStore,
    OrchestrationTransitionAuthority, StartGoalOutcome, StatefulRuntimeStoragePaths,
    StatefulWaitQuery, ORCHESTRATION_DRAFT_VERSION,
};

#[derive(Clone, Copy)]
enum OrchestrationToolKind {
    CreateDraft,
    Validate,
    Publish,
    GoalStart,
    GoalGet,
    GoalCancel,
    HandoffEmit,
    HandoffApprove,
    WaitInspect,
    WaitResolve,
}

impl OrchestrationToolKind {
    fn name(self) -> &'static str {
        match self {
            Self::CreateDraft => "orchestration_create_draft",
            Self::Validate => "orchestration_validate",
            Self::Publish => "orchestration_publish",
            Self::GoalStart => "goal_start",
            Self::GoalGet => "goal_get",
            Self::GoalCancel => "goal_cancel",
            Self::HandoffEmit => "handoff_emit",
            Self::HandoffApprove => "handoff_approve",
            Self::WaitInspect => "wait_inspect",
            Self::WaitResolve => "wait_resolve",
        }
    }

    fn read_only(self) -> bool {
        matches!(self, Self::Validate | Self::GoalGet | Self::WaitInspect)
    }

    fn risk_tier(self) -> ToolRiskTier {
        if self.read_only() {
            ToolRiskTier::ReadDiscover
        } else if matches!(self, Self::CreateDraft) {
            ToolRiskTier::InternalWrite
        } else {
            ToolRiskTier::ConsequentialWrite
        }
    }
}

#[derive(Clone)]
pub(crate) struct OrchestrationTool {
    state: AppState,
    kind: OrchestrationToolKind,
}

pub(crate) fn orchestration_tools(state: AppState) -> Vec<Arc<dyn Tool>> {
    [
        OrchestrationToolKind::CreateDraft,
        OrchestrationToolKind::Validate,
        OrchestrationToolKind::Publish,
        OrchestrationToolKind::GoalStart,
        OrchestrationToolKind::GoalGet,
        OrchestrationToolKind::GoalCancel,
        OrchestrationToolKind::HandoffEmit,
        OrchestrationToolKind::HandoffApprove,
        OrchestrationToolKind::WaitInspect,
        OrchestrationToolKind::WaitResolve,
    ]
    .into_iter()
    .map(|kind| {
        Arc::new(OrchestrationTool {
            state: state.clone(),
            kind,
        }) as Arc<dyn Tool>
    })
    .collect()
}

#[async_trait]
impl Tool for OrchestrationTool {
    fn schema(&self) -> ToolSchema {
        let properties = match self.kind {
            OrchestrationToolKind::CreateDraft => json!({
                "orchestration_id": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "root_node_id": { "type": "string" },
                "nodes": { "type": "array", "items": { "type": "object" } },
                "edges": { "type": "array", "items": { "type": "object" } },
                "goal_policy": { "type": "object" },
                "metadata": { "type": "object" },
                "idempotency_key": { "type": "string" }
            }),
            OrchestrationToolKind::Validate => json!({
                "orchestration_id": { "type": "string" }
            }),
            OrchestrationToolKind::Publish => json!({
                "orchestration_id": { "type": "string" },
                "idempotency_key": { "type": "string" }
            }),
            OrchestrationToolKind::GoalStart => json!({
                "orchestration_id": { "type": "string" },
                "orchestration_version": { "type": "integer", "minimum": 1 },
                "objective": { "type": "string" },
                "metadata": { "type": "object" },
                "idempotency_key": { "type": "string" }
            }),
            OrchestrationToolKind::GoalGet => json!({
                "goal_id": { "type": "string" }
            }),
            OrchestrationToolKind::GoalCancel => json!({
                "goal_id": { "type": "string" },
                "reason": { "type": "string" },
                "idempotency_key": { "type": "string" }
            }),
            OrchestrationToolKind::HandoffEmit => json!({
                "goal_id": { "type": "string" },
                "transition_key": {
                    "type": "string",
                    "description": "A named transition from the published orchestration; never provide a target workflow ID"
                },
                "artifact": { "type": "object" },
                "idempotency_key": { "type": "string" }
            }),
            OrchestrationToolKind::HandoffApprove => json!({
                "goal_id": { "type": "string" },
                "handoff_id": { "type": "string" },
                "decision": { "type": "string", "enum": ["approve", "reject"] },
                "reason": { "type": "string" },
                "idempotency_key": { "type": "string" }
            }),
            OrchestrationToolKind::WaitInspect => json!({
                "goal_id": { "type": "string" },
                "wait_id": { "type": "string" }
            }),
            OrchestrationToolKind::WaitResolve => json!({
                "goal_id": { "type": "string" },
                "wait_id": { "type": "string" },
                "resolution": {},
                "idempotency_key": { "type": "string" }
            }),
        };
        let required = match self.kind {
            OrchestrationToolKind::CreateDraft => {
                vec!["name", "root_node_id", "nodes", "edges", "idempotency_key"]
            }
            OrchestrationToolKind::Validate => vec!["orchestration_id"],
            OrchestrationToolKind::Publish => vec!["orchestration_id", "idempotency_key"],
            OrchestrationToolKind::GoalStart => {
                vec!["orchestration_id", "objective", "idempotency_key"]
            }
            OrchestrationToolKind::GoalGet => vec!["goal_id"],
            OrchestrationToolKind::GoalCancel => vec!["goal_id", "idempotency_key"],
            OrchestrationToolKind::HandoffEmit => {
                vec!["goal_id", "transition_key", "artifact", "idempotency_key"]
            }
            OrchestrationToolKind::HandoffApprove => {
                vec!["goal_id", "handoff_id", "decision", "idempotency_key"]
            }
            OrchestrationToolKind::WaitInspect => vec!["goal_id", "wait_id"],
            OrchestrationToolKind::WaitResolve => {
                vec!["goal_id", "wait_id", "resolution", "idempotency_key"]
            }
        };
        let description = match self.kind {
            OrchestrationToolKind::HandoffEmit => {
                "Emit a governed named transition; Tandem resolves its target only from the published orchestration"
            }
            OrchestrationToolKind::HandoffApprove => {
                "Approve or reject a pending governed handoff with explicit approval authority"
            }
            OrchestrationToolKind::WaitResolve => {
                "Resolve an external-condition wait with explicit resolution authority"
            }
            _ => "Use Tandem's tenant-scoped long-running orchestration API",
        };
        let capabilities = ToolCapabilities::new()
            .effect(if self.kind.read_only() {
                ToolEffect::Read
            } else {
                ToolEffect::Write
            })
            .domain(ToolDomain::Planning)
            .requires_verification();
        let permission = if self.kind.read_only() {
            AccessPermission::Read
        } else {
            AccessPermission::Execute
        };
        let resource = match self.kind {
            OrchestrationToolKind::HandoffApprove => ResourceKind::Approval,
            OrchestrationToolKind::GoalStart
            | OrchestrationToolKind::GoalGet
            | OrchestrationToolKind::GoalCancel
            | OrchestrationToolKind::HandoffEmit
            | OrchestrationToolKind::WaitInspect
            | OrchestrationToolKind::WaitResolve => ResourceKind::Run,
            _ => ResourceKind::Automation,
        };
        ToolSchema::new(
            self.kind.name(),
            description,
            json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            }),
        )
        .with_capabilities(capabilities)
        .with_security(
            ToolSecurityDescriptor::new()
                .permission(permission)
                .resource_kind(resource)
                .risk_tier(self.kind.risk_tier()),
        )
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        bail!(
            "{} requires an authenticated tenant context",
            self.kind.name()
        )
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let output = self.execute_scoped(&args, &tenant).await?;
        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string()),
            metadata: output,
        })
    }
}

impl OrchestrationTool {
    async fn execute_scoped(&self, args: &Value, tenant: &TenantContext) -> anyhow::Result<Value> {
        let store = OrchestrationStateStore::from_automation_runs_path(
            &self.state.automation_v2_runs_path,
        )?;
        match self.kind {
            OrchestrationToolKind::CreateDraft => {
                let actor = mutation_actor(args, tenant, None)?;
                let idempotency_key = require_idempotency(args)?;
                let request_digest = crate::stateful_runtime::stable_definition_snapshot_hash(
                    &json!({ "tenant": tenant, "request": public_args(args), "actor": actor.id }),
                );
                if let Some(response) = store.begin_orchestration_tool_request(
                    tenant,
                    self.kind.name(),
                    idempotency_key,
                    &request_digest,
                    crate::now_ms(),
                )? {
                    return Ok(response);
                }
                let payload: super::orchestrations_api::OrchestrationDraftPayload =
                    serde_json::from_value(args.clone())?;
                if payload.expected_updated_at_ms.is_some() {
                    bail!("expected_updated_at_ms is only valid for draft updates");
                }
                let now = crate::now_ms();
                let orchestration_id = payload
                    .orchestration_id
                    .clone()
                    .map(|id| id.trim().to_string())
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| {
                        let identity =
                            crate::stateful_runtime::stable_definition_snapshot_hash(&json!({
                                "tenant": tenant,
                                "operation": self.kind.name(),
                                "idempotency_key": idempotency_key,
                            }));
                        format!(
                            "orch-{}",
                            identity
                                .trim_start_matches("sha256:")
                                .chars()
                                .take(32)
                                .collect::<String>()
                        )
                    });
                let mut metadata = payload.metadata.clone().unwrap_or_else(|| json!({}));
                merge_object_value(&mut metadata, "created_by", json!(actor));
                merge_object_value(
                    &mut metadata,
                    "create_idempotency_key",
                    json!(idempotency_key),
                );
                merge_object_value(
                    &mut metadata,
                    "create_request_digest",
                    json!(request_digest),
                );
                let spec = OrchestrationSpec {
                    schema_version: 1,
                    orchestration_id,
                    name: payload.name,
                    description: payload.description,
                    status: OrchestrationStatus::Draft,
                    version: ORCHESTRATION_DRAFT_VERSION,
                    root_node_id: payload.root_node_id,
                    nodes: payload.nodes,
                    edges: payload.edges,
                    goal_policy: payload.goal_policy.unwrap_or_default(),
                    tenant_context: super::orchestrations_api::resource_scope_tenant(tenant),
                    created_at_ms: now,
                    updated_at_ms: now,
                    published_at_ms: None,
                    metadata: Some(metadata),
                };
                let persisted_spec = if let Err(error) = store.put_orchestration_draft(&spec, None)
                {
                    let replay = store
                        .get_orchestration_draft(tenant, &spec.orchestration_id)?
                        .filter(|stored| {
                            metadata_str(stored, "create_idempotency_key") == Some(idempotency_key)
                                && metadata_str(stored, "create_request_digest")
                                    == Some(request_digest.as_str())
                        })
                        .ok_or(error)?;
                    replay
                } else {
                    spec
                };
                let response = json!({
                    "orchestration": persisted_spec,
                    "orchestration_id": persisted_spec.orchestration_id,
                    "version": persisted_spec.version,
                    "status": persisted_spec.status,
                    "updated_at_ms": persisted_spec.updated_at_ms,
                });
                store.complete_orchestration_tool_request(
                    tenant,
                    self.kind.name(),
                    idempotency_key,
                    &request_digest,
                    &response,
                    crate::now_ms(),
                )?;
                Ok(response)
            }
            OrchestrationToolKind::Validate => {
                let spec = scoped_draft(&store, tenant, required_str(args, "orchestration_id")?)?;
                let validation =
                    super::orchestrations_api::full_validation_report(&self.state, tenant, &spec)
                        .await;
                Ok(json!({
                    "orchestration_id": spec.orchestration_id,
                    "version": spec.version,
                    "report": validation.report,
                    "stale_references": validation.stale_references,
                }))
            }
            OrchestrationToolKind::Publish => {
                let actor = mutation_actor(args, tenant, None)?;
                let idempotency_key = require_idempotency(args)?;
                let mut draft =
                    scoped_draft(&store, tenant, required_str(args, "orchestration_id")?)?;
                let expected_draft_updated_at_ms = draft.updated_at_ms;
                authorize_orchestration_owner(args, tenant, &draft, &actor)?;
                if draft.status == OrchestrationStatus::Archived {
                    bail!("archived drafts cannot be published");
                }
                let request_digest = crate::stateful_runtime::stable_definition_snapshot_hash(
                    &json!({ "tenant": tenant, "request": public_args(args), "actor": actor.id }),
                );
                if let Some(response) = store.begin_orchestration_tool_request(
                    tenant,
                    self.kind.name(),
                    idempotency_key,
                    &request_digest,
                    crate::now_ms(),
                )? {
                    return Ok(response);
                }
                if let Some(published) = store
                    .list_orchestration_versions(tenant, &draft.orchestration_id)?
                    .into_iter()
                    .find(|published| {
                        publish_metadata_str(published, "idempotency_key") == Some(idempotency_key)
                            && publish_metadata_str(published, "request_digest")
                                == Some(request_digest.as_str())
                    })
                {
                    let response = json!({
                        "orchestration": published,
                        "orchestration_id": published.orchestration_id,
                        "version": published.version,
                        "status": published.status,
                        "updated_at_ms": published.updated_at_ms,
                    });
                    store.complete_orchestration_tool_request(
                        tenant,
                        self.kind.name(),
                        idempotency_key,
                        &request_digest,
                        &response,
                        crate::now_ms(),
                    )?;
                    return Ok(response);
                }
                let now = crate::now_ms();
                let next_version = store
                    .latest_published_orchestration_version(tenant, &draft.orchestration_id)?
                    .unwrap_or(0)
                    .saturating_add(1);
                draft.status = OrchestrationStatus::Published;
                draft.version = next_version;
                draft.published_at_ms = Some(now);
                draft.updated_at_ms = now;
                let validation =
                    super::orchestrations_api::full_validation_report(&self.state, tenant, &draft)
                        .await;
                if !validation.report.valid || !validation.stale_references.is_empty() {
                    bail!("publishing requires a valid graph and refreshed workflow references");
                }
                let mut metadata = draft.metadata.take().unwrap_or_else(|| json!({}));
                merge_object_value(
                    &mut metadata,
                    "publish",
                    json!({
                        "actor": actor,
                        "published_at_ms": now,
                        "validation": validation.report,
                        "workflow_definition_hashes": validation.workflow_hashes,
                        "idempotency_key": idempotency_key,
                        "request_digest": request_digest,
                    }),
                );
                draft.metadata = Some(metadata);
                store.publish_orchestration_draft(&draft, Some(expected_draft_updated_at_ms))?;
                let response = json!({
                    "orchestration": draft,
                    "orchestration_id": draft.orchestration_id,
                    "version": draft.version,
                    "status": draft.status,
                    "updated_at_ms": draft.updated_at_ms,
                });
                store.complete_orchestration_tool_request(
                    tenant,
                    self.kind.name(),
                    idempotency_key,
                    &request_digest,
                    &response,
                    crate::now_ms(),
                )?;
                Ok(response)
            }
            OrchestrationToolKind::GoalStart => {
                let actor = mutation_actor(args, tenant, None)?;
                let mut metadata = args.get("metadata").cloned().unwrap_or_else(|| json!({}));
                merge_object_value(&mut metadata, "started_by", json!(actor));
                let request = crate::app::state::StartGoalRequest {
                    orchestration_id: required_str(args, "orchestration_id")?.to_string(),
                    orchestration_version: args
                        .get("orchestration_version")
                        .and_then(Value::as_u64),
                    objective: required_str(args, "objective")?.to_string(),
                    idempotency_key: require_idempotency(args)?.to_string(),
                    metadata: Some(metadata),
                    now_ms: crate::now_ms(),
                };
                match self
                    .state
                    .start_long_running_goal(tenant, &request, &actor)
                    .await?
                {
                    StartGoalOutcome::Created { goal, root_run } => Ok(json!({
                        "goal": goal,
                        "root_run_id": root_run.run_id,
                        "replayed": false,
                    })),
                    StartGoalOutcome::AlreadyStarted { goal, root_run } => Ok(json!({
                        "goal": goal,
                        "root_run_id": root_run.run_id,
                        "replayed": true,
                    })),
                }
            }
            OrchestrationToolKind::GoalGet => {
                let goal = scoped_goal(&store, tenant, required_str(args, "goal_id")?)?;
                let run_links = store.list_goal_run_links_for_tenant(tenant, &goal.goal_id)?;
                let handoffs = store.list_goal_handoffs_for_tenant(tenant, &goal.goal_id)?;
                let run_ids = run_links
                    .iter()
                    .map(|link| link.run_id.as_str())
                    .collect::<HashSet<_>>();
                let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(
                    &self.state.runtime_events_path,
                );
                let waits = crate::stateful_runtime::list_stateful_waits(
                    &paths.waits_path,
                    tenant,
                    StatefulWaitQuery::default(),
                )
                .into_iter()
                .filter(|wait| run_ids.contains(wait.run_id.as_str()))
                .collect::<Vec<_>>();
                Ok(json!({
                    "goal": goal,
                    "run_links": run_links,
                    "handoffs": handoffs,
                    "waits": waits,
                    "budgets": budget_value(&goal),
                }))
            }
            OrchestrationToolKind::GoalCancel => {
                let actor = mutation_actor(args, tenant, None)?;
                let idempotency_key = require_idempotency(args)?;
                let goal = scoped_goal(&store, tenant, required_str(args, "goal_id")?)?;
                authorize_goal_owner(args, tenant, &goal, &actor)?;
                let request_digest = crate::stateful_runtime::stable_definition_snapshot_hash(
                    &json!({ "tenant": tenant, "request": public_args(args), "actor": actor.id }),
                );
                if let Some(response) = store.begin_orchestration_tool_request(
                    tenant,
                    self.kind.name(),
                    idempotency_key,
                    &request_digest,
                    crate::now_ms(),
                )? {
                    return Ok(response);
                }
                let result = self
                    .state
                    .cancel_long_running_goal(
                        &goal.goal_id,
                        tenant,
                        args.get("reason")
                            .and_then(Value::as_str)
                            .unwrap_or("cancelled by agent"),
                        &actor,
                    )
                    .await?;
                let response = json!({
                    "outcome": format!("{:?}", result.outcome),
                    "goal": result.goal,
                    "cancelled_run_id": result.cancelled_run.as_ref().map(|run| &run.run_id),
                    "cancelled_wait_ids": result.cancelled_wait_ids,
                });
                store.complete_orchestration_tool_request(
                    tenant,
                    self.kind.name(),
                    idempotency_key,
                    &request_digest,
                    &response,
                    crate::now_ms(),
                )?;
                Ok(response)
            }
            OrchestrationToolKind::HandoffEmit => {
                let actor = mutation_actor(args, tenant, None)?;
                let goal = scoped_goal(&store, tenant, required_str(args, "goal_id")?)?;
                authorize_goal_owner(args, tenant, &goal, &actor)?;
                if goal.status.is_terminal() {
                    bail!("terminal goals cannot emit transitions");
                }
                let artifact: OrchestrationArtifactRef = serde_json::from_value(
                    args.get("artifact")
                        .cloned()
                        .context("artifact is required")?,
                )?;
                let workspace_root = self.state.workspace_index.snapshot().await.root;
                super::goals_api::enforce_artifact_content_policy(&workspace_root, &artifact)?;
                let result = self
                    .state
                    .emit_orchestration_transition(
                        &goal.goal_id,
                        &GovernedTransitionRequest {
                            transition_key: required_str(args, "transition_key")?.to_string(),
                            idempotency_key: require_idempotency(args)?.to_string(),
                            artifact,
                            now_ms: crate::now_ms(),
                        },
                        &OrchestrationTransitionAuthority {
                            actor,
                            can_emit: true,
                            can_approve: false,
                        },
                    )
                    .await?;
                Ok(transition_value(result))
            }
            OrchestrationToolKind::HandoffApprove => {
                let actor = mutation_actor(args, tenant, Some("orchestration.approve"))?;
                let idempotency_key = require_idempotency(args)?;
                let goal = scoped_goal(&store, tenant, required_str(args, "goal_id")?)?;
                let handoff_id = required_str(args, "handoff_id")?;
                let handoff = store
                    .get_workflow_handoff_for_tenant(tenant, handoff_id)?
                    .filter(|handoff| handoff.goal_id == goal.goal_id)
                    .context("handoff not found")?;
                let approve = match required_str(args, "decision")? {
                    "approve" => true,
                    "reject" => false,
                    _ => bail!("decision must be approve or reject"),
                };
                let request_digest = crate::stateful_runtime::stable_definition_snapshot_hash(
                    &json!({ "tenant": tenant, "request": public_args(args), "actor": actor.id }),
                );
                if let Some(response) = store.begin_orchestration_tool_request(
                    tenant,
                    self.kind.name(),
                    idempotency_key,
                    &request_digest,
                    crate::now_ms(),
                )? {
                    return Ok(response);
                }
                let decided_status = if approve {
                    tandem_automation::WorkflowHandoffStatus::Approved
                } else {
                    tandem_automation::WorkflowHandoffStatus::Rejected
                };
                if handoff.status == decided_status {
                    let response = json!({ "handoff": handoff });
                    store.complete_orchestration_tool_request(
                        tenant,
                        self.kind.name(),
                        idempotency_key,
                        &request_digest,
                        &response,
                        crate::now_ms(),
                    )?;
                    return Ok(response);
                }
                let decided = store.decide_pending_handoff(
                    &handoff.handoff_id,
                    &goal.tenant_context,
                    approve,
                    &OrchestrationTransitionAuthority {
                        actor,
                        can_emit: false,
                        can_approve: true,
                    },
                    crate::now_ms(),
                )?;
                let response = json!({ "handoff": decided });
                store.complete_orchestration_tool_request(
                    tenant,
                    self.kind.name(),
                    idempotency_key,
                    &request_digest,
                    &response,
                    crate::now_ms(),
                )?;
                Ok(response)
            }
            OrchestrationToolKind::WaitInspect => {
                let goal = scoped_goal(&store, tenant, required_str(args, "goal_id")?)?;
                let wait = scoped_wait(
                    &self.state,
                    &store,
                    tenant,
                    &goal,
                    required_str(args, "wait_id")?,
                )?;
                Ok(json!({ "goal_id": goal.goal_id, "wait": wait }))
            }
            OrchestrationToolKind::WaitResolve => {
                mutation_actor(args, tenant, Some("orchestration.resolve_wait"))?;
                let goal = scoped_goal(&store, tenant, required_str(args, "goal_id")?)?;
                if goal.status.is_terminal() {
                    bail!("terminal goals cannot resolve waits");
                }
                let wait_id = required_str(args, "wait_id")?;
                scoped_wait(&self.state, &store, tenant, &goal, wait_id)?;
                let wait = self
                    .state
                    .resolve_automation_v2_external_wait(
                        tenant,
                        wait_id,
                        require_idempotency(args)?,
                        args.get("resolution").cloned().unwrap_or(Value::Null),
                    )
                    .await?
                    .context("wait is not resolvable")?;
                Ok(json!({ "goal_id": goal.goal_id, "wait": wait }))
            }
        }
    }
}

fn scoped_draft(
    store: &OrchestrationStateStore,
    tenant: &TenantContext,
    orchestration_id: &str,
) -> anyhow::Result<OrchestrationSpec> {
    store
        .get_orchestration_draft(tenant, orchestration_id)?
        .context("orchestration draft not found")
}

fn scoped_goal(
    store: &OrchestrationStateStore,
    tenant: &TenantContext,
    goal_id: &str,
) -> anyhow::Result<LongRunningGoal> {
    // Scope runs in the store's SQL; a cross-tenant goal ID is absence.
    store
        .get_goal_for_tenant(tenant, goal_id)?
        .context("goal not found")
}

fn scoped_wait(
    state: &AppState,
    store: &OrchestrationStateStore,
    tenant: &TenantContext,
    goal: &LongRunningGoal,
    wait_id: &str,
) -> anyhow::Result<crate::stateful_runtime::StatefulWaitRecord> {
    let run_ids = store
        .list_goal_run_links_for_tenant(tenant, &goal.goal_id)?
        .into_iter()
        .map(|link| link.run_id)
        .collect::<HashSet<_>>();
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    crate::stateful_runtime::list_stateful_waits(
        &paths.waits_path,
        tenant,
        StatefulWaitQuery::default(),
    )
    .into_iter()
    .find(|wait| wait.wait_id == wait_id && run_ids.contains(&wait.run_id))
    .context("wait not found")
}

fn transition_value(result: GovernedTransitionResult) -> Value {
    match result {
        GovernedTransitionResult::PendingApproval { handoff, goal } => json!({
            "outcome": "pending_approval",
            "handoff": handoff,
            "goal": goal,
        }),
        GovernedTransitionResult::Committed {
            commit,
            handoff,
            downstream_run,
            link,
            goal,
        } => json!({
            "outcome": "committed",
            "commit": format!("{commit:?}"),
            "handoff": handoff,
            "downstream_run_id": downstream_run.run_id,
            "link": link,
            "goal": goal,
        }),
    }
}

fn verified_context(args: &Value) -> Option<tandem_types::VerifiedTenantContext> {
    args.get("__verified_tenant_context")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn verified_admin(args: &Value) -> bool {
    verified_context(args).is_some_and(|context| {
        context.roles.iter().any(|role| {
            matches!(
                role.as_str(),
                "owner" | "admin" | "hosted:owner" | "hosted:admin" | "enterprise:admin"
            )
        }) || context.capabilities.iter().any(|capability| {
            matches!(
                capability.as_str(),
                "hosted.admin" | "orchestration.control" | "orchestration.write"
            )
        })
    })
}

fn mutation_actor(
    args: &Value,
    tenant: &TenantContext,
    required_capability: Option<&str>,
) -> anyhow::Result<tandem_types::PrincipalRef> {
    let verified = verified_context(args);
    if !tenant.is_local_implicit() && verified.is_none() {
        bail!("mutation requires a verified tenant principal");
    }
    if let Some(capability) = required_capability {
        let authorized = verified.as_ref().is_some_and(|context| {
            context.capabilities.iter().any(|value| value == capability)
                || context.roles.iter().any(|role| {
                    matches!(
                        role.as_str(),
                        "owner" | "admin" | "hosted:owner" | "hosted:admin" | "enterprise:admin"
                    )
                })
        });
        if !authorized {
            bail!("authenticated principal lacks {capability} authority");
        }
    }
    let actor = verified
        .as_ref()
        .map(|context| context.human_actor.actor_id.trim())
        .filter(|actor| !actor.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| tenant.actor_id.clone())
        .or_else(|| {
            tenant
                .is_local_implicit()
                .then(|| "local-operator".to_string())
        })
        .context("mutation requires an authenticated principal")?;
    Ok(tandem_types::PrincipalRef::human_user(actor))
}

fn authorize_orchestration_owner(
    args: &Value,
    tenant: &TenantContext,
    spec: &OrchestrationSpec,
    actor: &tandem_types::PrincipalRef,
) -> anyhow::Result<()> {
    if metadata_str(spec, "created_by") != Some(actor.id.as_str())
        && !tenant.is_local_implicit()
        && !verified_admin(args)
    {
        bail!("orchestration mutation requires its owner or an authorized administrator");
    }
    Ok(())
}

fn authorize_goal_owner(
    args: &Value,
    tenant: &TenantContext,
    goal: &LongRunningGoal,
    actor: &tandem_types::PrincipalRef,
) -> anyhow::Result<()> {
    let started_by = goal
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("started_by"))
        .and_then(|value| {
            value
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
        });
    if started_by != Some(actor.id.as_str()) && !tenant.is_local_implicit() && !verified_admin(args)
    {
        bail!("goal mutation requires its initiating actor or an authorized administrator");
    }
    Ok(())
}

fn metadata_str<'a>(spec: &'a OrchestrationSpec, key: &str) -> Option<&'a str> {
    spec.metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| {
            value
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
        })
}

fn publish_metadata_str<'a>(spec: &'a OrchestrationSpec, key: &str) -> Option<&'a str> {
    spec.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("publish"))
        .and_then(|publish| publish.get(key))
        .and_then(Value::as_str)
}

fn public_args(args: &Value) -> Value {
    let mut value = args.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("__verified_tenant_context");
    }
    value
}

fn required_str<'a>(args: &'a Value, field: &str) -> anyhow::Result<&'a str> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{field} is required"))
}

fn require_idempotency(args: &Value) -> anyhow::Result<&str> {
    required_str(args, "idempotency_key")
}

fn merge_object_value(target: &mut Value, key: &str, value: Value) {
    if !target.is_object() {
        *target = json!({ "source_metadata": target.take() });
    }
    if let Some(object) = target.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

fn scope_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn budget_value(goal: &LongRunningGoal) -> Value {
    json!({
        "policy": goal.policy,
        "consumed": {
            "hops": goal.hop_count,
            "total_tokens": goal.total_tokens,
            "total_cost_usd": goal.total_cost_usd,
        },
        "remaining": {
            "hops": goal.policy.max_hops.saturating_sub(goal.hop_count),
            "tokens": goal.policy.max_total_tokens.map(|limit| limit.saturating_sub(goal.total_tokens)),
            "cost_usd": goal.policy.max_total_cost_usd.map(|limit| (limit - goal.total_cost_usd).max(0.0)),
        }
    })
}
