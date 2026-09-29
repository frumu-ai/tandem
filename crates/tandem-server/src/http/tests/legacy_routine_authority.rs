// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::{extract::Extension, http::StatusCode, Router};
use serde_json::{json, Value};
use tandem_types::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContext, TenantContextAssertionClaims,
    VerifiedTenantContext,
};

pub(super) fn tenant(actor: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(
        "org-routine",
        "dep-routine",
        Some("dep-routine".into()),
        actor,
    )
}

pub(super) fn verified(actor: &str, role: &str) -> VerifiedTenantContext {
    let now = crate::now_ms();
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        Uuid::new_v4().to_string(),
        tenant(actor),
        HumanActor::tandem_user(actor),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user(actor, "tandem-web")),
        vec![format!("hosted:role:{role}")],
    );
    claims.policy_version = Some(1);
    claims.capabilities = tandem_enterprise_contract::hosted_policy::role_capabilities(role)
        .iter()
        .map(|capability| capability.to_string())
        .collect();
    claims.into()
}

pub(super) async fn hosted_state() -> (AppState, tempfile::TempDir) {
    let state = test_state().await;
    let temp = tempfile::tempdir().expect("policy directory");
    let path = temp.path().join("policy.json");
    let users: Vec<_> = [("alice", "member"), ("bob", "member"), ("admin", "admin")]
        .into_iter()
        .map(|(actor, role)| {
            json!({
                "id": actor, "email": null, "username": null, "role": role,
                "capabilities": tandem_enterprise_contract::hosted_policy::role_capabilities(role),
                "is_active": true, "email_verified": true
            })
        })
        .collect();
    let grants: Vec<_> = ["alice", "bob"]
        .into_iter()
        .map(|actor| {
            json!({
                "id": format!("routine-writer-{actor}"), "deployment_id": "dep-routine",
                "principal_kind": "member", "principal_id": actor,
                "resource_kind": "deployment", "resource_id": "dep-routine",
                "permissions": ["automation.write"]
            })
        })
        .collect();
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema_version": 1, "policy_version": 1,
            "organization_id": "org-routine", "deployment_id": "dep-routine",
            "generated_at": chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).unwrap(),
            "users": users, "org_units": [], "org_unit_memberships": [],
            "deployment_grants": grants
        }))
        .expect("policy JSON"),
    )
    .expect("policy file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("private policy permissions");
    }
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-routine", "dep-routine", path);
    state
        .reload_hosted_policy()
        .await
        .expect("load hosted policy");
    (state, temp)
}

fn app(state: &AppState, actor: &str, role: &str) -> Router {
    let mut identity = verified(actor, role);
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .expect("project current hosted actor");
    crate::http::routes_routines_automations::apply(Router::new())
        .layer(Extension(tenant(actor)))
        .layer(Extension(RequestPrincipal::authenticated_user(
            actor,
            "tandem-web",
        )))
        .layer(Extension(identity))
        .with_state(state.clone())
}

fn owned_routine() -> crate::routines::types::RoutineSpec {
    serde_json::from_value(json!({
        "routine_id": "alice-routine", "tenant_context": tenant("alice"),
        "name": "Original", "status": "active",
        "schedule": {"interval_seconds": {"seconds": 60}}, "timezone": "UTC",
        "misfire_policy": {"type": "run_once"}, "entrypoint": "mission.default",
        "args": {}, "allowed_tools": [], "output_targets": [],
        "creator_type": "user", "creator_id": "alice",
        "requires_approval": true, "external_integrations_allowed": false
    }))
    .expect("owned routine fixture")
}

async fn request(app: Router, method: &str, path: &str, body: Value) -> StatusCode {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("legacy routine request"),
        )
        .await
        .expect("legacy routine response");
    response.status()
}

#[tokio::test]
async fn legacy_routine_owner_guards_both_write_aliases() {
    let (state, _policy) = hosted_state().await;
    for (actor, role, expected) in [
        ("alice", "member", StatusCode::OK),
        ("admin", "admin", StatusCode::OK),
        ("bob", "member", StatusCode::FORBIDDEN),
    ] {
        for prefix in ["/routines", "/automations"] {
            for method in ["PATCH", "DELETE"] {
                let original = state
                    .put_routine(owned_routine())
                    .await
                    .expect("seed routine");
                let before = std::fs::read(&state.routines_path).expect("persisted routine");
                let status = request(
                    app(&state, actor, role),
                    method,
                    &format!("{prefix}/alice-routine"),
                    json!({"name": "Changed"}),
                )
                .await;
                assert_eq!(status, expected, "{actor} {method} {prefix}");
                let stored = state
                    .get_routine_for_tenant("alice-routine", &tenant("alice"))
                    .await;
                if actor == "bob" {
                    assert_eq!(
                        serde_json::to_value(stored.unwrap()).unwrap(),
                        serde_json::to_value(original).unwrap()
                    );
                    assert_eq!(std::fs::read(&state.routines_path).unwrap(), before);
                } else if method == "DELETE" {
                    assert!(stored.is_none());
                } else {
                    assert_eq!(stored.unwrap().name, "Changed");
                }
            }
        }
    }
}

#[tokio::test]
async fn legacy_routine_create_cannot_replace_another_owner_or_spoof_creator() {
    let (state, _policy) = hosted_state().await;
    for prefix in ["/routines", "/automations"] {
        let original = state
            .put_routine(owned_routine())
            .await
            .expect("seed routine");
        let body = if prefix == "/routines" {
            json!({
                "routine_id": "alice-routine", "name": "Replacement",
                "schedule": {"interval_seconds": {"seconds": 60}},
                "entrypoint": "mission.default", "creator_id": "alice"
            })
        } else {
            json!({
                "automation_id": "alice-routine", "name": "Replacement",
                "schedule": {"interval_seconds": {"seconds": 60}},
                "mission": {"objective": "Replace routine"}, "creator_id": "alice"
            })
        };
        let before = std::fs::read(&state.routines_path).expect("persisted routine");
        assert_eq!(
            request(app(&state, "bob", "member"), "POST", prefix, body.clone()).await,
            StatusCode::FORBIDDEN,
            "nonowner create must not overwrite {prefix}",
        );
        assert_eq!(
            serde_json::to_value(
                state
                    .get_routine_for_tenant("alice-routine", &tenant("alice"))
                    .await
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(original).unwrap(),
        );
        assert_eq!(std::fs::read(&state.routines_path).unwrap(), before);

        assert_eq!(
            request(app(&state, "admin", "admin"), "POST", prefix, body).await,
            StatusCode::OK,
            "admin may replace {prefix}",
        );
        let stored = state
            .get_routine_for_tenant("alice-routine", &tenant("alice"))
            .await
            .expect("admin update retained original owner");
        assert_eq!(stored.tenant_context.actor_id.as_deref(), Some("alice"));
        assert_eq!(stored.creator_id, "alice");
    }

    let status = request(
        app(&state, "bob", "member"),
        "POST",
        "/automations",
        json!({
            "automation_id": "bob-new", "name": "New",
            "schedule": {"interval_seconds": {"seconds": 60}},
            "mission": {"objective": "Own routine"}, "creator_id": "alice"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let created = state
        .get_routine_for_tenant("bob-new", &tenant("bob"))
        .await
        .expect("new routine");
    assert_eq!(created.creator_id, "bob");
    assert_eq!(created.tenant_context.actor_id.as_deref(), Some("bob"));
}

#[tokio::test]
async fn hosted_pack_builder_cannot_reach_unscoped_routine_and_pack_stores() {
    let (state, _policy) = hosted_state().await;
    state
        .runtime
        .get()
        .expect("runtime")
        .permissions
        .add_rule(
            "pack_builder",
            "pack_builder",
            tandem_core::PermissionAction::Allow,
        )
        .await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            std::sync::Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;

    let mut original = owned_routine();
    original.routine_id = "tpk_pack_builder_alpha.default".to_string();
    let original = state.put_routine(original).await.unwrap();
    let before = std::fs::read(&state.routines_path).unwrap();
    let mut identity = verified("bob", "member");
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .unwrap();
    let app =
        crate::http::routes_global::apply(crate::http::routes_pack_builder::apply(Router::new()))
            .layer(Extension(tenant("bob")))
            .layer(Extension(identity))
            .with_state(state.clone());

    for (path, body) in [
        (
            "/tool/execute",
            json!({"tool": "pack_builder", "args": {"mode": "preview", "goal": "alpha", "auto_apply": true}}),
        ),
        (
            "/tool/execute",
            json!({"tool": "pack_builder", "args": {
                "mode": "apply", "plan_id": "caller-chosen", "__session_id": "alice-session",
                "approve_pack_install": true
            }}),
        ),
        (
            "/pack-builder/preview",
            json!({"goal": "alpha", "auto_apply": true}),
        ),
        (
            "/pack-builder/apply",
            json!({"plan_id": "caller-chosen", "approvals": {"approve_pack_install": true}}),
        ),
    ] {
        assert_eq!(
            request(app.clone(), "POST", path, body).await,
            StatusCode::FORBIDDEN,
            "hosted pack-builder entrypoint {path} must fail before side effects",
        );
        assert_eq!(std::fs::read(&state.routines_path).unwrap(), before);
        assert_eq!(
            serde_json::to_value(
                state
                    .get_routine_for_tenant(&original.routine_id, &tenant("alice"))
                    .await
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&original).unwrap(),
        );
        assert!(state
            .get_automation_v2("automation.tpk_pack_builder_alpha.default")
            .await
            .is_none());
    }
}
