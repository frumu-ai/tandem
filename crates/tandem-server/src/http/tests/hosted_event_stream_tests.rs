// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::{body::Body, http::Request, routing::get, Router};
use futures::StreamExt;
use tandem_types::{AuthorityChain, HumanActor, TenantContextAssertionClaims};
use tower::ServiceExt;

#[path = "hosted_event_stream_tests/buffered_context.rs"]
mod buffered_context;

fn stream_tenant(actor: &str) -> TenantContext {
    TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), actor)
}

struct StreamFixture {
    state: AppState,
    _temp: tempfile::TempDir,
    path: std::path::PathBuf,
    policy: Value,
    verified: tandem_types::VerifiedTenantContext,
}

impl StreamFixture {
    async fn new(capabilities: &[&str]) -> Self {
        let state = crate::test_support::test_state().await;
        let now = crate::now_ms();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("policy.json");
        let role = if capabilities.contains(&"hosted.admin") {
            "admin"
        } else {
            "member"
        };
        let policy = json!({
            "schema_version": 1, "policy_version": 1,
            "organization_id": "org-a", "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [{"id": "alice", "email": null, "username": null,
                "role": role, "capabilities": capabilities,
                "is_active": true, "email_verified": true}],
            "org_units": [], "org_unit_memberships": [], "deployment_grants": []
        });
        std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        state
            .enterprise
            .hosted_policy
            .configure_test_source("org-a", "dep-a", path.clone());
        state.reload_hosted_policy().await.unwrap();
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 300_000,
            "stream-test",
            stream_tenant("alice"),
            HumanActor::tandem_user("alice"),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                "alice",
                "tandem-web",
            )),
            vec![format!("hosted:role:{role}")],
        );
        claims.policy_version = Some(1);
        claims.capabilities = capabilities.iter().map(|value| (*value).into()).collect();
        Self {
            state,
            _temp: temp,
            path,
            policy,
            verified: claims.into(),
        }
    }

    fn router(&self) -> Router {
        super::routes_context::apply(Router::new(), self.state.clone())
            .route("/event", get(global::events))
            .route("/global/event", get(global::events))
            .route("/run/{id}/events", get(global::run_events))
            .route("/api/run/{id}/events", get(global::run_events))
            .route("/workflows/events", get(workflows::workflow_events))
            .layer(Extension(stream_tenant("alice")))
            .layer(Extension(self.verified.clone()))
            .with_state(self.state.clone())
    }

    async fn context_run(&self, id: &str, owner: &str, run_type: &str) {
        let workspace = tandem_core::normalize_workspace_path(
            &self.state.workspace_index.snapshot().await.root,
        )
        .unwrap();
        let run = serde_json::from_value(json!({
            "run_id": id,
            "run_type": run_type,
            "tenant_context": stream_tenant(owner),
            "status": "queued",
            "objective": id,
            "workspace": {"workspace_id":"", "canonical_path":workspace, "lease_epoch":0},
            "revision": 1,
            "created_at_ms": 1,
            "updated_at_ms": 1
        }))
        .unwrap();
        super::context_runs::save_context_run_state(&self.state, &run)
            .await
            .unwrap();
    }

    async fn revoke(&mut self) {
        self.policy["policy_version"] = json!(self.policy["policy_version"].as_u64().unwrap() + 1);
        self.policy["users"][0]["capabilities"] = json!([]);
        std::fs::write(&self.path, serde_json::to_vec(&self.policy).unwrap()).unwrap();
        self.state.reload_hosted_policy().await.unwrap();
    }

    async fn workflow(&self, id: &str, owner: &str) {
        let record = serde_json::from_value(json!({
            "run_id": id, "workflow_id": "workflow", "tenant_context": stream_tenant(owner),
            "status": "running", "created_at_ms": 1, "updated_at_ms": 1
        }))
        .unwrap();
        self.state.put_workflow_run(record).await.unwrap();
    }

    async fn automation(&self, id: &str, owner: &str, visibility: &str) {
        let mut spec = crate::AutomationV2Spec {
            automation_id: id.into(),
            name: id.into(),
            description: None,
            status: crate::AutomationV2Status::Paused,
            schedule: crate::AutomationV2Schedule {
                schedule_type: crate::AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".into(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            agents: Vec::new(),
            flow: crate::AutomationFlowSpec { nodes: Vec::new() },
            execution: crate::AutomationExecutionPolicy::default(),
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            creator_id: owner.into(),
            workspace_root: None,
            metadata: Some(json!({"resource_access": {"visibility": visibility,
                "owner_principal": {"kind":"human_user", "id":owner}, "audience_principals":["eng"]}})),
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        };
        spec.set_tenant_context(&stream_tenant(owner));
        self.state.put_automation_v2(spec).await.unwrap();
    }

    fn publish(&self, kind: &str, actor: &str, mut properties: Value) {
        properties["tenantContext"] = json!(stream_tenant(actor));
        self.state
            .event_bus
            .publish(EngineEvent::new(kind, properties));
    }

    async fn automation_run_and_goal(&self, id: &str, owner: &str) -> Value {
        self.automation(id, owner, "private").await;
        let run = json!({
            "run_id": id, "automation_id": id, "tenant_context": stream_tenant(owner),
            "trigger_type": "manual", "status": "running", "created_at_ms": 1,
            "updated_at_ms": 1, "checkpoint": {}
        });
        self.state
            .automation_v2_runs
            .write()
            .await
            .insert(id.into(), serde_json::from_value(run.clone()).unwrap());
        let goal = serde_json::from_value(json!({
            "goal_id": id, "orchestration_id": "orchestration", "orchestration_version": 1,
            "objective": "test", "status": "active", "tenant_context": stream_tenant(owner),
            "policy": tandem_automation::GoalPolicy::default(), "active_run_id": id,
            "created_at_ms": 1, "updated_at_ms": 1
        }))
        .unwrap();
        crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
            &self.state.automation_v2_runs_path,
        )
        .unwrap()
        .put_goal(&goal)
        .unwrap();
        run
    }
}

#[tokio::test]
async fn run_event_stream_aliases_reject_another_actors_active_session() {
    for route in ["/run", "/api/run"] {
        let fixture = StreamFixture::new(&[]).await;
        for (actor, run_id) in [("alice", "own-run"), ("bob", "other-run")] {
            let mut session = tandem_types::Session::new(None, None);
            session.tenant_context = stream_tenant(actor);
            let session_id = session.id.clone();
            fixture.state.storage.save_session(session).await.unwrap();
            fixture
                .state
                .run_registry
                .acquire(&session_id, run_id.into(), None, None, None)
                .await
                .unwrap();
        }

        let denied = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("{route}/other-run/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::NOT_FOUND, "{route}");

        let allowed = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("{route}/own-run/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK, "{route}");
        let mut body = allowed.into_body().into_data_stream();
        let connected = body.next().await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&connected).contains("run.stream.connected"));
        fixture.publish(
            "session.run.started",
            "alice",
            json!({"runID":"own-run", "marker":"owner-event"}),
        );
        let next = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&next).contains("owner-event"));

        let missing = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("{route}/missing-run/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND, "{route}");
    }
}

#[tokio::test]
async fn run_event_stream_aliases_keep_standalone_local_sessions() {
    for route in ["/run", "/api/run"] {
        let state = crate::test_support::test_state().await;
        let session = tandem_types::Session::new(None, None);
        let session_id = session.id.clone();
        state.storage.save_session(session).await.unwrap();
        state
            .run_registry
            .acquire(&session_id, "local-run".into(), None, None, None)
            .await
            .unwrap();
        let app = Router::new()
            .route("/run/{id}/events", get(global::run_events))
            .route("/api/run/{id}/events", get(global::run_events))
            .layer(Extension(TenantContext::local_implicit()))
            .with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("{route}/local-run/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{route}");
        let mut body = response.into_body().into_data_stream();
        body.next().await.unwrap().unwrap();
        state.event_bus.publish(EngineEvent::new(
            "session.run.started",
            json!({"runID":"local-run", "marker":"standalone-event"}),
        ));
        let event = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&event).contains("standalone-event"));
    }
}

#[tokio::test]
async fn context_run_reads_and_rollback_do_not_cross_actors() {
    let fixture = StreamFixture::new(&[]).await;
    fixture
        .context_run("own-context", "alice", "interactive")
        .await;
    fixture
        .context_run("other-context", "bob", "interactive")
        .await;

    for path in [
        "/run/other-context/events",
        "/api/run/other-context/events",
        "/context/runs/other-context",
        "/context/runs/other-context/events",
        "/context/runs/other-context/events/stream",
        "/context/runs/other-context/checkpoints/mutations",
    ] {
        let response = fixture
            .router()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let rollback = fixture
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/context/runs/other-context/checkpoints/mutations/rollback-execute")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"confirm":"ROLLBACK"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rollback.status(), StatusCode::NOT_FOUND);
    let owner_without_admin = fixture
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/context/runs/own-context/checkpoints/mutations/rollback-execute")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"confirm":"ROLLBACK"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(owner_without_admin.status(), StatusCode::NOT_FOUND);

    for path in ["/run/own-context/events", "/context/runs/own-context"] {
        let response = fixture
            .router()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
    let listed = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri("/context/runs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let body = axum::body::to_bytes(listed.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("own-context"));
    assert!(!text.contains("other-context"));

    let workspace = fixture.state.workspace_index.snapshot().await.root;
    let uri = format!(
        "/context/runs/events/stream?workspace={}",
        urlencoding::encode(&workspace)
    );
    let multiplex = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri(uri.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(multiplex.status(), StatusCode::OK);
    let mut body = multiplex.into_body().into_data_stream();
    let ready = body.next().await.unwrap().unwrap();
    let ready = String::from_utf8_lossy(&ready);
    assert!(ready.contains("own-context"));
    assert!(!ready.contains("other-context"));

    let explicit = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri(format!("{uri}&run_ids=other-context"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(explicit.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn context_run_create_cannot_claim_managed_projection_identity() {
    let fixture = StreamFixture::new(&[]).await;
    for (run_id, run_type) in [
        ("session-forged", "interactive"),
        ("custom-forged", "session"),
        ("automation-v2-forged", "automation_v2"),
        ("x/../workflow-forged", "interactive"),
        ("x\\..\\workflow-forged", "interactive"),
        ("../forged", "interactive"),
        (".", "interactive"),
    ] {
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/context/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"run_id":run_id, "run_type":run_type, "objective":"forged"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{run_id}");
    }
    assert!(
        !super::context_runs::context_run_state_path(&fixture.state, "workflow-forged").exists()
    );
}

#[tokio::test]
async fn context_run_loader_rejects_mismatched_stored_identity() {
    let fixture = StreamFixture::new(&["workflow.read"]).await;
    fixture
        .context_run("workflow-stored-id", "alice", "workflow")
        .await;
    let path = super::context_runs::context_run_state_path(&fixture.state, "workflow-stored-id");
    let mut stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    stored["run_id"] = json!("interactive-forged");
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert!(matches!(
        super::context_runs::load_context_run_state(&fixture.state, "workflow-stored-id").await,
        Err(StatusCode::NOT_FOUND)
    ));
    let response = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri("/context/runs/workflow-stored-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hosted_routine_projections_rebind_legacy_tenant_to_canonical_run() {
    let fixture = StreamFixture::new(&["automation.read"]).await;
    let canonical: crate::RoutineRunRecord = serde_json::from_value(json!({
        "run_id":"hosted-routine-run", "routine_id":"routine", "tenant_context":stream_tenant("alice"),
        "trigger_type":"manual", "run_count":1, "status":"running", "created_at_ms":1,
        "updated_at_ms":1, "requires_approval":false, "entrypoint":"main"
    }))
    .unwrap();
    fixture
        .state
        .routine_runs
        .write()
        .await
        .insert(canonical.run_id.clone(), canonical.clone());
    let context_id = super::context_runs::sync_routine_run_blackboard(&fixture.state, &canonical)
        .await
        .unwrap();
    let mut projection =
        super::context_runs::load_context_run_state_sync(&fixture.state, &context_id).unwrap();
    assert_eq!(projection.tenant_context, stream_tenant("alice"));

    // Simulate a projection written before the tenant binding was corrected.
    projection.tenant_context = TenantContext::local_implicit();
    super::context_runs::save_context_run_state_sync(&fixture.state, &projection).unwrap();
    for path in [
        format!("/context/runs/{context_id}"),
        format!("/run/{context_id}/events"),
    ] {
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(path.as_str())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
    let migrated =
        super::context_runs::load_context_run_state_sync(&fixture.state, &context_id).unwrap();
    assert_eq!(migrated.tenant_context, canonical.tenant_context);
}

#[tokio::test]
async fn run_stream_preserves_shared_automation_and_workflow_reviewer_reads() {
    let fixture = StreamFixture::new(&["automation.read", "workflow.read", "hosted.admin"]).await;
    fixture.automation_run_and_goal("shared-run", "bob").await;
    fixture.automation("shared-run", "bob", "org").await;
    fixture
        .context_run("automation-v2-shared-run", "bob", "automation_v2")
        .await;
    fixture.workflow("reviewed-run", "bob").await;
    fixture
        .context_run("workflow-reviewed-run", "bob", "workflow")
        .await;

    for id in ["automation-v2-shared-run", "workflow-reviewed-run"] {
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri(format!("/run/{id}/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{id}");
    }

    fixture
        .context_run("workflow-missing-canonical", "bob", "workflow")
        .await;
    let forged = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri("/run/workflow-missing-canonical/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn run_stream_rechecks_idle_policy_and_keeps_terminal_session_event() {
    let mut fixture = StreamFixture::new(&[]).await;
    let mut session = tandem_types::Session::new(None, None);
    session.tenant_context = stream_tenant("alice");
    let session_id = session.id.clone();
    fixture.state.storage.save_session(session).await.unwrap();
    fixture
        .state
        .run_registry
        .acquire(&session_id, "terminal-run".into(), None, None, None)
        .await
        .unwrap();
    let response = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri("/run/terminal-run/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    body.next().await.unwrap().unwrap();
    fixture
        .state
        .run_registry
        .finish_if_match(&session_id, "terminal-run")
        .await;
    fixture.publish(
        "session.run.finished",
        "alice",
        json!({"sessionID":session_id, "runID":"terminal-run", "marker":"terminal"}),
    );
    let event = tokio::time::timeout(Duration::from_secs(2), body.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&event).contains("terminal"));
    let next = body.next();
    tokio::pin!(next);
    assert!(futures::poll!(&mut next).is_pending());
    fixture.revoke().await;
    assert!(tokio::time::timeout(Duration::from_secs(2), next)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn global_stream_authorizes_orchestration_and_webhook_event_representations() {
    for route in ["/event", "/global/event"] {
        for can_read in [false, true] {
            let capabilities = if can_read {
                vec!["automation.read"]
            } else {
                vec![]
            };
            let fixture = StreamFixture::new(&capabilities).await;
            let own = fixture.automation_run_and_goal("own", "alice").await;
            let other = fixture.automation_run_and_goal("other", "bob").await;
            let mut session = tandem_types::Session::new(None, None);
            session.tenant_context = stream_tenant("alice");
            let session_id = session.id.clone();
            fixture.state.storage.save_session(session).await.unwrap();
            let response = fixture
                .router()
                .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let mut body = response.into_body().into_data_stream();
            for _ in 0..2 {
                body.next().await.unwrap().unwrap();
            }
            for (kind, own_properties, other_properties) in [
                (
                    "orchestration.goal.transitioned",
                    json!({"goalID":"own", "run":own}),
                    json!({"goalID":"other", "run":other}),
                ),
                (
                    "orchestration.goal.started",
                    json!({"goalID":"own", "rootRunID":"own"}),
                    json!({"goalID":"other", "rootRunID":"other"}),
                ),
                (
                    "orchestration.goal.paused",
                    json!({"goalID":"own"}),
                    json!({"goalID":"other"}),
                ),
                (
                    "orchestration.goal.resumed",
                    json!({"goalID":"own"}),
                    json!({"goalID":"other"}),
                ),
                (
                    "orchestration.goal.cancelled",
                    json!({"goalID":"own", "runID":"own"}),
                    json!({"goalID":"other", "runID":"other"}),
                ),
                (
                    "stateful_runtime.wait.webhook_woken",
                    json!({"runID":"own", "waitID":"wait"}),
                    json!({"runID":"other", "waitID":"wait"}),
                ),
                (
                    "stateful_runtime.wait.timer_woken",
                    json!({"runID":"own", "waitID":"wait"}),
                    json!({"runID":"other", "waitID":"wait"}),
                ),
                (
                    "stateful_runtime.wait.timeout_reminded",
                    json!({"runID":"own", "waitID":"wait"}),
                    json!({"runID":"other", "waitID":"wait"}),
                ),
            ] {
                // Same-actor event attribution must not authorize another
                // actor's private canonical automation.
                fixture.publish(kind, "alice", other_properties);
                fixture.publish(kind, "alice", own_properties);
                fixture.publish(
                    "message.part.updated",
                    "alice",
                    json!({
                        "part":{"sessionID":session_id, "text":"session-sentinel"}
                    }),
                );
                let next = tokio::time::timeout(Duration::from_secs(2), body.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let text = String::from_utf8_lossy(&next);
                if can_read {
                    assert!(
                        text.contains(kind) && !text.contains("other"),
                        "{route}: {text}"
                    );
                    let sentinel = body.next().await.unwrap().unwrap();
                    assert!(String::from_utf8_lossy(&sentinel).contains("session-sentinel"));
                } else {
                    assert!(
                        text.contains("session-sentinel"),
                        "{route} leaked {kind}: {text}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn global_event_aliases_require_resource_permissions_and_actor_visibility() {
    for route in ["/event", "/global/event"] {
        let fixture = StreamFixture::new(&[]).await;
        fixture.workflow("workflow-run", "alice").await;
        fixture.automation("automation", "alice", "private").await;
        let mut own = tandem_types::Session::new(Some("own".into()), Some(".".into()));
        own.tenant_context = stream_tenant("alice");
        let own_id = own.id.clone();
        fixture.state.storage.save_session(own).await.unwrap();
        let mut other = tandem_types::Session::new(Some("other".into()), Some(".".into()));
        other.tenant_context = stream_tenant("bob");
        let other_id = other.id.clone();
        fixture.state.storage.save_session(other).await.unwrap();
        let response = fixture
            .router()
            .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let mut body = response.into_body().into_data_stream();
        for _ in 0..2 {
            body.next().await.unwrap().unwrap();
        }
        fixture.publish(
            "workflow.run.started",
            "alice",
            json!({"runID":"workflow-run", "workflowID":"workflow", "marker":"denied-workflow"}),
        );
        fixture.publish("automation.v2.run.started", "alice", json!({"runID":"automation-run", "automationID":"automation", "marker":"denied-automation"}));
        fixture.publish(
            "message.part.updated",
            "bob",
            json!({"part":{"sessionID":other_id, "text":"denied-other-actor"}}),
        );
        fixture.publish(
            "message.part.updated",
            "alice",
            json!({"part":{"sessionID":own_id, "text":"authorized-session"}}),
        );
        let next = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&next).contains("authorized-session"),
            "unauthorized event escaped {route}: {}",
            String::from_utf8_lossy(&next)
        );
    }
}

#[tokio::test]
async fn workflow_event_stream_revalidates_ready_queued_and_idle_reads() {
    for (before_ready, queued) in [(true, false), (false, true), (false, false)] {
        let mut fixture = StreamFixture::new(&["workflow.read"]).await;
        fixture.workflow("run", "alice").await;
        let response = fixture
            .router()
            .oneshot(
                Request::builder()
                    .uri("/workflows/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut body = response.into_body().into_data_stream();
        if !before_ready {
            let ready = body.next().await.unwrap().unwrap();
            assert!(String::from_utf8_lossy(&ready).contains("ready"));
        }
        if queued {
            fixture.publish(
                "workflow.run.started",
                "alice",
                json!({"runID":"run", "workflowID":"workflow"}),
            );
        }
        let next = body.next();
        tokio::pin!(next);
        if !before_ready && !queued {
            assert!(futures::poll!(&mut next).is_pending());
        }
        fixture.revoke().await;
        assert!(
            tokio::time::timeout(Duration::from_secs(2), next)
                .await
                .expect("revoked idle workflow stream must close")
                .is_none(),
            "revoked workflow stream emitted a frame"
        );
    }
}

#[tokio::test]
async fn workflow_streams_keep_owner_and_current_reviewer_visibility() {
    for route in ["/event", "/global/event", "/workflows/events"] {
        for reviewer in [false, true] {
            let capabilities = if reviewer {
                vec!["workflow.read", "hosted.admin"]
            } else {
                vec!["workflow.read"]
            };
            let fixture = StreamFixture::new(&capabilities).await;
            fixture.workflow("own", "alice").await;
            fixture.workflow("other", "bob").await;
            let response = fixture
                .router()
                .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let mut body = response.into_body().into_data_stream();
            for _ in 0..if route == "/workflows/events" { 1 } else { 2 } {
                body.next().await.unwrap().unwrap();
            }
            fixture.publish(
                "workflow.run.started",
                "bob",
                json!({"workflowID":"workflow", "runID":"other", "marker":"reviewer-event"}),
            );
            fixture.publish(
                "workflow.run.started",
                "alice",
                json!({"workflowID":"wrong", "runID":"own", "marker":"wrong-binding"}),
            );
            fixture.publish(
                "workflow.run.started",
                "alice",
                json!({"workflowID":"workflow", "runID":"own", "marker":"owner-event"}),
            );
            let next = tokio::time::timeout(Duration::from_secs(2), body.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(String::from_utf8_lossy(&next).contains(if reviewer {
                "reviewer-event"
            } else {
                "owner-event"
            }));
        }
    }
}

#[tokio::test]
async fn global_stream_keeps_shared_automations_and_rechecks_membership() {
    let mut fixture = StreamFixture::new(&["automation.read"]).await;
    fixture.policy["policy_version"] = json!(2);
    fixture.policy["org_units"] = json!([{"id":"eng", "slug":"eng", "display_name":"Engineering", "kind":"department", "state":"active"}]);
    fixture.policy["org_unit_memberships"] = json!([{"unit_id":"eng", "user_id":"alice"}]);
    fixture.verified.policy_version = Some(2);
    fixture.verified.org_units = vec!["eng".into()];
    std::fs::write(&fixture.path, serde_json::to_vec(&fixture.policy).unwrap()).unwrap();
    fixture.state.reload_hosted_policy().await.unwrap();
    for (id, owner, visibility) in [
        ("private", "bob", "private"),
        ("own", "alice", "private"),
        ("org", "bob", "org"),
        ("group", "bob", "group"),
    ] {
        fixture.automation(id, owner, visibility).await;
    }
    let response = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri("/global/event")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body().into_data_stream();
    for _ in 0..2 {
        body.next().await.unwrap().unwrap();
    }
    fixture.publish(
        "automation.v2.updated",
        "bob",
        json!({"automationID":"private", "marker":"denied-private"}),
    );
    for (id, owner) in [("own", "alice"), ("org", "bob"), ("group", "bob")] {
        fixture.publish(
            "automation.v2.updated",
            owner,
            json!({"automationID":id, "marker":format!("allowed-{id}")}),
        );
        let next = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&next).contains(&format!("allowed-{id}")));
    }
    fixture.publish("context.task.created", "bob", json!({
        "automation_id":"org", "automationID":"org", "workflow_id":"org", "workflowID":"org",
        "run_id":"native-run", "runID":"native-run", "source":"automation_v2", "marker":"allowed-context-alias"
    }));
    let event = tokio::time::timeout(Duration::from_secs(2), body.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&event).contains("allowed-context-alias"));
    let next = body.next();
    tokio::pin!(next);
    assert!(futures::poll!(&mut next).is_pending());
    fixture.policy["policy_version"] = json!(3);
    fixture.policy["org_unit_memberships"] = json!([]);
    std::fs::write(&fixture.path, serde_json::to_vec(&fixture.policy).unwrap()).unwrap();
    fixture.state.reload_hosted_policy().await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), next)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn global_stream_checks_snake_case_approvals_and_conflicting_session_aliases() {
    let fixture = StreamFixture::new(&["workflow.read"]).await;
    fixture.workflow("own", "alice").await;
    fixture.workflow("other", "bob").await;
    let mut session = tandem_types::Session::new(None, None);
    session.tenant_context = stream_tenant("alice");
    let id = session.id.clone();
    fixture.state.storage.save_session(session).await.unwrap();
    let response = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri("/event")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body().into_data_stream();
    for _ in 0..2 {
        body.next().await.unwrap().unwrap();
    }
    fixture.publish(
        "stateful_runtime.wait.timer_woken",
        "alice",
        json!({"runID":"other", "marker":"denied-workflow-wait"}),
    );
    fixture.publish(
        "stateful_runtime.wait.timer_woken",
        "alice",
        json!({"runID":"own", "marker":"allowed-workflow-wait"}),
    );
    let next = tokio::time::timeout(Duration::from_secs(2), body.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&next).contains("allowed-workflow-wait"));
    fixture.publish(
        "approval.decision.recorded",
        "alice",
        json!({"workflow_id":"workflow", "run_id":"other", "marker":"denied-review"}),
    );
    fixture.publish(
        "message.part.updated",
        "alice",
        json!({"sessionID":id, "part":{"session_id":"different", "text":"denied-alias"}}),
    );
    fixture.publish(
        "approval.decision.recorded",
        "alice",
        json!({"workflow_id":"workflow", "run_id":"own", "marker":"allowed-approval"}),
    );
    let next = tokio::time::timeout(Duration::from_secs(2), body.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&next).contains("allowed-approval"));
}

#[tokio::test]
async fn event_streams_keep_standalone_signed_and_unsigned_compatibility() {
    for signed in [false, true] {
        let fixture = StreamFixture::new(&[]).await;
        let state = crate::test_support::test_state().await;
        let mut app = Router::new()
            .route("/event", get(global::events))
            .route("/workflows/events", get(workflows::workflow_events))
            .layer(Extension(stream_tenant("alice")));
        if signed {
            app = app.layer(Extension(fixture.verified.clone()));
        }
        let app = app.with_state(state.clone());
        for route in ["/event", "/workflows/events"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let mut body = response.into_body().into_data_stream();
            for _ in 0..if route == "/event" { 2 } else { 1 } {
                body.next().await.unwrap().unwrap();
            }
            state.event_bus.publish(EngineEvent::new("workflow.run.started", json!({"runID":"legacy", "workflowID":"legacy", "tenantContext":stream_tenant("bob"), "marker":"standalone-event"})));
            let next = tokio::time::timeout(Duration::from_secs(2), body.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(String::from_utf8_lossy(&next).contains("standalone-event"));
        }
    }
}

#[tokio::test]
async fn hosted_stream_denies_after_resource_lookup_wait() {
    for route in ["/event", "/workflows/events"] {
        let mut fixture = StreamFixture::new(&["workflow.read"]).await;
        fixture.workflow("own", "alice").await;
        let response = fixture
            .router()
            .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let mut body = response.into_body().into_data_stream();
        for _ in 0..if route == "/event" { 2 } else { 1 } {
            body.next().await.unwrap().unwrap();
        }
        let state = fixture.state.clone();
        let held = state.workflow_runs.write().await;
        fixture.publish(
            "workflow.run.started",
            "alice",
            json!({"runID":"own", "workflowID":"workflow"}),
        );
        let next = body.next();
        tokio::pin!(next);
        assert!(futures::poll!(&mut next).is_pending());
        fixture.revoke().await;
        drop(held);
        assert!(tokio::time::timeout(Duration::from_secs(2), next)
            .await
            .unwrap()
            .is_none());
        assert!(
            body.next().await.is_none(),
            "denied streams must stay closed"
        );
    }
}

#[tokio::test]
async fn hosted_streams_close_when_assertion_expires_while_idle() {
    for route in ["/event", "/global/event", "/workflows/events"] {
        let mut fixture = StreamFixture::new(&["workflow.read"]).await;
        fixture.verified.expires_at_ms = crate::now_ms() + 300;
        let response = fixture
            .router()
            .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let mut body = response.into_body().into_data_stream();
        for _ in 0..if route == "/workflows/events" { 1 } else { 2 } {
            body.next().await.unwrap().unwrap();
        }
        assert!(tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn global_stream_keeps_authorized_routine_deletion_tombstone() {
    let fixture = StreamFixture::new(&["automation.read"]).await;
    let response = fixture
        .router()
        .oneshot(
            Request::builder()
                .uri("/event")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body().into_data_stream();
    for _ in 0..2 {
        body.next().await.unwrap().unwrap();
    }
    fixture.publish(
        "routine.deleted",
        "bob",
        json!({"routineID":"already-removed"}),
    );
    let next = tokio::time::timeout(Duration::from_secs(2), body.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&next).contains("already-removed"));
}
