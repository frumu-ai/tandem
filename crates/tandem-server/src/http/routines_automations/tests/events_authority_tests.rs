// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::{body::Body, http::Request, routing::get, Router};
use tandem_types::{AuthorityChain, HumanActor, TenantContextAssertionClaims};
use tower::ServiceExt;

#[tokio::test]
async fn automation_events_close_after_policy_revision_even_without_events() {
    for route in ["/v2", "/legacy", "/routines"] {
        for (revoke_before_ready, queue_event) in [(false, false), (false, true), (true, false)] {
            let state = crate::test_support::test_state().await;
            let now = crate::now_ms();
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("policy.json");
            let mut policy = json!({
                "schema_version": 1, "policy_version": 1,
                "organization_id": "org-a", "deployment_id": "dep-a",
                "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
                "users": [{"id": "alice", "email": null, "username": null,
                    "role": "member", "capabilities": ["automation.read"],
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
            let tenant = TenantContext::explicit_user_workspace(
                "org-a",
                "dep-a",
                Some("dep-a".into()),
                "alice",
            );
            let mut claims = TenantContextAssertionClaims::new_v1(
                "tandem-web",
                "tandem-runtime",
                now,
                now + 300_000,
                "events-assertion",
                tenant.clone(),
                HumanActor::tandem_user("alice"),
                AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                    "alice",
                    "tandem-web",
                )),
                vec!["hosted:role:member".into()],
            );
            claims.policy_version = Some(1);
            claims.capabilities = vec!["automation.read".into()];
            let verified: VerifiedTenantContext = claims.into();
            state
                .enterprise
                .hosted_policy
                .authorize_permission(
                    Some(&verified),
                    tandem_types::AccessPermission::HostedAutomationRead,
                )
                .unwrap();
            let app = Router::new()
                .route("/v2", get(automations_v2_events))
                .route("/legacy", get(automations_events))
                .route("/routines", get(routines_events))
                .layer(Extension(tenant.clone()))
                .layer(Extension(verified))
                .with_state(state.clone());
            let uri = format!("{route}?automation_id=auto-a&routine_id=auto-a&run_id=run-a");
            let response = app
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let mut body = response.into_body().into_data_stream();
            if !revoke_before_ready {
                let ready = body.next().await.unwrap().unwrap();
                assert!(String::from_utf8_lossy(&ready).contains("ready"));
                let event_type = if route == "/v2" {
                    "automation.v2.run.started"
                } else {
                    "routine.run.created"
                };
                let publish = |tenant: &TenantContext,
                               kind: &str,
                               automation: &str,
                               run: &str,
                               marker: &str| {
                    state.event_bus.publish(crate::routines::types::tenant_scoped_engine_event(
                    kind, tenant, json!({"automationID": automation, "routineID": automation, "runID": run, "marker": marker}),
                ));
                };
                let foreign = TenantContext::explicit_user_workspace(
                    "org-b",
                    "dep-b",
                    Some("dep-b".into()),
                    "bob",
                );
                publish(&foreign, event_type, "auto-a", "run-a", "foreign-event");
                publish(&tenant, "unrelated.event", "auto-a", "run-a", "wrong-type");
                publish(&tenant, event_type, "auto-b", "run-a", "wrong-automation");
                if route != "/routines" {
                    publish(&tenant, event_type, "auto-a", "run-b", "wrong-run");
                }
                publish(&tenant, event_type, "auto-a", "run-a", "authorized-event");
                let event = tokio::time::timeout(Duration::from_secs(2), body.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert!(String::from_utf8_lossy(&event).contains("authorized-event"));
                if queue_event {
                    publish(
                        &tenant,
                        event_type,
                        "auto-a",
                        "run-a",
                        "must-not-be-delivered",
                    );
                }
            }
            let next = body.next();
            tokio::pin!(next);
            // Poll while authority is still valid to exercise revocation during an
            // outstanding read, not merely the next call's entry check.
            if !revoke_before_ready && !queue_event {
                assert!(futures::poll!(&mut next).is_pending());
            }
            policy["policy_version"] = json!(2);
            policy["users"][0]["capabilities"] = json!([]);
            std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
            state.reload_hosted_policy().await.unwrap();
            let next = tokio::time::timeout(Duration::from_secs(2), next)
                .await
                .expect("revoked automation stream must close even when idle");
            assert!(
                next.is_none(),
                "revoked stream must not emit a ready or event frame"
            );
        }
    }
}

#[tokio::test]
async fn automation_event_guard_rejects_unverified_or_mismatched_identity() {
    let state = crate::test_support::test_state().await;
    let now = crate::now_ms();
    let tenant =
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice");
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 300_000,
        "events-assertion",
        tenant.clone(),
        HumanActor::tandem_user("alice"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("alice", "tandem-web")),
        vec![],
    );
    claims.tenant_context.actor_id = Some("bob".into());
    let verified: VerifiedTenantContext = claims.into();
    let stream = guard_automation_events(
        tokio_stream::once(Ok(Event::default().data("must-not-be-delivered"))),
        state.clone(),
        tenant.clone(),
        Some(verified),
    );
    tokio::pin!(stream);
    assert!(stream.next().await.is_none());

    let temp = tempfile::tempdir().unwrap();
    state.enterprise.hosted_policy.configure_test_source(
        "org-a",
        "dep-a",
        temp.path().join("missing.json"),
    );
    let stream = guard_automation_events(
        tokio_stream::once(Ok(Event::default().data("must-not-be-delivered"))),
        state,
        tenant,
        None,
    );
    tokio::pin!(stream);
    assert!(stream.next().await.is_none());
}
