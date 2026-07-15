// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
#[serial_test::serial]
async fn incident_monitor_redacts_secrets_from_published_body_fields() {
    // TAN-540: free-text incident fields must be redacted before they are
    // stored and published to destinations, not only the safety-context metadata.
    let state = test_state().await;
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let draft = state
        .submit_incident_monitor_draft(crate::IncidentMonitorSubmission {
            source: Some("manual".to_string()),
            title: Some("Auth failed: Authorization: Bearer sk-live-TITLESECRET".to_string()),
            detail: Some("db connect failed password=hunter2 while retrying".to_string()),
            excerpt: vec!["stack line api_key=AKIAEXCERPTSECRET trailing".to_string()],
            evidence_refs: vec!["token=EVIDENCESECRETREF".to_string()],
            risk_level: Some("medium".to_string()),
            confidence: Some("medium".to_string()),
            ..Default::default()
        })
        .await
        .expect("draft");

    let title = draft.title.clone().unwrap_or_default();
    let detail = draft.detail.clone().unwrap_or_default();

    // Nothing secret survives into the stored (and later published) fields.
    assert!(!title.contains("sk-live-TITLESECRET"), "title leaked: {title}");
    assert!(!detail.contains("hunter2"), "detail leaked password: {detail}");
    assert!(
        !detail.contains("AKIAEXCERPTSECRET"),
        "assembled detail leaked excerpt secret: {detail}"
    );
    assert!(
        !detail.contains("EVIDENCESECRETREF"),
        "assembled detail leaked evidence secret: {detail}"
    );
    assert!(
        draft
            .evidence_refs
            .iter()
            .all(|reference| !reference.contains("EVIDENCESECRETREF")),
        "evidence_refs leaked secret: {:?}",
        draft.evidence_refs
    );

    // And the redaction marker is present where secrets were removed.
    assert!(title.contains("[redacted]"), "title not redacted: {title}");
    assert!(detail.contains("[redacted]"), "detail not redacted: {detail}");
}
