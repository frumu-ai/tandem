// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
async fn public_automation_webhook_allows_configured_browser_preflight_headers() {
    let state = test_state().await;
    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/webhooks/automations/whpub_preflight")
                .header("origin", "http://localhost:3000")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "content-type,x-tandem-webhook-secret,x-tandem-webhook-signature,x-tandem-webhook-event-id",
                )
                .body(Body::empty())
                .expect("preflight request"),
        )
        .await
        .expect("preflight response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("http://localhost:3000")
    );
    let allowed_headers = response
        .headers()
        .get("access-control-allow-headers")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    for expected in [
        "content-type",
        "x-tandem-webhook-secret",
        "x-tandem-webhook-signature",
        "x-tandem-webhook-event-id",
    ] {
        assert!(
            allowed_headers
                .split(",")
                .any(|value| value.trim() == expected),
            "missing allowed preflight header {expected}: {allowed_headers}"
        );
    }
}
