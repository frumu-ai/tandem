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
    let mut hosted_automation_source = None;
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
            if !tenant_matches(tenant, &spec.tenant_context()) {
                return false;
            }
            if current.is_some() {
                let readable =
                    super::automation_object_authority::can_read(state, tenant, verified, &spec);
                hosted_automation_source = Some(spec);
                readable
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
    if let Some(spec) = hosted_automation_source.as_ref() {
        return super::automation_object_authority::can_execute(state, tenant, verified, spec);
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

// Native discovery may await before publication locks are taken. It is never
// authority for the frame: every binding below is read again under its actual
// owning projection/history/native locks at the publication boundary.
enum ResolvedContextReadSource {
    Session(String),
    Automation {
        native_id: String,
        sources: crate::app::state::AutomationV2RunReadSources,
    },
    Workflow(String),
    Routine(String),
    Interactive,
}

struct ResolvedContextRead {
    run_id: String,
    run_type: String,
    source: ResolvedContextReadSource,
}

async fn discover_context_read(state: &AppState, run_id: &str) -> Option<()> {
    let run = super::context_runs::load_context_run_state(state, run_id)
        .await
        .ok()?;
    match run.run_type.as_str() {
        "session" => {
            let native_id = run.run_id.strip_prefix("session-")?;
            state.storage.get_session(native_id).await?;
        }
        "automation_v2" | "incident_monitor_triage" => {
            let native_id = run.run_id.strip_prefix("automation-v2-")?;
            state.get_automation_v2_run(native_id).await?;
        }
        "workflow" => {
            let native_id = run.run_id.strip_prefix("workflow-")?;
            state.get_workflow_run(native_id).await?;
        }
        "routine" => {
            let native_id = run.run_id.strip_prefix("routine-")?;
            state.get_routine_run(native_id).await?;
        }
        _ if reserved_projection_id(&run.run_id) => return None,
        _ => {}
    }
    Some(())
}

/// Revalidate the entire batch at one synchronous frame-construction boundary.
/// Locks are held through `publish`, including ready JSON and SSE conversion.
/// Ordinary contention is awaited, never interpreted as revoked authority.
pub(super) async fn with_current_context_run_reads<T>(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    run_ids: &[String],
    publish: impl FnOnce(Vec<String>) -> T,
    #[cfg(test)] progress: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
) -> T {
    for run_id in run_ids {
        let _ = discover_context_read(state, run_id).await;
        #[cfg(test)]
        if let Some(progress) = progress {
            let _ = progress.send(run_id.clone());
        }
    }

    // Manual workflow creation holds publication through native creation
    // and projection sync. Publication therefore precedes engine locks.
    let _publication = state.enterprise.hosted_policy.lock_publication().await;

    // Projection ownership is mutable too. Take its existing per-run
    // engine locks in a stable order before native guards: context task
    // mutations already use projection -> native lookup.
    let mut projection_ids = run_ids.iter().map(String::as_str).collect::<Vec<_>>();
    projection_ids.sort_unstable();
    projection_ids.dedup();
    let mut projection_guards = Vec::new();
    for run_id in projection_ids {
        if let Ok(guard) = super::context_runs::context_run_projection_guard_for(run_id).await {
            projection_guards.push(guard);
        }
    }

    // Repair pending payload.run replacements and legacy routine owners
    // while holding the same engine tokens used by all four commits and
    // ordinary snapshot saves. Never re-enter the engine lock for repair.
    let mut projections = Vec::new();
    // Lock ordering must not reorder the Ready frame's subscriptions.
    for run_id in run_ids {
        let Some(guard) = projection_guards
            .iter()
            .find(|guard| guard.run_id() == run_id)
        else {
            continue;
        };
        if let Ok(run) =
            super::context_runs::load_context_run_state_with_projection_guard(state, guard).await
        {
            projections.push(run);
        }
    }

    // History precedes native maps: native persistence releases those
    // maps before awaiting its history writer. Retain the path-bound
    // history read guard through frame construction, and finish fresh
    // recovery reads before taking native guards. A queued writer cannot
    // force a re-lock or turn ordinary contention into denial.
    let history_guard =
        crate::app::state::automation_v2_run_history_read_guard(&state.automation_v2_runs_path)
            .await;
    let mut resolved = Vec::new();
    for run in projections {
        let source = match run.run_type.as_str() {
            "session" => run
                .run_id
                .strip_prefix("session-")
                .map(|id| ResolvedContextReadSource::Session(id.to_owned())),
            "automation_v2" | "incident_monitor_triage" => {
                if let Some(id) = run.run_id.strip_prefix("automation-v2-") {
                    crate::app::state::load_automation_v2_run_read_sources(
                        state,
                        &history_guard,
                        id,
                    )
                    .await
                    .map(|sources| ResolvedContextReadSource::Automation {
                        native_id: id.to_owned(),
                        sources,
                    })
                } else {
                    None
                }
            }
            "workflow" => run
                .run_id
                .strip_prefix("workflow-")
                .map(|id| ResolvedContextReadSource::Workflow(id.to_owned())),
            "routine" => run
                .run_id
                .strip_prefix("routine-")
                .map(|id| ResolvedContextReadSource::Routine(id.to_owned())),
            _ if reserved_projection_id(&run.run_id) => None,
            _ => Some(ResolvedContextReadSource::Interactive),
        };
        if let Some(source) = source {
            resolved.push(ResolvedContextRead {
                run_id: run.run_id,
                run_type: run.run_type,
                source,
            });
        }
    }

    // Native persistence takes automation runs before automation specs.
    // Preserve that ordering even for readers: queued writers make a
    // read/read inversion capable of deadlocking with Tokio's RwLock.
    let automation_runs = state.automation_v2_runs.read().await;
    let workflow_runs = state.workflow_runs.read().await;
    let routine_runs = state.routine_runs.read().await;

    // Match the established external-commit authority lock order.
    let automations = state.automations_v2.read().await;
    let memberships = state.enterprise.org_unit_memberships.read().await;
    let access_grants = state.enterprise.org_unit_access_grants.read().await;
    let cross_tenant_grants = state.enterprise.cross_tenant_grants.read().await;

    let session_ids = resolved
        .iter()
        .filter_map(|resource| match &resource.source {
            ResolvedContextReadSource::Session(id) => Some(id.clone()),
            _ => None,
        })
        .collect();
    // Acquire SQLite last: ownership is mutable even in independent
    // Storage instances. An empty session batch opens no DB transaction.
    let session_owners = state.storage.session_owner_read_guard(session_ids).await;
    if let Err(error) = &session_owners {
        tracing::error!(%error, "failed to stabilize stream session ownership");
    }

    // Finish all potentially blocking projection reads before the first
    // current expiry/ACL decision. The engine guards retain their binding.
    let current_rows = resolved
        .iter()
        .filter_map(|resource| {
            super::context_runs::load_context_run_state_sync(state, &resource.run_id)
                .ok()
                .map(|run| (resource, run))
        })
        .collect::<Vec<_>>();

    // No awaits, filesystem reads or live store re-locking from here
    // through publication of the complete batch.
    let current_ids = current_rows
        .iter()
        .filter(|(resource, run)| {
            if run.run_type != resource.run_type || !tenant_matches(tenant, &run.tenant_context) {
                return false;
            }
            let owner = || {
                super::sessions_actor_scope::session_visible_to_actor(tenant, &run.tenant_context)
            };
            match &resource.source {
                ResolvedContextReadSource::Session(id) => session_owners
                    .as_ref()
                    .ok()
                    .and_then(|owners| owners.tenant_context(id))
                    .is_some_and(|native_tenant| {
                        *native_tenant == run.tenant_context
                            && current_context(state, tenant, verified, None).is_ok()
                            && super::sessions_actor_scope::session_visible_to_actor(
                                tenant,
                                native_tenant,
                            )
                    }),
                ResolvedContextReadSource::Automation { native_id, sources } => {
                    let Some(record) = crate::app::state::current_automation_v2_run_read_source(
                        native_id,
                        automation_runs.get(native_id),
                        sources,
                    ) else {
                        return false;
                    };
                    if record.run_id != *native_id || record.tenant_context != run.tenant_context {
                        return false;
                    }
                    let Some(spec) = automations
                        .get(&record.automation_id)
                        .or(record.automation_snapshot.as_ref())
                    else {
                        return false;
                    };
                    if !tenant_matches(tenant, &spec.tenant_context()) {
                        return false;
                    }
                    match current_context(
                        state,
                        tenant,
                        verified,
                        Some(AccessPermission::HostedAutomationRead),
                    ) {
                        Ok(Some(_)) => {
                            super::automation_object_authority::can_read_with_held_grants(
                                state,
                                tenant,
                                verified,
                                spec,
                                &memberships,
                                &access_grants,
                                &cross_tenant_grants,
                            )
                        }
                        Ok(None) => owner(),
                        Err(_) => false,
                    }
                }
                ResolvedContextReadSource::Workflow(id) => {
                    let Some(record) = workflow_runs.get(id) else {
                        return false;
                    };
                    if record.run_id != *id || record.tenant_context != run.tenant_context {
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
                        record,
                        tenant,
                        &RequestPrincipal::authenticated_user(actor, "context-run"),
                        current.as_ref(),
                    )
                }
                ResolvedContextReadSource::Routine(id) => {
                    routine_runs.get(id).is_some_and(|record| {
                        record.run_id == *id && record.tenant_context == run.tenant_context
                    }) && current_context(
                        state,
                        tenant,
                        verified,
                        Some(AccessPermission::HostedAutomationRead),
                    )
                    .is_ok()
                        && owner()
                }
                ResolvedContextReadSource::Interactive => {
                    !reserved_projection_id(&run.run_id)
                        && current_context(state, tenant, verified, None).is_ok()
                        && owner()
                }
            }
        })
        .map(|(resource, _)| resource.run_id.clone())
        .collect();
    publish(current_ids)
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
    use serde_json::json;

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

    #[tokio::test]
    async fn hosted_automation_projection_honors_current_exact_object_grants() {
        use tandem_types::{
            AccessPermission, AuthorityChain, DataClass, HumanActor, OrganizationUnitAccessGrant,
            RequestPrincipal, ResourceKind, ResourceRef, TenantContextAssertionClaims,
        };

        let state = crate::test_support::test_state().await;
        let alice =
            TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice");
        let bob =
            TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "bob");
        let mut spec = AutomationSpecBuilder::new("shared-context-run-source")
            .nodes(vec![test_automation_node("inspect", vec![], "triage", 0)])
            .build();
        spec.creator_id = "alice".into();
        spec.metadata = Some(json!({"resource_access": {
            "visibility": "private",
            "owner_principal": {"kind": "human_user", "id": "alice"}
        }}));
        spec.set_tenant_context(&alice);
        let spec = state.put_automation_v2(spec).await.unwrap();
        let native = state
            .create_automation_v2_run(&spec, "manual")
            .await
            .unwrap();
        let projection_id =
            super::super::context_runs::automation_v2_context_run_id(&native.run_id);
        let projection = super::super::context_runs::load_context_run_state(&state, &projection_id)
            .await
            .unwrap();

        let now = crate::now_ms();
        let bundle = tandem_enterprise_contract::hosted_policy::HostedPolicyBundle::from_json(
            serde_json::to_vec(&json!({
                "schema_version": 1, "policy_version": 1,
                "organization_id": "org-a", "deployment_id": "dep-a",
                "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
                "users": [
                    {"id": "alice", "email": null, "username": null, "role": "member", "capabilities": [], "is_active": true, "email_verified": true},
                    {"id": "bob", "email": null, "username": null, "role": "member", "capabilities": ["automation.read", "automation.execute"], "is_active": true, "email_verified": true}
                ],
                "org_units": [{"id": "eng", "slug": "eng", "display_name": "Engineering", "kind": "department", "state": "active"}],
                "org_unit_memberships": [{"unit_id": "eng", "user_id": "bob"}],
                "deployment_grants": []
            }))
            .unwrap()
            .as_slice(),
        )
        .unwrap();
        state
            .enterprise
            .hosted_policy
            .install_test_bundle(bundle)
            .unwrap();
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 60_000,
            "context-run-bob",
            bob.clone(),
            HumanActor::tandem_user("bob"),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user("bob", "tandem-web")),
            vec!["hosted:role:member".into()],
        );
        claims.policy_version = Some(1);
        claims.capabilities = vec!["automation.read".into(), "automation.execute".into()];
        claims.org_units = vec!["eng".into()];
        let verified = claims.into();
        let grant = OrganizationUnitAccessGrant::active(
            "bob-context-run-source",
            bob.clone(),
            tandem_enterprise_contract::hosted_policy::hosted_unit_principal("eng"),
            ResourceRef::new(
                "org-a",
                "dep-a",
                ResourceKind::Automation,
                &spec.automation_id,
            ),
            now,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::Internal]);
        state
            .enterprise
            .org_unit_access_grants
            .write()
            .await
            .insert("bob-context-run-source".into(), grant.clone());

        assert!(context_run_visible(&state, &projection, &bob, Some(&verified), false).await);
        assert!(!context_run_visible(&state, &projection, &bob, Some(&verified), true).await);
        let execute_grant =
            grant.with_permissions(vec![AccessPermission::Read, AccessPermission::Execute]);
        state
            .enterprise
            .org_unit_access_grants
            .write()
            .await
            .insert("bob-context-run-source".into(), execute_grant);
        assert!(context_run_visible(&state, &projection, &bob, Some(&verified), true).await);

        state
            .enterprise
            .org_unit_access_grants
            .write()
            .await
            .remove("bob-context-run-source");
        assert!(!context_run_visible(&state, &projection, &bob, Some(&verified), false).await);
        assert!(!context_run_visible(&state, &projection, &bob, Some(&verified), true).await);
        let wrong_tenant = TenantContext::explicit_user_workspace(
            "other-org",
            "dep-a",
            Some("dep-a".into()),
            "bob",
        );
        assert!(
            !context_run_visible(&state, &projection, &wrong_tenant, Some(&verified), false).await
        );
    }
}
