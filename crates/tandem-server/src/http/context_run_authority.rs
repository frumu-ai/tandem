// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Resolve a context run's durable owner before exposing its state or events.
//! The context row is a projection for managed runs, not an independent ACL.

use super::{
    context_types::ContextRunState, event_stream_authority::current_context, tenant_matches,
};
use crate::AppState;
use axum::{
    extract::{MatchedPath, Path, Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use tandem_types::{AccessPermission, RequestPrincipal, TenantContext, VerifiedTenantContext};

#[derive(Clone)]
pub(super) enum RunStreamResource {
    ActiveSession(String),
    ContextRun(String),
}

pub(super) fn reserved_projection_id(run_id: &str) -> bool {
    ["session-", "automation-v2-", "workflow-", "routine-"]
        .iter()
        .any(|prefix| run_id.starts_with(prefix))
}

pub(super) fn valid_context_run_id(run_id: &str) -> bool {
    // A run ID is a single directory name, not a path. Checking this before
    // the prefix guard also prevents `x/../workflow-id` from preempting a
    // managed projection's directory under an innocuous-looking ID.
    !run_id.is_empty()
        && !run_id.contains('/')
        && !run_id.contains('\\')
        && !run_id.chars().any(char::is_control)
        && matches!(
            std::path::Path::new(run_id).components().next(),
            Some(std::path::Component::Normal(_))
        )
        && std::path::Path::new(run_id).components().count() == 1
}

pub(super) fn managed_projection_type(kind: &str) -> bool {
    matches!(
        kind,
        "session" | "automation_v2" | "incident_monitor_triage" | "workflow" | "routine"
    )
}

pub(super) async fn resolve_run_stream_resource(
    state: &AppState,
    run_id: &str,
) -> Option<RunStreamResource> {
    if let Some(session_id) = state.run_registry.session_for_run(run_id).await {
        return Some(RunStreamResource::ActiveSession(session_id));
    }
    super::context_runs::load_context_run_state(state, run_id)
        .await
        .ok()
        .map(|_| RunStreamResource::ContextRun(run_id.to_owned()))
}

pub(super) async fn run_stream_resource_visible(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    resource: &RunStreamResource,
) -> bool {
    match resource {
        RunStreamResource::ActiveSession(session_id) => {
            let Some(session) = state.storage.get_session(session_id).await else {
                return false;
            };
            current_context(state, tenant, verified, None).is_ok()
                && super::sessions_actor_scope::session_visible_to_actor(
                    tenant,
                    &session.tenant_context,
                )
        }
        RunStreamResource::ContextRun(run_id) => {
            let Ok(run) = super::context_runs::load_context_run_state(state, run_id).await else {
                return false;
            };
            context_run_visible(state, &run, tenant, verified, false).await
        }
    }
}

pub(super) async fn context_run_visible(
    state: &AppState,
    run: &ContextRunState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    mutation: bool,
) -> bool {
    if !tenant_matches(tenant, &run.tenant_context) {
        return false;
    }
    let owner =
        || super::sessions_actor_scope::session_visible_to_actor(tenant, &run.tenant_context);
    let visible = match run.run_type.as_str() {
        "session" => {
            let Some(session_id) = run.run_id.strip_prefix("session-") else {
                return false;
            };
            let Some(session) = state.storage.get_session(session_id).await else {
                return false;
            };
            current_context(state, tenant, verified, None).is_ok()
                && run.tenant_context == session.tenant_context
                && super::sessions_actor_scope::session_visible_to_actor(
                    tenant,
                    &session.tenant_context,
                )
        }
        // Incident Monitor triage relabels its automation-v2 projection for
        // clients, but the native automation run and spec still own its ACL.
        "automation_v2" | "incident_monitor_triage" => {
            let Some(native_id) = run.run_id.strip_prefix("automation-v2-") else {
                return false;
            };
            let Some(record) = state.get_automation_v2_run(native_id).await else {
                return false;
            };
            if record.tenant_context != run.tenant_context {
                return false;
            }
            let Some(spec) = state
                .get_automation_v2(&record.automation_id)
                .await
                .or(record.automation_snapshot)
            else {
                return false;
            };
            let Ok(current) = current_context(
                state,
                tenant,
                verified,
                Some(AccessPermission::HostedAutomationRead),
            ) else {
                return false;
            };
            tenant_matches(tenant, &spec.tenant_context())
                && if let Some(current) = current.as_ref() {
                    super::routines_automations::automation_v2_visible_to_context(
                        &spec,
                        Some(current),
                    )
                } else {
                    owner()
                }
        }
        "workflow" => {
            let Some(native_id) = run.run_id.strip_prefix("workflow-") else {
                return false;
            };
            let Some(record) = state.get_workflow_run(native_id).await else {
                return false;
            };
            if record.tenant_context != run.tenant_context {
                return false;
            }
            let Ok(current) = current_context(
                state,
                tenant,
                verified,
                Some(AccessPermission::HostedWorkflowRead),
            ) else {
                return false;
            };
            let actor = current
                .as_ref()
                .map(|value| value.human_actor.actor_id.as_str())
                .or(tenant.actor_id.as_deref())
                .unwrap_or_default();
            super::workflows::workflow_run_visible_to_caller(
                &record,
                tenant,
                &RequestPrincipal::authenticated_user(actor, "context-run"),
                current.as_ref(),
            )
        }
        "routine" => {
            let Some(native_id) = run.run_id.strip_prefix("routine-") else {
                return false;
            };
            let Some(record) = state.get_routine_run(native_id).await else {
                return false;
            };
            current_context(
                state,
                tenant,
                verified,
                Some(AccessPermission::HostedAutomationRead),
            )
            .is_ok()
                && record.tenant_context == run.tenant_context
                && owner()
        }
        _ => {
            // A caller cannot relabel a managed projection as an interactive
            // run to bypass its canonical run/session ACL.
            if reserved_projection_id(&run.run_id) {
                return false;
            }
            current_context(state, tenant, verified, None).is_ok() && owner()
        }
    };
    if !visible || !mutation {
        return visible;
    }
    // A resource reader (including a shared automation audience or workflow
    // reviewer) is not thereby allowed to mutate tasks, checkpoints or files.
    let permission = match run.run_type.as_str() {
        "automation_v2" | "incident_monitor_triage" | "routine" => {
            AccessPermission::HostedAutomationExecute
        }
        _ => AccessPermission::HostedUse,
    };
    owner() && current_context(state, tenant, verified, Some(permission)).is_ok()
}

// All by-ID context routes, including ledger, checkpoint rollback and other
// handlers that did not previously consult tenant context, enter here. Lists
// and multiplex streams still need per-row checks in their own handlers.
pub(super) async fn guard_context_route(
    State(state): State<AppState>,
    path: Option<Path<HashMap<String, String>>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(matched) = request.extensions().get::<MatchedPath>() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !matched.as_str().contains("{run_id}") {
        return next.run(request).await;
    }
    let Some(Path(params)) = path else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(run_id) = params.get("run_id") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(tenant) = request.extensions().get::<TenantContext>() else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let verified = request.extensions().get::<VerifiedTenantContext>();
    let mutation = !matches!(*request.method(), Method::GET | Method::HEAD);
    let allowed = if let Ok(run) = super::context_runs::load_context_run_state(&state, run_id).await
    {
        // Rollback can write or delete workspace files, so ordinary run
        // execution authority is insufficient in hosted deployments.
        context_run_visible(&state, &run, tenant, verified, mutation).await
            && (!matched
                .as_str()
                .ends_with("/checkpoints/mutations/rollback-execute")
                || current_context(
                    &state,
                    tenant,
                    verified,
                    Some(AccessPermission::HostedAdmin),
                )
                .is_ok())
    } else if matched.as_str() == "/context/runs/{run_id}" && *request.method() == Method::PUT {
        // PUT is also a create path. It cannot preempt reserved projection IDs.
        !reserved_projection_id(run_id)
            && current_context(&state, tenant, verified, Some(AccessPermission::HostedUse)).is_ok()
    } else {
        false
    };
    if !allowed {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::tests::{test_automation_node, AutomationSpecBuilder};

    #[tokio::test]
    async fn triage_projection_uses_native_automation_scope() {
        let state = crate::test_support::test_state().await;
        let alice = TenantContext::explicit("triage-org", "triage-workspace", Some("alice".into()));
        let bob = TenantContext::explicit("triage-org", "triage-workspace", Some("bob".into()));
        let other_tenant =
            TenantContext::explicit("other-org", "triage-workspace", Some("alice".into()));
        let mut spec = AutomationSpecBuilder::new("incident-monitor-triage-authority")
            .nodes(vec![test_automation_node("inspect", vec![], "triage", 0)])
            .build();
        spec.set_tenant_context(&alice);
        let spec = state.put_automation_v2(spec).await.unwrap();
        let native = state
            .create_automation_v2_run(&spec, "incident_monitor_triage")
            .await
            .unwrap();
        let projection_id =
            super::super::context_runs::automation_v2_context_run_id(&native.run_id);
        let mut projection =
            super::super::context_runs::load_context_run_state(&state, &projection_id)
                .await
                .unwrap();
        projection.run_type = "incident_monitor_triage".to_string();

        assert!(managed_projection_type(&projection.run_type));
        assert!(context_run_visible(&state, &projection, &alice, None, false).await);
        assert!(context_run_visible(&state, &projection, &alice, None, true).await);
        assert!(!context_run_visible(&state, &projection, &bob, None, false).await);
        assert!(!context_run_visible(&state, &projection, &other_tenant, None, false).await);

        let mut wrong_projection_scope = projection.clone();
        wrong_projection_scope.tenant_context.actor_id = Some("bob".to_string());
        assert!(!context_run_visible(&state, &wrong_projection_scope, &bob, None, false).await);

        let mut orphaned = projection.clone();
        orphaned.run_id = "automation-v2-missing-native-run".to_string();
        assert!(!context_run_visible(&state, &orphaned, &alice, None, false).await);

        let mut relabelled = projection.clone();
        relabelled.run_type = "interactive".to_string();
        assert!(!context_run_visible(&state, &relabelled, &alice, None, false).await);

        let mut wrong_spec_scope = spec.clone();
        wrong_spec_scope.set_tenant_context(&other_tenant);
        state
            .automations_v2
            .write()
            .await
            .insert(spec.automation_id.clone(), wrong_spec_scope);
        assert!(!context_run_visible(&state, &projection, &alice, None, false).await);
    }
}
