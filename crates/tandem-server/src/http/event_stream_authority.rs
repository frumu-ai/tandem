// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::{tenant_matches, AppState};
use axum::response::sse::Event;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::{convert::Infallible, time::Duration};
use tandem_types::{
    AccessDecision, AccessPermission, DataClass, EngineEvent, TenantContext, VerifiedTenantContext,
};
use tokio_stream::wrappers::BroadcastStream;

// Evaluate one current immutable revision. Never reuse connection-time roles,
// group claims, or a strict projection for resource visibility after an await.
pub(super) fn current_context(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    permission: Option<AccessPermission>,
) -> Result<Option<VerifiedTenantContext>, ()> {
    let Some(policy) = state.enterprise.hosted_policy.current().map_err(|_| ())? else {
        return Ok(None);
    };
    let verified = verified.ok_or(())?;
    if !tenant_matches(tenant, &verified.tenant_context)
        || tenant.actor_id != verified.tenant_context.actor_id
    {
        return Err(());
    }
    let now = crate::now_ms();
    let projection = policy.project_identity(verified, now).map_err(|_| ())?;
    let allowed = |permission| {
        projection
            .evaluate_access(
                &policy.deployment_resource(),
                permission,
                DataClass::Internal,
                now,
            )
            .decision
            == AccessDecision::Allow
    };
    if permission.is_some_and(|permission| !allowed(permission)) {
        return Err(());
    }
    let memberships = policy
        .memberships_for_identity(verified, now)
        .map_err(|_| ())?;
    let mut current = verified.clone();
    current.org_units.retain(|unit| {
        memberships.iter().any(|row| {
            row.unit == tandem_enterprise_contract::hosted_policy::hosted_unit_principal(unit)
        })
    });
    current.roles.clear();
    current.capabilities = [
        (AccessPermission::HostedAdmin, "hosted.admin"),
        (AccessPermission::HostedAutomationWrite, "automation.write"),
        (AccessPermission::HostedAutomationShare, "automation.share"),
    ]
    .into_iter()
    .filter(|(permission, _)| allowed(*permission))
    .map(|(_, name)| name.to_owned())
    .collect();
    current.strict_projection = Some(projection);
    Ok(Some(current))
}

// Covers initial frames, queued events, and a read already pending when policy
// expires or is revoked. A denied connection ends permanently, even while idle.
pub(super) fn guard<S>(
    stream: S,
    state: AppState,
    tenant: TenantContext,
    verified: Option<VerifiedTenantContext>,
    permission: Option<AccessPermission>,
) -> impl Stream<Item = Result<Event, Infallible>>
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    futures::stream::unfold(
        (
            Box::pin(stream),
            state,
            tenant,
            verified,
            tokio::time::interval(Duration::from_secs(1)),
        ),
        move |(mut stream, state, tenant, verified, mut timer)| async move {
            loop {
                current_context(&state, &tenant, verified.as_ref(), permission).ok()?;
                tokio::select! {
                    biased;
                    _ = timer.tick() => continue,
                    item = stream.next() => {
                        let item = item?;
                        current_context(&state, &tenant, verified.as_ref(), permission).ok()?;
                        return Some((item, (stream, state, tenant, verified, timer)));
                    }
                }
            }
        },
    )
    .fuse()
}

// A run stream must retain the resolved session identity after the active
// registry slot is released, and must recheck its durable owner and current
// hosted grants before queued frames as well as while the stream is idle.
pub(super) fn guard_run<S>(
    stream: S,
    state: AppState,
    tenant: TenantContext,
    verified: Option<VerifiedTenantContext>,
    resource: super::context_run_authority::RunStreamResource,
) -> impl Stream<Item = Result<Event, Infallible>>
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    futures::stream::unfold(
        (
            Box::pin(stream),
            state,
            tenant,
            verified,
            resource,
            tokio::time::interval(Duration::from_secs(1)),
        ),
        |(mut stream, state, tenant, verified, resource, mut timer)| async move {
            loop {
                if !super::context_run_authority::run_stream_resource_visible(
                    &state,
                    &tenant,
                    verified.as_ref(),
                    &resource,
                )
                .await
                {
                    return None;
                }
                tokio::select! {
                    biased;
                    _ = timer.tick() => continue,
                    item = stream.next() => {
                        let item = item?;
                        if !super::context_run_authority::run_stream_resource_visible(
                            &state,
                            &tenant,
                            verified.as_ref(),
                            &resource,
                        )
                        .await
                        {
                            return None;
                        }
                        return Some((item, (stream, state, tenant, verified, resource, timer)));
                    }
                }
            }
        },
    )
    .fuse()
}

// Do not choose whichever alias happens to appear first when representations
// conflict. Null optional fields are absent; malformed non-null IDs deny.
fn consistent_id<'a>(
    values: impl IntoIterator<Item = Option<&'a Value>>,
) -> Result<Option<&'a str>, ()> {
    let mut id = None;
    for value in values
        .into_iter()
        .flatten()
        .filter(|value| !value.is_null())
    {
        let value = value
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or(())?;
        if id.is_some_and(|old| old != value) {
            return Err(());
        }
        id = Some(value);
    }
    Ok(id)
}

fn property_id<'a>(properties: &'a Value, keys: &[&str]) -> Result<Option<&'a str>, ()> {
    consistent_id(keys.iter().map(|key| properties.get(*key)))
}

fn event_tenant(event: &EngineEvent) -> Result<Option<TenantContext>, ()> {
    let mut tenant = event
        .envelope
        .as_ref()
        .and_then(|row| row.tenant_context.clone());
    for value in [
        event.properties.get("tenantContext"),
        event.properties.get("tenant_context"),
    ]
    .into_iter()
    .flatten()
    {
        let value: TenantContext = serde_json::from_value(value.clone()).map_err(|_| ())?;
        if tenant.as_ref().is_some_and(|old| old != &value) {
            return Err(());
        }
        tenant = Some(value);
    }
    Ok(tenant)
}

async fn visible(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    event: &EngineEvent,
) -> Result<bool, ()> {
    if current_context(state, tenant, verified, None)?.is_none() {
        return Ok(super::global::event_visible_to_tenant(event, tenant));
    }
    let asserted_tenant = event_tenant(event)?;
    if asserted_tenant
        .as_ref()
        .is_some_and(|other| !tenant_matches(tenant, other))
    {
        return Ok(false);
    }
    let properties = &event.properties;
    let workflow = property_id(properties, &["workflowID", "workflowId", "workflow_id"])?;
    let goal_event = event.event_type.starts_with("orchestration.goal.");
    let stateful_wait = event.event_type.starts_with("stateful_runtime.wait.");
    let embedded_run = goal_event.then(|| properties.get("run")).flatten();
    let automation = consistent_id(
        ["automationID", "automationId", "automation_id"]
            .into_iter()
            .flat_map(|key| {
                [
                    properties.get(key),
                    embedded_run.and_then(|run| run.get(key)),
                ]
            }),
    )?;
    let routine = property_id(properties, &["routineID", "routineId", "routine_id"])?;
    let run = consistent_id(
        ["runID", "runId", "run_id"]
            .into_iter()
            .flat_map(|key| {
                [
                    properties.get(key),
                    embedded_run.and_then(|run| run.get(key)),
                ]
            })
            .chain(
                goal_event
                    .then(|| properties.get("rootRunID"))
                    .flatten()
                    .map(Some),
            ),
    )?;
    if let (Some(run), Some(envelope_run)) = (
        run,
        event
            .envelope
            .as_ref()
            .and_then(|row| row.run_id.as_deref()),
    ) {
        if run != envelope_run {
            return Err(());
        }
    }
    let run = run.or_else(|| {
        event
            .envelope
            .as_ref()
            .and_then(|row| row.run_id.as_deref())
    });
    // Context projections are another representation of the same resource.
    let context_run = event
        .event_type
        .starts_with("context.")
        .then_some(run)
        .flatten();
    let workflow_run = context_run.and_then(|id| id.strip_prefix("workflow-"));
    // Scheduler and webhook wait notifications use generic run IDs. Resolve
    // the canonical resource type; event labels/actor attribution are not ACLs.
    let stateful_workflow = if stateful_wait {
        let id = run.ok_or(())?;
        let workflow = state.get_workflow_run(id).await;
        if workflow.is_some() && state.get_automation_v2_run(id).await.is_some() {
            return Ok(false);
        }
        workflow
    } else {
        None
    };
    // Automation context tasks also publish workflowID as a legacy alias for
    // automationID. Their canonical automation reference owns that event.
    if workflow_run.is_some()
        || stateful_workflow.is_some()
        || event.event_type.starts_with("workflow.")
        || (workflow.is_some() && automation.is_none())
    {
        let Some(id) = workflow_run.or(run) else {
            return Ok(false);
        };
        let record = match stateful_workflow {
            Some(record) => Some(record),
            None => state.get_workflow_run(id).await,
        };
        let Some(record) = record else {
            return Ok(false);
        };
        if workflow.is_some_and(|id| id != record.workflow_id) {
            return Ok(false);
        }
        let current = current_context(
            state,
            tenant,
            verified,
            Some(AccessPermission::HostedWorkflowRead),
        )?
        .ok_or(())?;
        return Ok(super::workflows::workflow_run_visible_to_caller(
            &record,
            tenant,
            &tandem_types::RequestPrincipal::authenticated_user(
                &current.human_actor.actor_id,
                "event-stream",
            ),
            Some(&current),
        ));
    }
    let automation_run = context_run.and_then(|id| id.strip_prefix("automation-v2-"));
    if automation.is_some()
        || event.event_type.starts_with("automation.")
        || event.event_type.starts_with("automation_v2.")
        || automation_run.is_some()
        || goal_event
        || stateful_wait
    {
        // Pause/resume notifications contain only a goal ID. Resolve its run
        // from the tenant-scoped durable store, never from event-supplied ACLs.
        let goal_run = if goal_event && run.is_none() {
            let goal_id = property_id(properties, &["goalID", "goalId", "goal_id"])?
                .ok_or(())?
                .to_owned();
            let path = state.automation_v2_runs_path.clone();
            let tenant = tenant.clone();
            tokio::task::spawn_blocking(move || {
                crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(&path)?
                    .get_goal_for_tenant(&tenant, &goal_id)
            })
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?
            .and_then(|goal| goal.active_run_id)
        } else {
            None
        };
        let record = if let Some(id) = automation_run.or(run).or(goal_run.as_deref()) {
            state.get_automation_v2_run(id).await
        } else {
            None
        };
        if (goal_event || stateful_wait) && record.is_none() {
            return Ok(false);
        }
        if record.as_ref().is_some_and(|record| {
            !tenant_matches(tenant, &record.tenant_context)
                || automation.is_some_and(|id| id != record.automation_id)
        }) {
            return Ok(false);
        }
        let id = record
            .as_ref()
            .map(|row| row.automation_id.as_str())
            .or(automation)
            .ok_or(())?;
        let spec = state
            .get_automation_v2(id)
            .await
            .or_else(|| record.and_then(|row| row.automation_snapshot));
        let Some(spec) = spec else {
            return Ok(false);
        };
        let current = current_context(
            state,
            tenant,
            verified,
            Some(AccessPermission::HostedAutomationRead),
        )?
        .ok_or(())?;
        return Ok(tenant_matches(tenant, &spec.tenant_context())
            && super::routines_automations::automation_v2_visible_to_context(
                &spec,
                Some(&current),
            ));
    }
    let routine_run = context_run.and_then(|id| id.strip_prefix("routine-"));
    // The routine deletion handler publishes this tenant-scoped tombstone
    // after removal. Routine reads are tenant-wide; there is no remaining row.
    if event.event_type == "routine.deleted" && routine.is_some() && run.is_none() {
        current_context(
            state,
            tenant,
            verified,
            Some(AccessPermission::HostedAutomationRead),
        )?;
        return Ok(asserted_tenant
            .as_ref()
            .is_some_and(|owner| tenant_matches(tenant, owner)));
    }
    if routine.is_some() || event.event_type.starts_with("routine.") || routine_run.is_some() {
        let owner = if let Some(id) = routine_run.or(run) {
            let record = state
                .get_routine_run_for_tenant(id, tenant)
                .await
                .ok_or(())?;
            if routine.is_some_and(|id| id != record.routine_id) {
                return Ok(false);
            }
            record.tenant_context
        } else {
            state
                .get_routine_for_tenant(routine.ok_or(())?, tenant)
                .await
                .ok_or(())?
                .tenant_context
        };
        current_context(
            state,
            tenant,
            verified,
            Some(AccessPermission::HostedAutomationRead),
        )?;
        return Ok(tenant_matches(tenant, &owner));
    }
    let mut session = consistent_id(
        ["sessionID", "sessionId", "session_id"]
            .into_iter()
            .flat_map(|key| {
                [
                    properties.get(key),
                    properties.get("part").and_then(|part| part.get(key)),
                    properties.get("record").and_then(|record| record.get(key)),
                ]
            }),
    )?;
    if let Some(envelope_session) = event
        .envelope
        .as_ref()
        .and_then(|row| row.session_id.as_deref())
    {
        if session.is_some_and(|id| id != envelope_session) {
            return Err(());
        }
        session = Some(envelope_session);
    }
    session = session.or_else(|| context_run.and_then(|id| id.strip_prefix("session-")));
    if let Some(id) = session {
        let record = state.storage.get_session(id).await.ok_or(())?;
        current_context(state, tenant, verified, None)?;
        return Ok(super::sessions_actor_scope::session_visible_to_actor(
            tenant,
            &record.tenant_context,
        ));
    }
    // Tenant-scoped non-resource notifications still require the exact actor;
    // unknown/missing ownership must not become deployment-wide disclosure.
    current_context(state, tenant, verified, None)?;
    Ok(asserted_tenant.as_ref().is_some_and(|owner| {
        !owner.is_local_implicit()
            && super::sessions_actor_scope::session_visible_to_actor(tenant, owner)
    }))
}

pub(super) fn subscribe(
    state: AppState,
    tenant: TenantContext,
    verified: Option<VerifiedTenantContext>,
) -> impl Stream<Item = EngineEvent> + Send + 'static {
    let receiver = state.event_bus.subscribe();
    BroadcastStream::new(receiver).filter_map(move |event| {
        let state = state.clone();
        let tenant = tenant.clone();
        let verified = verified.clone();
        async move {
            let event = event.ok()?;
            visible(&state, &tenant, verified.as_ref(), &event)
                .await
                .ok()?
                .then_some(event)
        }
    })
}
