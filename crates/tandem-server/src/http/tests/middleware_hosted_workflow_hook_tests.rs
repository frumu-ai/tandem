// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::{body::Body, Router};
use tandem_types::{AuthorityChain, HumanActor};
use tower::ServiceExt;

fn claims(actor: &str, role: &str, now: u64) -> TenantContextAssertionClaims {
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        format!("workflow-hook-{actor}-{role}"),
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), actor),
        HumanActor::tandem_user(actor),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user(actor, "tandem-web")),
        vec![format!("hosted:role:{role}")],
    );
    claims.policy_version = Some(1);
    claims.capabilities = tandem_enterprise_contract::hosted_policy::role_capabilities(role)
        .iter()
        .map(|cap| cap.to_string())
        .collect();
    claims
}

fn write_policy(path: &std::path::Path, now: u64) {
    let users = [("alice", "admin"), ("bob", "member")]
        .into_iter()
        .map(|(id, role)| {
            json!({
                "id": id, "email": null, "username": null, "role": role,
                "capabilities": tandem_enterprise_contract::hosted_policy::role_capabilities(role),
                "is_active": true, "email_verified": true
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        path,
        serde_json::to_vec(&json!({
            "schema_version": 1, "policy_version": 1,
            "organization_id": "org-a", "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": users, "org_units": [], "org_unit_memberships": [],
            "deployment_grants": [{
                "id": "bob-workflow-share", "deployment_id": "dep-a",
                "principal_kind": "member", "principal_id": "bob",
                "resource_kind": "deployment", "resource_id": "dep-a",
                "permissions": ["workflow.share"]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

async fn ingress(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    match attach_enterprise_request_context_for_mode(
        &state,
        &mut request,
        RuntimeAuthMode::HostedSingleTenant,
    )
    .await
    {
        Ok(true) => next.run(request).await,
        Ok(false) => StatusCode::FORBIDDEN.into_response(),
        Err(error) => panic!("required test denial receipt failed: {error}"),
    }
}

fn seed_workflow_hook(state: &AppState) {
    let root = state
        .workflow_runs_path
        .parent()
        .expect("workflow state directory")
        .join("builtin_workflows");
    let workflows = root.join("workflows");
    let hooks = root.join("hooks");
    std::fs::create_dir_all(&workflows).expect("create workflow directory");
    std::fs::create_dir_all(&hooks).expect("create hook directory");
    std::fs::write(
        workflows.join("hosted_hook.yaml"),
        "workflow:\n  id: hosted_hook\n  name: Hosted Hook\n  steps:\n    - action: tool:workflow_test.executor\n",
    )
    .expect("write workflow");
    std::fs::write(
        hooks.join("hosted_hook.yaml"),
        "hooks:\n  - id: hosted_hook.task_completed.notify\n    workflow_id: hosted_hook\n    event: task_completed\n    actions:\n      - action: tool:workflow_test.slack\n",
    )
    .expect("write hook");
}

async fn patch_hook(app: &Router, assertion: &str, path: &str) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(path)
                .header("x-tandem-context-assertion", assertion)
                .header("content-type", "application/json")
                .body(Body::from(json!({ "enabled": false }).to_string()))
                .expect("patch request"),
        )
        .await
        .expect("patch response")
}

#[tokio::test]
async fn hosted_workflow_share_grant_cannot_change_deployment_hook() {
    let state = crate::test_support::test_state().await;
    seed_workflow_hook(&state);
    state.reload_workflows().await.expect("reload workflows");
    let binding_id = state
        .list_workflow_hooks(None)
        .await
        .into_iter()
        .find(|hook| hook.workflow_id == "hosted_hook")
        .expect("seeded hook")
        .binding_id;

    let temp = tempfile::tempdir().expect("temporary policy directory");
    let key = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
    let raw = json!({"key-a": {"purpose": "context_assertion",
        "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
        "organization_id": "org-a", "deployment_id": "dep-a", "allowed_audiences": ["tandem-runtime"], "status": "active"}}).to_string();
    let security = crate::context_assertion_security::RuntimeContextAssertionSecurity::from_test_metadata_keyring(
        &raw,
        &temp.path().join("replay.json"),
    );
    *state.context_assertion_security.write().unwrap() = Some(std::sync::Arc::new(security));
    let policy_path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", policy_path.clone());
    let now = crate::now_ms();
    write_policy(&policy_path, now);
    state
        .reload_hosted_policy()
        .await
        .expect("reload hosted policy");
    let app = super::super::routes_workflows::apply(Router::new())
        .route(
            "/direct-workflow-hook/{id}",
            axum::routing::patch(super::super::workflows::workflow_hooks_patch),
        )
        .layer(axum::middleware::from_fn_with_state(state.clone(), ingress))
        .with_state(state.clone());
    let sign = |actor, role| {
        super::tests::sign_test_context_assertion(&key, "key-a", claims(actor, role, now))
    };

    let before_file = std::fs::read(&state.workflow_hook_overrides_path).ok();
    assert_eq!(
        patch_hook(
            &app,
            &sign("bob", "member"),
            &format!("/workflow-hooks/{binding_id}"),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        patch_hook(
            &app,
            &sign("bob", "member"),
            &format!("/direct-workflow-hook/{binding_id}"),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        std::fs::read(&state.workflow_hook_overrides_path).ok(),
        before_file
    );
    assert!(state
        .list_workflow_hooks(None)
        .await
        .iter()
        .any(|hook| hook.binding_id == binding_id && hook.enabled));

    assert_eq!(
        patch_hook(
            &app,
            &sign("alice", "admin"),
            &format!("/workflow-hooks/{binding_id}"),
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        state.workflow_hook_overrides.read().await.get(&binding_id),
        Some(&false)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_workflow_hook_commit_survives_disconnect_before_policy_reload() {
    let state = crate::test_support::test_state().await;
    seed_workflow_hook(&state);
    state.reload_workflows().await.expect("reload workflows");
    let binding_id = state
        .list_workflow_hooks(None)
        .await
        .into_iter()
        .find(|hook| hook.workflow_id == "hosted_hook")
        .expect("seeded hook")
        .binding_id;
    let temp = tempfile::tempdir().expect("hosted policy directory");
    let policy_path = temp.path().join("policy.json");
    let now = crate::now_ms();
    write_policy(&policy_path, now);
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", policy_path.clone());
    state.reload_hosted_policy().await.expect("initial policy");
    let mut verified: tandem_types::VerifiedTenantContext = claims("alice", "admin", now).into();
    state
        .enterprise
        .hosted_policy
        .project(&mut verified)
        .expect("project admin");

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let commit_state = state.clone();
    let commit_id = binding_id.clone();
    let caller = tokio::spawn(async move {
        commit_state
            .set_workflow_hook_enabled_with_prewrite(
                &commit_id,
                false,
                Some(&verified),
                move || {
                    entered_tx.send(()).expect("signal prewrite");
                    release_rx.recv().expect("release prewrite");
                },
            )
            .await
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("hook commit reached guarded write");
    assert!(state
        .enterprise
        .hosted_policy
        .publication_write_blocked_for_test());
    caller.abort();
    assert!(caller.await.is_err());

    let mut policy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&policy_path).expect("read policy"))
            .expect("parse policy");
    policy["policy_version"] = json!(2);
    policy["users"][0]["role"] = json!("member");
    policy["users"][0]["capabilities"] =
        json!(tandem_enterprise_contract::hosted_policy::role_capabilities("member"));
    std::fs::write(
        &policy_path,
        serde_json::to_vec(&policy).expect("encode policy"),
    )
    .expect("downgrade policy on disk");
    let reload_state = state.clone();
    let mut reload = tokio::spawn(async move { reload_state.reload_hosted_policy().await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut reload)
            .await
            .is_err(),
        "policy publication must wait for the guarded hook write"
    );
    release_tx.send(()).expect("release hook write");
    tokio::time::timeout(std::time::Duration::from_secs(5), reload)
        .await
        .expect("policy reload completed")
        .expect("reload task")
        .expect("publish downgraded policy");
    assert_eq!(
        state.workflow_hook_overrides.read().await.get(&binding_id),
        Some(&false)
    );
    assert!(state
        .list_workflow_hooks(None)
        .await
        .iter()
        .any(|hook| hook.binding_id == binding_id && !hook.enabled));
    let persisted: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&state.workflow_hook_overrides_path).expect("persisted override"),
    )
    .expect("parse override");
    assert_eq!(persisted[&binding_id], json!(false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_workflow_hook_expired_admin_cannot_finish_paused_write() {
    let state = crate::test_support::test_state().await;
    seed_workflow_hook(&state);
    state.reload_workflows().await.expect("reload workflows");
    let binding_id = state
        .list_workflow_hooks(None)
        .await
        .into_iter()
        .find(|hook| hook.workflow_id == "hosted_hook")
        .expect("seeded hook")
        .binding_id;
    let temp = tempfile::tempdir().expect("hosted policy directory");
    let policy_path = temp.path().join("policy.json");
    let now = crate::now_ms();
    write_policy(&policy_path, now);
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", policy_path);
    state.reload_hosted_policy().await.expect("initial policy");
    let mut verified: tandem_types::VerifiedTenantContext = claims("alice", "admin", now).into();
    verified.expires_at_ms = crate::now_ms() + 3_000;
    state
        .enterprise
        .hosted_policy
        .project(&mut verified)
        .expect("project short-lived admin");
    let expires_at_ms = verified.expires_at_ms;
    let before_file = std::fs::read(&state.workflow_hook_overrides_path).ok();

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let commit_state = state.clone();
    let commit_id = binding_id.clone();
    let caller = tokio::spawn(async move {
        commit_state
            .set_workflow_hook_enabled_with_prewrite(
                &commit_id,
                false,
                Some(&verified),
                move || {
                    entered_tx.send(()).expect("signal prewrite");
                    release_rx.recv().expect("release prewrite");
                },
            )
            .await
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("hook commit reached guarded write");
    tokio::time::sleep(std::time::Duration::from_millis(
        expires_at_ms.saturating_sub(crate::now_ms()) + 10,
    ))
    .await;
    release_tx.send(()).expect("release hook write");
    let error = tokio::time::timeout(std::time::Duration::from_secs(5), caller)
        .await
        .expect("hook commit completed")
        .expect("caller task")
        .expect_err("expired admin cannot write hook override");
    assert!(error
        .downcast_ref::<crate::app::state::WorkflowHookAdminDenied>()
        .is_some());
    assert_eq!(
        std::fs::read(&state.workflow_hook_overrides_path).ok(),
        before_file
    );
    assert!(state
        .workflow_hook_overrides
        .read()
        .await
        .get(&binding_id)
        .is_none());
    assert!(state
        .list_workflow_hooks(None)
        .await
        .iter()
        .any(|hook| hook.binding_id == binding_id && hook.enabled));
}
