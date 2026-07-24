// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn public_webhook_body_limits_apply_before_collection_and_vary_by_provider() {
    let state = test_state().await;
    let tenant_context = tenant("org-limits", "workspace-limits");
    let generic = setup_webhook(&state, "automation-webhook-generic-limit", &tenant_context).await;
    state
        .put_automation_v2(minimal_automation(
            "automation-webhook-github-limit",
            &tenant_context,
        ))
        .await
        .expect("put GitHub automation");
    let mut github_input = create_input("automation-webhook-github-limit", tenant_context.clone());
    github_input.provider = "github".to_string();
    github_input.signature_scheme = Some(AutomationWebhookSignatureScheme::GithubHmacSha256);
    let github = state
        .create_automation_webhook_trigger(github_input)
        .await
        .expect("create GitHub trigger");
    let app = app_router(state.clone());

    let unknown_body = serde_json::to_vec(&json!({
        "blob": "x".repeat(33 * 1024),
    }))
    .expect("unknown body");
    let unknown_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/automations/unknown-capability-limit")
                .header("content-type", "application/json")
                .body(Body::from(unknown_body))
                .expect("unknown request"),
        )
        .await
        .expect("unknown response");
    assert_eq!(unknown_response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let generic_body = serde_json::to_vec(&json!({
        "blob": "x".repeat(257 * 1024),
    }))
    .expect("generic body");
    let generic_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    generic.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .body(Body::from(generic_body))
                .expect("generic request"),
        )
        .await
        .expect("generic response");
    assert_eq!(generic_response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let github_body = serde_json::to_vec(&json!({
        "blob": "x".repeat(300 * 1024),
    }))
    .expect("GitHub body");
    let github_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    github.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-github-delivery", "github-large-delivery")
                .header(
                    "x-hub-signature-256",
                    github_automation_webhook_signature_header(&github.secret, &github_body),
                )
                .body(Body::from(github_body))
                .expect("GitHub request"),
        )
        .await
        .expect("GitHub response");
    assert_eq!(github_response.status(), StatusCode::ACCEPTED);

    let forged_length = app_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/automations/another-unknown-capability")
                .header("content-type", "application/json")
                .header("content-length", "999999")
                .body(Body::from("{}"))
                .expect("forged content-length request"),
        )
        .await
        .expect("forged content-length response");
    assert_eq!(forged_length.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

/// Isolated TA-13 security load harness. The sequential wall budget is also a
/// conservative CPU/lock-time proxy: every request performs all work inline in
/// this process, and the per-request ceiling includes the rejection-ledger lock.
#[tokio::test]
#[ignore = "run explicitly as the bounded TA-13 bad-signature load harness"]
async fn bad_signature_load_is_throttled_and_retained_within_explicit_ceilings() {
    const REQUESTS: usize = 512;
    const MAX_UNTHROTTLED_REQUESTS: usize = 30;
    const MAX_TOTAL_CPU_AND_WALL_MS: u128 = 30_000;
    const MAX_REQUEST_AND_LOCK_LATENCY_MS: u128 = 2_000;
    const MAX_RETAINED_DISK_BYTES: u64 = 64 * 1024;
    const MAX_RETAINED_MEMORY_BYTES: usize = 128 * 1024;
    const MAX_REJECTION_DISK_WRITES: usize = MAX_UNTHROTTLED_REQUESTS;
    static ATTACKER_BODY: &[u8] = br#"{"payload":"ta13-attacker-body-marker"}"#;

    assert!(
        std::env::var("TANDEM_PUBLIC_WEBHOOK_RATE_LIMIT_PER_MIN").is_err()
            && std::env::var("TANDEM_PUBLIC_WEBHOOK_NETWORK_RATE_LIMIT_PER_MIN").is_err(),
        "run the isolated harness with the default production limiter ceilings"
    );

    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-ta13", "workspace-ta13");
    let created = setup_webhook(&state, "automation-webhook-ta13-load", &tenant_context).await;
    let app = app_router(state.clone());
    let durable_deliveries_before =
        tokio::fs::read_to_string(&state.automation_webhook_deliveries_path)
            .await
            .ok();
    let started = std::time::Instant::now();
    let mut maximum_request_latency = std::time::Duration::ZERO;
    let mut rejected = 0usize;
    let mut throttled = 0usize;

    for index in 0..REQUESTS {
        let request_started = std::time::Instant::now();
        let response = app
            .clone()
            .oneshot(webhook_request(
                &created.trigger.public_path_token,
                Some("ta13-wrong-secret"),
                ATTACKER_BODY,
                &format!("ta13-event-{index}"),
                crate::now_ms(),
            ))
            .await
            .expect("load-harness response");
        maximum_request_latency = maximum_request_latency.max(request_started.elapsed());
        match response.status() {
            StatusCode::UNAUTHORIZED => rejected = rejected.saturating_add(1),
            StatusCode::TOO_MANY_REQUESTS => throttled = throttled.saturating_add(1),
            status => panic!("unexpected load-harness status {status}"),
        }
    }

    let elapsed = started.elapsed();
    assert_eq!(rejected, MAX_UNTHROTTLED_REQUESTS);
    assert_eq!(throttled, REQUESTS - MAX_UNTHROTTLED_REQUESTS);
    assert!(elapsed.as_millis() <= MAX_TOTAL_CPU_AND_WALL_MS);
    assert!(
        maximum_request_latency.as_millis() <= MAX_REQUEST_AND_LOCK_LATENCY_MS,
        "single-request/lock latency was {maximum_request_latency:?}"
    );

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let retained_memory_bytes = deliveries
        .iter()
        .map(|delivery| {
            serde_json::to_vec(delivery)
                .expect("serialize retained delivery")
                .len()
        })
        .sum::<usize>();
    assert_eq!(deliveries.len(), MAX_UNTHROTTLED_REQUESTS);
    assert!(retained_memory_bytes <= MAX_RETAINED_MEMORY_BYTES);

    let rejection_ledger = state
        .automation_webhook_deliveries_path
        .parent()
        .expect("webhook state parent")
        .join("automation_webhook_rejections.jsonl");
    let telemetry = tokio::fs::read_to_string(&rejection_ledger)
        .await
        .expect("bounded rejection telemetry");
    let disk_writes = telemetry.lines().count();
    let retained_disk_bytes = tokio::fs::metadata(&rejection_ledger)
        .await
        .expect("rejection telemetry metadata")
        .len();
    assert!(disk_writes <= MAX_REJECTION_DISK_WRITES);
    assert!(retained_disk_bytes <= MAX_RETAINED_DISK_BYTES);
    assert!(!telemetry.contains("ta13-attacker-body-marker"));
    let durable_deliveries_after =
        tokio::fs::read_to_string(&state.automation_webhook_deliveries_path)
            .await
            .ok();
    assert_eq!(
        durable_deliveries_after, durable_deliveries_before,
        "pre-auth rejections must not enter the full-map durable store"
    );
    eprintln!(
        "TA-13 bounded load: requests={REQUESTS} rejected={rejected} throttled={throttled} total_ms={} max_request_lock_ms={} retained_memory_bytes={retained_memory_bytes} disk_writes={disk_writes} retained_disk_bytes={retained_disk_bytes}",
        elapsed.as_millis(),
        maximum_request_latency.as_millis(),
    );
}

#[tokio::test]
async fn notion_setup_nonce_is_required_correct_and_unexpired() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_notion_webhook(&state, "automation-notion-nonce", &tenant_context).await;
    let app = app_router(state.clone());
    let body = json!({ "verification_token": "should_not_capture" }).to_string();

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .expect("missing nonce request"),
        )
        .await
        .expect("missing nonce response");
    assert_eq!(missing.status(), StatusCode::OK);

    let wrong = app
        .clone()
        .oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            "wrong-setup-nonce",
            "should_not_capture",
        ))
        .await
        .expect("wrong nonce response");
    assert_eq!(wrong.status(), StatusCode::OK);
    let awaiting = state
        .get_automation_webhook_trigger(&tenant_context, &created.trigger.trigger_id)
        .await
        .expect("trigger after rejected setup attempts");
    assert_eq!(
        awaiting.notion_verification.expect("verification").status,
        AutomationWebhookNotionVerificationStatus::AwaitingToken
    );

    let expired =
        setup_notion_webhook(&state, "automation-notion-expired-nonce", &tenant_context).await;
    {
        let mut triggers = state.automation_webhook_triggers.write().await;
        triggers
            .get_mut(&expired.trigger.trigger_id)
            .and_then(|trigger| trigger.notion_verification.as_mut())
            .expect("expired verification")
            .setup_challenge_expires_at_ms = Some(crate::now_ms().saturating_sub(1));
    }
    let expired_response = app
        .oneshot(notion_verification_request(
            &expired.trigger.public_path_token,
            expired.notion_setup_nonce.as_deref().expect("setup nonce"),
            "expired_nonce_token",
        ))
        .await
        .expect("expired nonce response");
    assert_eq!(expired_response.status(), StatusCode::OK);
    let still_awaiting = state
        .get_automation_webhook_trigger(&tenant_context, &expired.trigger.trigger_id)
        .await
        .expect("expired trigger");
    assert_eq!(
        still_awaiting
            .notion_verification
            .expect("verification")
            .status,
        AutomationWebhookNotionVerificationStatus::AwaitingToken
    );
}

#[tokio::test]
async fn notion_setup_nonce_is_consumed_once_under_concurrency() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_notion_webhook(&state, "automation-notion-race", &tenant_context).await;
    let app = app_router(state.clone());
    let nonce = created.notion_setup_nonce.as_deref().expect("setup nonce");
    let (left, right) = tokio::join!(
        app.clone().oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            nonce,
            "concurrent_token_left",
        )),
        app.oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            nonce,
            "concurrent_token_right",
        )),
    );
    assert_eq!(left.expect("left response").status(), StatusCode::OK);
    assert_eq!(right.expect("right response").status(), StatusCode::OK);

    let revealed = state
        .reveal_automation_webhook_notion_verification_token(
            &tenant_context,
            "automation-notion-race",
            &created.trigger.trigger_id,
        )
        .await
        .expect("reveal race winner")
        .expect("one token captured");
    assert!(matches!(
        revealed.as_str(),
        "concurrent_token_left" | "concurrent_token_right"
    ));
    let trigger = state
        .get_automation_webhook_trigger(&tenant_context, &created.trigger.trigger_id)
        .await
        .expect("race trigger");
    let verification = trigger.notion_verification.expect("verification");
    assert_eq!(
        verification.status,
        AutomationWebhookNotionVerificationStatus::TokenReceived
    );
    assert!(verification.setup_challenge_digest.is_none());
    assert!(verification.setup_challenge_consumed_at_ms.is_some());
}

#[tokio::test]
async fn notion_verifier_rejects_uncommitted_or_lifecycle_inconsistent_material() {
    let state = test_state().await;
    let tenant_context = tenant("org-notion-rollback", "workspace-notion-rollback");
    let created = setup_notion_webhook(&state, "automation-notion-rollback", &tenant_context).await;
    let uncommitted_token = "notion_uncommitted_rollback_token";
    let material_key = crate::app::state::secret_material_key(&created.trigger.secret.secret_ref);
    state
        .automation_webhook_secret_material
        .write()
        .await
        .get_mut(&material_key)
        .expect("Notion secret material")
        .secret = uncommitted_token.to_string();

    let body = br#"{"type":"page.created"}"#;
    let signature = notion_automation_webhook_signature_header(uncommitted_token, body);
    let headers = crate::app::state::AutomationWebhookSignatureHeaders::default()
        .with_notion_signature(Some(&signature));
    let awaiting_error = state
        .verify_automation_webhook_request_with_headers(
            &created.trigger.public_path_token,
            headers.clone(),
            body,
            Some("notion-rollback-awaiting".to_string()),
            crate::now_ms(),
            300_000,
        )
        .await
        .expect_err("awaiting lifecycle must reject signed events");
    assert_eq!(
        awaiting_error,
        crate::app::state::AutomationWebhookVerificationError::ProviderSecretNotImported
    );

    state
        .automation_webhook_triggers
        .write()
        .await
        .get_mut(&created.trigger.trigger_id)
        .and_then(|trigger| trigger.notion_verification.as_mut())
        .expect("Notion verification state")
        .status = AutomationWebhookNotionVerificationStatus::TokenReceived;
    let digest_error = state
        .verify_automation_webhook_request_with_headers(
            &created.trigger.public_path_token,
            headers,
            body,
            Some("notion-rollback-digest".to_string()),
            crate::now_ms(),
            300_000,
        )
        .await
        .expect_err("uncommitted material digest must reject signed events");
    assert_eq!(
        digest_error,
        crate::app::state::AutomationWebhookVerificationError::MissingSecretMaterial
    );
}
