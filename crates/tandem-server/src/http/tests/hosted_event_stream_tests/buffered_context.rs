// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use crate::http::{context_runs as context, context_types::ContextRunsStreamCursor};
use axum::response::{IntoResponse, Sse};
use base64::Engine;
use ed25519_dalek::Signer;
use tandem_types::{
    AccessPermission, DataClass, OrganizationUnitAccessGrant, ResourceKind, ResourceRef,
};

#[path = "history_authority.rs"]
mod history_authority;
#[path = "projection_authority.rs"]
mod projection_authority;

async fn group_fixture() -> StreamFixture {
    let mut fixture = StreamFixture::new(&["automation.read", "workflow.read"]).await;
    fixture.policy["policy_version"] = json!(2);
    fixture.policy["org_units"] = json!([{
        "id": "eng", "slug": "eng", "display_name": "Engineering",
        "kind": "department", "state": "active"
    }]);
    fixture.policy["org_unit_memberships"] = json!([{"unit_id": "eng", "user_id": "alice"}]);
    fixture.verified.policy_version = Some(2);
    fixture.verified.org_units = vec!["eng".into()];
    std::fs::write(&fixture.path, serde_json::to_vec(&fixture.policy).unwrap()).unwrap();
    fixture.state.reload_hosted_policy().await.unwrap();
    fixture
}

async fn managed_automation(fixture: &StreamFixture, native_id: &str, visibility: &str) -> String {
    fixture.automation_run_and_goal(native_id, "bob").await;
    fixture.automation(native_id, "bob", visibility).await;
    let run_id = format!("automation-v2-{native_id}");
    fixture.context_run(&run_id, "bob", "automation_v2").await;
    run_id
}

async fn exact_read_grant(fixture: &StreamFixture, native_id: &str) {
    let grant = OrganizationUnitAccessGrant::active(
        "buffered-context-read",
        stream_tenant("alice"),
        tandem_enterprise_contract::hosted_policy::hosted_unit_principal("eng"),
        ResourceRef::new("org-a", "dep-a", ResourceKind::Automation, native_id),
        crate::now_ms(),
    )
    .with_permissions(vec![AccessPermission::Read])
    .with_data_classes(vec![DataClass::Internal]);
    fixture
        .state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(grant.grant_id.clone(), grant);
}

async fn remove_sharing(fixture: &StreamFixture, native_id: &str) {
    let mut specs = fixture.state.automations_v2.write().await;
    specs.get_mut(native_id).unwrap().metadata.as_mut().unwrap()["resource_access"]
        ["audience_principals"] = json!([]);
}

async fn assert_resource_revoked_identity_current(fixture: &StreamFixture, run_id: &str) {
    assert!(
        crate::http::event_stream_authority::current_context(
            &fixture.state,
            &stream_tenant("alice"),
            Some(&fixture.verified),
            None,
        )
        .is_ok(),
        "resource revocation must not invalidate the hosted identity"
    );
    assert!(
        crate::http::event_stream_authority::current_context(
            &fixture.state,
            &stream_tenant("alice"),
            Some(&fixture.verified),
            Some(AccessPermission::HostedAutomationRead),
        )
        .is_ok(),
        "the hosted automation read operation remains authorized"
    );
    assert!(
        !crate::http::context_run_authority::run_stream_resource_visible(
            &fixture.state,
            &stream_tenant("alice"),
            Some(&fixture.verified),
            &crate::http::context_run_authority::RunStreamResource::ContextRun(run_id.into()),
        )
        .await
    );
}

async fn workspace(fixture: &StreamFixture) -> String {
    tandem_core::normalize_workspace_path(&fixture.state.workspace_index.snapshot().await.root)
        .unwrap()
}

fn append_replay(
    fixture: &StreamFixture,
    run_id: &str,
    kind: &str,
    seq: u64,
    ts_ms: u64,
    marker: &str,
) {
    let (path, row) = if kind == "context_run_event" {
        (
            context::context_run_events_path(&fixture.state, run_id),
            json!({
                "event_id": format!("buffered-{run_id}-{seq}"), "run_id": run_id,
                "seq": seq, "ts_ms": ts_ms, "type": "context.run.updated",
                "status": "queued", "revision": 1, "payload": {"marker": marker}
            }),
        )
    } else {
        (
            context::context_run_blackboard_patches_path(&fixture.state, run_id),
            json!({
                "patch_id": format!("buffered-patch-{run_id}-{seq}"), "run_id": run_id,
                "seq": seq, "ts_ms": ts_ms, "source_event_seq": seq,
                "op": "set_rolling_summary", "payload": {"summary": marker}
            }),
        )
    };
    context::append_jsonl_line(&path, &row).unwrap();
}

async fn frame_receiver(
    fixture: &StreamFixture,
    run_ids: Vec<String>,
) -> tokio::sync::mpsc::Receiver<context::ContextRunsQueuedFrame> {
    context::context_runs_multiplex_frame_receiver(
        fixture.state.clone(),
        stream_tenant("alice"),
        Some(fixture.verified.clone()),
        workspace(fixture).await,
        run_ids,
        ContextRunsStreamCursor::default(),
        None,
    )
}

async fn wait_buffered(
    receiver: &tokio::sync::mpsc::Receiver<context::ContextRunsQueuedFrame>,
    count: usize,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while receiver.len() < count {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("production frames must actually enter the queue");
}

async fn wait_live_subscriber(state: &AppState, previous: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while state.event_bus.receiver_count() <= previous {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("production producer must finish replay and subscribe to live events");
}

fn dequeue_body(
    fixture: &StreamFixture,
    receiver: tokio::sync::mpsc::Receiver<context::ContextRunsQueuedFrame>,
) -> Body {
    let stream = context::context_runs_multiplex_dequeue_stream(
        fixture.state.clone(),
        stream_tenant("alice"),
        Some(fixture.verified.clone()),
        receiver,
    );
    Sse::new(crate::http::event_stream_authority::guard(
        stream,
        fixture.state.clone(),
        stream_tenant("alice"),
        Some(fixture.verified.clone()),
        None,
    ))
    .into_response()
    .into_body()
}

fn dequeue_body_observed(
    fixture: &StreamFixture,
    receiver: tokio::sync::mpsc::Receiver<context::ContextRunsQueuedFrame>,
    progress: tokio::sync::mpsc::UnboundedSender<String>,
) -> Body {
    let stream = context::context_runs_multiplex_dequeue_stream_observed(
        fixture.state.clone(),
        stream_tenant("alice"),
        Some(fixture.verified.clone()),
        receiver,
        progress,
    );
    Sse::new(crate::http::event_stream_authority::guard(
        stream,
        fixture.state.clone(),
        stream_tenant("alice"),
        Some(fixture.verified.clone()),
        None,
    ))
    .into_response()
    .into_body()
}

async fn resolved_id(progress: &mut tokio::sync::mpsc::UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(5), progress.recv())
        .await
        .expect("dequeue must resolve the earlier native resource")
        .expect("per-stream progress observer")
}

fn frame_json(bytes: &[u8]) -> Value {
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(!text.contains("\nevent:") && !text.contains("\nid:") && !text.contains("\nretry:"));
    serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap()
}

async fn next_frame(
    body: &mut (impl futures::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin),
) -> Value {
    let next = tokio::time::timeout(Duration::from_secs(5), body.next())
        .await
        .expect("an authorized frame must remain deliverable")
        .unwrap()
        .unwrap();
    frame_json(&next)
}

fn publish_live(fixture: &StreamFixture, run_id: &str, workspace: &str, marker: &str) {
    context::publish_context_run_stream_envelope(
        &fixture.state,
        &crate::http::context_types::ContextRunsStreamEnvelope {
            kind: "context_run_event".into(),
            run_id: run_id.into(),
            workspace: workspace.into(),
            seq: 1,
            ts_ms: crate::now_ms(),
            payload: json!({"marker": marker}),
        },
    );
}

fn sign_context_assertion(
    key: &ed25519_dalek::SigningKey,
    kid: &str,
    claims: &TenantContextAssertionClaims,
) -> String {
    let encoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = tandem_types::TenantContextAssertionHeader::ed25519(kid);
    let encoded_header = encoder.encode(serde_json::to_vec(&header).unwrap());
    let encoded_claims = encoder.encode(serde_json::to_vec(claims).unwrap());
    let input = format!("{encoded_header}.{encoded_claims}");
    let signature = key.sign(input.as_bytes());
    format!("{input}.{}", encoder.encode(signature.to_bytes()))
}

#[tokio::test]
async fn buffered_context_ready_replay_events_and_patches_recheck_exact_grant_revoke_and_expiry() {
    for expire in [false, true] {
        let fixture = group_fixture().await;
        let run_id = managed_automation(&fixture, "scoped-buffered", "private").await;
        exact_read_grant(&fixture, "scoped-buffered").await;
        fixture
            .context_run("unrelated-buffered", "alice", "interactive")
            .await;
        append_replay(
            &fixture,
            &run_id,
            "context_run_event",
            1,
            10,
            "denied-event",
        );
        append_replay(&fixture, &run_id, "blackboard_patch", 1, 20, "denied-patch");
        append_replay(
            &fixture,
            "unrelated-buffered",
            "context_run_event",
            1,
            30,
            "allowed-other",
        );
        let receiver =
            frame_receiver(&fixture, vec![run_id.clone(), "unrelated-buffered".into()]).await;
        wait_buffered(&receiver, 4).await;
        assert_eq!(
            receiver.len(),
            4,
            "ready, both replay representations, and control are queued"
        );

        let mut grants = fixture
            .state
            .enterprise
            .org_unit_access_grants
            .write()
            .await;
        if expire {
            grants
                .get_mut("buffered-context-read")
                .unwrap()
                .expires_at_ms = Some(crate::now_ms().saturating_sub(1));
        } else {
            grants.remove("buffered-context-read");
        }
        drop(grants);
        assert_resource_revoked_identity_current(&fixture, &run_id).await;

        let mut body = dequeue_body(&fixture, receiver).into_data_stream();
        let ready = next_frame(&mut body).await;
        assert_eq!(ready["kind"], "ready");
        assert_eq!(ready["subscribed_run_ids"], json!(["unrelated-buffered"]));
        let other = next_frame(&mut body).await;
        assert_eq!(other["run_id"], "unrelated-buffered");
        assert_eq!(other["payload"]["payload"]["marker"], "allowed-other");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), body.next())
                .await
                .is_err(),
            "revocation filters frames but does not close independently authorized subscriptions"
        );
    }
}

#[tokio::test]
async fn buffered_context_live_frame_rechecks_group_sharing_without_policy_change() {
    let fixture = group_fixture().await;
    let run_id = managed_automation(&fixture, "group-buffered", "group").await;
    fixture
        .context_run("other-live", "alice", "interactive")
        .await;
    let previous = fixture.state.event_bus.receiver_count();
    let receiver = frame_receiver(&fixture, vec![run_id.clone(), "other-live".into()]).await;
    wait_live_subscriber(&fixture.state, previous).await;
    let workspace = workspace(&fixture).await;
    publish_live(&fixture, &run_id, &workspace, "denied-live");
    publish_live(&fixture, "other-live", &workspace, "allowed-live");
    wait_buffered(&receiver, 3).await;
    remove_sharing(&fixture, "group-buffered").await;
    assert_resource_revoked_identity_current(&fixture, &run_id).await;

    let mut body = dequeue_body(&fixture, receiver).into_data_stream();
    let ready = next_frame(&mut body).await;
    assert_eq!(ready["subscribed_run_ids"], json!(["other-live"]));
    let next = next_frame(&mut body).await;
    assert_eq!(next["run_id"], "other-live");
    assert_eq!(next["payload"]["marker"], "allowed-live");
    assert!(
        next.get("tenantContext").is_some(),
        "live wire extensions must be retained"
    );
}

#[tokio::test]
async fn buffered_context_dequeue_filters_a_previously_authorized_blocked_send() {
    // Helper-level pending-send check; production buffering is exercised by the
    // preceding tests and the full router regression below.
    let fixture = group_fixture().await;
    let run_id = managed_automation(&fixture, "blocked-send", "group").await;
    fixture
        .context_run("blocked-control", "alice", "interactive")
        .await;
    assert!(
        crate::http::context_run_authority::run_stream_resource_visible(
            &fixture.state,
            &stream_tenant("alice"),
            Some(&fixture.verified),
            &crate::http::context_run_authority::RunStreamResource::ContextRun(run_id.clone()),
        )
        .await
    );
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    sender
        .send(context::ContextRunsQueuedFrame::Envelope {
            run_id: run_id.clone(),
            payload: json!({"marker": "queued-before-revoke"}).to_string(),
        })
        .await
        .unwrap();
    let pending = sender.send(context::ContextRunsQueuedFrame::Envelope {
        run_id: run_id.clone(),
        payload: json!({"marker": "blocked-before-revoke"}).to_string(),
    });
    tokio::pin!(pending);
    assert!(futures::poll!(&mut pending).is_pending());
    remove_sharing(&fixture, "blocked-send").await;
    assert_resource_revoked_identity_current(&fixture, &run_id).await;

    let mut body = dequeue_body(&fixture, receiver).into_data_stream();
    let next = body.next();
    tokio::pin!(next);
    assert!(futures::poll!(&mut next).is_pending());
    pending.await.unwrap();
    // A sender also blocks while the revoked second frame still occupies the
    // queue; drive consumer and control send together without sleeps.
    let control = sender.send(context::ContextRunsQueuedFrame::Envelope {
        run_id: "blocked-control".into(),
        payload: json!({"marker": "allowed-control"}).to_string(),
    });
    let (next, sent) = tokio::join!(next, control);
    sent.unwrap();
    assert_eq!(
        frame_json(&next.unwrap().unwrap())["marker"],
        "allowed-control"
    );
}

#[test]
fn buffered_context_production_router_rechecks_replay_after_sharing_is_removed() {
    const TEST_NAME: &str = "http::hosted_event_stream_tests::buffered_context::buffered_context_production_router_rechecks_replay_after_sharing_is_removed";
    const CHILD_MARKER: &str = "TANDEM_TEST_BUFFERED_HOSTED_ROUTER_CHILD";
    const CHILD_COMPLETED: &str = "buffered-hosted-router-child-completed";

    if std::env::var_os(CHILD_MARKER).as_deref() != Some(std::ffi::OsStr::new("1")) {
        let root = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                TEST_NAME,
                "--nocapture",
                "--test-threads=1",
                "--color=never",
            ])
            .env(CHILD_MARKER, "1")
            .env("TMPDIR", root.path())
            .env("TMP", root.path())
            .env("TEMP", root.path())
            .env("TANDEM_RUNTIME_AUTH_MODE", "hosted_single_tenant");
        // Configure only the child. Never let inherited operator audit keys,
        // external anchors, or control-plane endpoints reach this test.
        for name in [
            "TANDEM_AUDIT_HMAC_KEY",
            "TANDEM_AUDIT_HMAC_KEY_FILE",
            "TANDEM_AUDIT_HMAC_KEY_ID",
            "TANDEM_AUDIT_HMAC_KEYRING_FILE",
            "TANDEM_AUDIT_ANCHOR_DIR",
            "HOSTED_CONTROL_PANEL_PUBLIC_URL",
            "HOSTED_PUBLIC_URL",
            "TANDEM_HOSTED_CONTROL_PLANE_URL",
            "TANDEM_ENTERPRISE_CONTROL_PLANE_URL",
        ] {
            command.env_remove(name);
        }
        // test_state creates its state below TMPDIR. This sibling anchor is
        // temporary and external to that state root, as hosted audit requires.
        command
            .env("TANDEM_AUDIT_ANCHOR_DIR", root.path().join("audit-anchor"))
            .env(
                "TANDEM_AUDIT_HMAC_KEY",
                "buffered-context-router-test-only-hmac-key-32-bytes",
            );
        let output = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "hosted router child failed with {}: {stdout}\n{stderr}",
            output.status
        );
        assert_eq!(stdout.matches("running 1 test").count(), 1, "{stdout}");
        assert!(
            stdout.contains(&format!("test {TEST_NAME} ... ok")),
            "{stdout}"
        );
        assert!(stdout.contains("1 passed; 0 failed; 0 ignored"), "{stdout}");
        assert_eq!(
            stderr
                .lines()
                .filter(|line| *line == CHILD_COMPLETED)
                .count(),
            1,
            "child did not complete actual hosted-router assertions: {stderr}"
        );
        eprintln!(
            "hosted router child status: {}\n{stdout}\n{stderr}",
            output.status
        );
        return;
    }

    assert_eq!(
        std::env::var("TANDEM_RUNTIME_AUTH_MODE").unwrap(),
        "hosted_single_tenant"
    );
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(buffered_context_production_router_child());
    eprintln!("{CHILD_COMPLETED}");
}

async fn buffered_context_production_router_child() {
    let mut fixture = group_fixture().await;
    let run_id = managed_automation(&fixture, "production-buffered", "group").await;
    let control_run_id = managed_automation(&fixture, "production-control", "group").await;
    append_replay(
        &fixture,
        &run_id,
        "context_run_event",
        1,
        10,
        "production-denied-event",
    );
    append_replay(
        &fixture,
        &run_id,
        "blackboard_patch",
        1,
        20,
        "production-denied-patch",
    );
    append_replay(
        &fixture,
        &control_run_id,
        "context_run_event",
        1,
        30,
        "production-allowed",
    );

    let key = ed25519_dalek::SigningKey::from_bytes(&[91; 32]);
    let raw = json!({"buffered-key": {
        "purpose": "context_assertion",
        "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
        "organization_id": "org-a", "deployment_id": "dep-a",
        "allowed_audiences": ["tandem-runtime"], "status": "active"
    }}).to_string();
    let security = crate::context_assertion_security::RuntimeContextAssertionSecurity::from_test_metadata_keyring(
        &raw, &fixture._temp.path().join("buffered-replay.json"),
    );
    *fixture.state.context_assertion_security.write().unwrap() =
        Some(std::sync::Arc::new(security));
    fixture
        .state
        .set_api_token(Some("buffered-route-token".into()))
        .await;
    let now = crate::now_ms();
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        "buffered-production-request",
        stream_tenant("alice"),
        HumanActor::tandem_user("alice"),
        AuthorityChain::from_request(tandem_types::RequestPrincipal::authenticated_user(
            "alice",
            "tandem-web",
        )),
        fixture.verified.roles.clone(),
    );
    claims.policy_version = fixture.verified.policy_version;
    claims.capabilities = fixture.verified.capabilities.clone();
    claims.org_units = fixture.verified.org_units.clone();
    fixture.verified = claims.clone().into();
    let assertion = sign_context_assertion(&key, "buffered-key", &claims);
    let uri = format!(
        "/context/runs/events/stream?workspace={}&run_ids={},{}",
        urlencoding::encode(&workspace(&fixture).await),
        run_id,
        control_run_id
    );
    let previous = fixture.state.event_bus.receiver_count();
    let response = crate::http::app_router(fixture.state.clone())
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", "Bearer buffered-route-token")
                .header("x-tandem-context-assertion", assertion)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    // The actual producer subscribes only after every replay send completed.
    // Do not poll the HTTP body until ready and replay are definitely buffered.
    wait_live_subscriber(&fixture.state, previous).await;
    remove_sharing(&fixture, "production-buffered").await;
    assert_resource_revoked_identity_current(&fixture, &run_id).await;
    let mut body = response.into_body().into_data_stream();
    let ready = next_frame(&mut body).await;
    assert_eq!(ready["subscribed_run_ids"], json!([control_run_id]));
    let next = next_frame(&mut body).await;
    assert_eq!(next["run_id"], control_run_id);
    assert_eq!(next["payload"]["payload"]["marker"], "production-allowed");
}

#[tokio::test]
async fn buffered_context_ready_rechecks_earlier_resource_after_later_lookup_wait() {
    let fixture = group_fixture().await;
    let run_id = managed_automation(&fixture, "earlier-ready", "group").await;
    fixture.workflow("later-ready", "alice").await;
    fixture
        .context_run("workflow-later-ready", "alice", "workflow")
        .await;
    let previous = fixture.state.event_bus.receiver_count();
    let receiver = frame_receiver(
        &fixture,
        vec![run_id.clone(), "workflow-later-ready".into()],
    )
    .await;
    wait_live_subscriber(&fixture.state, previous).await;
    let held = fixture.state.workflow_runs.write().await;
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut body = dequeue_body_observed(&fixture, receiver, progress_tx).into_data_stream();
    let reader = tokio::spawn(async move { next_frame(&mut body).await });
    // The first native resolution has completed, while the held later native
    // lock prevents the batch from completing. No scheduling sleep is needed.
    assert_eq!(resolved_id(&mut progress_rx).await, run_id);
    assert!(
        !reader.is_finished(),
        "the later workflow lookup must remain pending"
    );
    remove_sharing(&fixture, "earlier-ready").await;
    assert_resource_revoked_identity_current(&fixture, &run_id).await;
    drop(held);
    let ready = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready["subscribed_run_ids"], json!(["workflow-later-ready"]));
}

#[tokio::test]
async fn buffered_context_ready_waits_for_contended_current_grants_without_denying_reader() {
    let fixture = group_fixture().await;
    let run_id = managed_automation(&fixture, "contended-ready", "private").await;
    exact_read_grant(&fixture, "contended-ready").await;
    let previous = fixture.state.event_bus.receiver_count();
    let receiver = frame_receiver(&fixture, vec![run_id.clone()]).await;
    wait_live_subscriber(&fixture.state, previous).await;
    let held = fixture
        .state
        .enterprise
        .org_unit_access_grants
        .write()
        .await;
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut body = dequeue_body_observed(&fixture, receiver, progress_tx).into_data_stream();
    let reader = tokio::spawn(async move { next_frame(&mut body).await });
    assert_eq!(resolved_id(&mut progress_rx).await, run_id);
    assert!(
        !reader.is_finished(),
        "current grant read must await ordinary contention"
    );
    drop(held);
    let ready = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready["subscribed_run_ids"], json!([run_id]));
}

#[tokio::test]
async fn buffered_context_held_read_grants_remain_valid_with_revocation_writer_queued() {
    // This helper control specifically exercises the held-registry evaluator;
    // production dequeue/replay reproduction is covered by the route test.
    let fixture = group_fixture().await;
    managed_automation(&fixture, "queued-writer", "private").await;
    exact_read_grant(&fixture, "queued-writer").await;
    let _publication = fixture
        .state
        .enterprise
        .hosted_policy
        .lock_publication()
        .await;
    let specs = fixture.state.automations_v2.read().await;
    let memberships = fixture.state.enterprise.org_unit_memberships.read().await;
    let grants = fixture.state.enterprise.org_unit_access_grants.read().await;
    let cross = fixture.state.enterprise.cross_tenant_grants.read().await;
    let writer = fixture.state.enterprise.org_unit_access_grants.write();
    tokio::pin!(writer);
    assert!(futures::poll!(writer.as_mut()).is_pending());
    assert!(
        crate::http::automation_object_authority::can_read_with_held_grants(
            &fixture.state,
            &stream_tenant("alice"),
            Some(&fixture.verified),
            specs.get("queued-writer").unwrap(),
            &memberships,
            &grants,
            &cross,
        ),
        "a held current exact grant must not be re-locked with try_read behind the queued writer"
    );
    drop(grants);
    let mut writer = writer.await;
    writer.remove("buffered-context-read");
}

#[tokio::test]
async fn buffered_context_ready_rechecks_native_binding_after_later_lookup_wait() {
    let fixture = group_fixture().await;
    let run_id = managed_automation(&fixture, "native-ready", "group").await;
    fixture.workflow("later-native", "alice").await;
    fixture
        .context_run("workflow-later-native", "alice", "workflow")
        .await;
    let previous = fixture.state.event_bus.receiver_count();
    let receiver = frame_receiver(
        &fixture,
        vec![run_id.clone(), "workflow-later-native".into()],
    )
    .await;
    wait_live_subscriber(&fixture.state, previous).await;
    let held = fixture.state.workflow_runs.write().await;
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut body = dequeue_body_observed(&fixture, receiver, progress_tx).into_data_stream();
    let reader = tokio::spawn(async move { next_frame(&mut body).await });
    assert_eq!(resolved_id(&mut progress_rx).await, run_id);
    fixture
        .state
        .automation_v2_runs
        .write()
        .await
        .get_mut("native-ready")
        .unwrap()
        .tenant_context = stream_tenant("charlie");
    assert!(crate::http::event_stream_authority::current_context(
        &fixture.state,
        &stream_tenant("alice"),
        Some(&fixture.verified),
        None,
    )
    .is_ok());
    drop(held);
    let ready = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ready["subscribed_run_ids"],
        json!(["workflow-later-native"])
    );
}

#[tokio::test]
async fn buffered_context_ready_rechecks_session_owner_after_later_lookup_wait() {
    let fixture = group_fixture().await;
    let mut session = tandem_types::Session::new(Some("buffered ready owner".into()), None);
    session.tenant_context = stream_tenant("alice");
    let run_id = format!("session-{}", session.id);
    fixture
        .state
        .storage
        .save_session(session.clone())
        .await
        .unwrap();
    fixture.context_run(&run_id, "alice", "session").await;
    fixture.workflow("later-session", "alice").await;
    fixture
        .context_run("workflow-later-session", "alice", "workflow")
        .await;
    let previous = fixture.state.event_bus.receiver_count();
    let receiver = frame_receiver(
        &fixture,
        vec![run_id.clone(), "workflow-later-session".into()],
    )
    .await;
    wait_live_subscriber(&fixture.state, previous).await;
    let held = fixture.state.workflow_runs.write().await;
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut body = dequeue_body_observed(&fixture, receiver, progress_tx).into_data_stream();
    let reader = tokio::spawn(async move { next_frame(&mut body).await });
    assert_eq!(resolved_id(&mut progress_rx).await, run_id);
    session.tenant_context = stream_tenant("bob");
    fixture.state.storage.save_session(session).await.unwrap();
    assert!(crate::http::event_stream_authority::current_context(
        &fixture.state,
        &stream_tenant("alice"),
        Some(&fixture.verified),
        None,
    )
    .is_ok());
    drop(held);
    let ready = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ready["subscribed_run_ids"],
        json!(["workflow-later-session"])
    );
}

#[tokio::test]
async fn buffered_context_keeps_current_scoped_group_owner_and_reviewer_reads() {
    let mut fixture = group_fixture().await;
    let scoped = managed_automation(&fixture, "current-scoped", "private").await;
    exact_read_grant(&fixture, "current-scoped").await;
    let group = managed_automation(&fixture, "current-group", "group").await;
    fixture
        .context_run("current-owner", "alice", "interactive")
        .await;
    let run_ids = vec![scoped, group, "current-owner".into()];
    for (index, run_id) in run_ids.iter().enumerate() {
        append_replay(
            &fixture,
            run_id,
            "context_run_event",
            1,
            index as u64 + 1,
            run_id,
        );
    }
    let receiver = frame_receiver(&fixture, run_ids.clone()).await;
    wait_buffered(&receiver, 4).await;
    let mut body = dequeue_body(&fixture, receiver).into_data_stream();
    assert_eq!(
        next_frame(&mut body).await["subscribed_run_ids"],
        json!(run_ids)
    );
    for run_id in &run_ids {
        assert_eq!(next_frame(&mut body).await["run_id"], json!(run_id));
    }

    fixture.policy["policy_version"] = json!(3);
    fixture.policy["users"][0]["role"] = json!("admin");
    fixture.policy["users"][0]["capabilities"] =
        json!(["automation.read", "workflow.read", "hosted.admin"]);
    fixture.verified.policy_version = Some(3);
    fixture.verified.roles = vec!["hosted:role:admin".into()];
    fixture.verified.capabilities.push("hosted.admin".into());
    std::fs::write(&fixture.path, serde_json::to_vec(&fixture.policy).unwrap()).unwrap();
    fixture.state.reload_hosted_policy().await.unwrap();
    fixture.workflow("current-reviewed", "bob").await;
    fixture
        .context_run("workflow-current-reviewed", "bob", "workflow")
        .await;
    append_replay(
        &fixture,
        "workflow-current-reviewed",
        "context_run_event",
        1,
        1,
        "reviewer-control",
    );
    let receiver = frame_receiver(&fixture, vec!["workflow-current-reviewed".into()]).await;
    wait_buffered(&receiver, 2).await;
    let mut body = dequeue_body(&fixture, receiver).into_data_stream();
    assert_eq!(
        next_frame(&mut body).await["subscribed_run_ids"],
        json!(["workflow-current-reviewed"])
    );
    assert_eq!(
        next_frame(&mut body).await["run_id"],
        "workflow-current-reviewed"
    );
}

#[tokio::test]
async fn buffered_context_keeps_local_signed_and_unsigned_frames_and_cursor_order() {
    for signed in [false, true] {
        let fixture = StreamFixture::new(&[]).await;
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::local_implicit();
        let workspace =
            tandem_core::normalize_workspace_path(&state.workspace_index.snapshot().await.root)
                .unwrap();
        let run: crate::http::context_types::ContextRunState = serde_json::from_value(json!({
            "run_id": "local-buffered", "run_type": "interactive", "tenant_context": tenant,
            "status": "queued", "objective": "local", "workspace": {
                "workspace_id": "", "canonical_path": workspace, "lease_epoch": 0
            }, "revision": 1, "created_at_ms": 1, "updated_at_ms": 1
        }))
        .unwrap();
        context::save_context_run_state(&state, &run).await.unwrap();
        for (seq, kind, ts_ms) in [
            (1, "context_run_event", 10),
            (2, "blackboard_patch", 20),
            (3, "context_run_event", 30),
        ] {
            let (path, row) = if kind == "context_run_event" {
                (
                    context::context_run_events_path(&state, "local-buffered"),
                    json!({
                        "event_id": format!("local-{seq}"), "run_id": "local-buffered", "seq": seq,
                        "ts_ms": ts_ms, "type": "context.run.updated", "status": "queued", "payload": {}
                    }),
                )
            } else {
                (
                    context::context_run_blackboard_patches_path(&state, "local-buffered"),
                    json!({
                        "patch_id": "local-patch", "run_id": "local-buffered", "seq": seq,
                        "ts_ms": ts_ms, "op": "set_rolling_summary", "payload": {"summary": "local"}
                    }),
                )
            };
            context::append_jsonl_line(&path, &row).unwrap();
        }
        let mut verified = fixture.verified.clone();
        verified.policy_version = None;
        verified.tenant_context = tenant.clone();
        let mut app = crate::http::routes_context::apply(Router::new(), state.clone())
            .layer(Extension(tenant));
        if signed {
            app = app.layer(Extension(verified));
        }
        let cursor = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(
                &json!({"events": {"local-buffered": 1}, "patches": {"local-buffered": 0}}),
            )
            .unwrap(),
        );
        let response = app
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri(format!(
            "/context/runs/events/stream?workspace={}&run_ids=local-buffered&cursor={cursor}",
            urlencoding::encode(&workspace),
        ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body().into_data_stream();
        assert_eq!(
            next_frame(&mut body).await["subscribed_run_ids"],
            json!(["local-buffered"])
        );
        let patch = next_frame(&mut body).await;
        assert_eq!(patch["kind"], "blackboard_patch");
        assert_eq!(patch["seq"], 2);
        let event = next_frame(&mut body).await;
        assert_eq!(event["kind"], "context_run_event");
        assert_eq!(event["seq"], 3);
    }
}
