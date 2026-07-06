// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_types::ToolResult;

#[test]
fn build_issue_body_includes_hidden_markers() {
    let draft = IncidentMonitorDraftRecord {
        draft_id: "draft-1".to_string(),
        fingerprint: "abc123".to_string(),
        repo: "acme/platform".to_string(),
        status: "draft_ready".to_string(),
        created_at_ms: 1,
        triage_run_id: Some("triage-1".to_string()),
        issue_number: None,
        title: Some("session.error detected".to_string()),
        detail: Some("summary".to_string()),
        ..IncidentMonitorDraftRecord::default()
    };
    let body = build_issue_body(&draft, None, None, "digest-1");
    assert!(body.contains("<!-- tandem:fingerprint:v1:abc123 -->"));
    assert!(body.contains("<!-- tandem:evidence:v1:digest-1 -->"));
    assert!(body.contains("triage_run_id: triage-1"));
}

#[test]
fn github_copilot_issue_payloads_match_current_schema() {
    let list_payload = github_list_issues_payload("frumu-ai", "tandem", 1);
    assert_eq!(list_payload["owner"], "frumu-ai");
    assert_eq!(list_payload["repo"], "tandem");
    assert_eq!(list_payload["perPage"], 100);
    assert_eq!(list_payload["page"], 1);
    assert!(
        list_payload.get("state").is_none(),
        "GitHub Copilot MCP accepts OPEN/CLOSED only; omitting state lists both"
    );

    let get_payload = github_get_issue_payload("frumu-ai", "tandem", 68);
    assert_eq!(get_payload["method"], "get");
    assert_eq!(get_payload["owner"], "frumu-ai");
    assert_eq!(get_payload["repo"], "tandem");
    assert_eq!(get_payload["issue_number"], 68);
}

#[test]
fn build_issue_body_renders_incident_excerpt_as_fenced_logs() {
    let draft = IncidentMonitorDraftRecord {
        draft_id: "draft-logs".to_string(),
        fingerprint: "log-fingerprint".to_string(),
        repo: "acme/platform".to_string(),
        status: "draft_ready".to_string(),
        created_at_ms: 1,
        detail: Some("fallback detail".to_string()),
        ..IncidentMonitorDraftRecord::default()
    };
    let incident = IncidentMonitorIncidentRecord {
        incident_id: "incident-logs".to_string(),
        fingerprint: draft.fingerprint.clone(),
        event_type: "workflow.run.failed".to_string(),
        status: "triage_queued".to_string(),
        repo: draft.repo.clone(),
        workspace_root: "/tmp/acme".to_string(),
        title: "Workflow failed".to_string(),
        excerpt: vec![
            "first failure line".to_string(),
            "second failure line".to_string(),
        ],
        ..IncidentMonitorIncidentRecord::default()
    };
    let body = build_issue_body(&draft, Some(&incident), None, "digest-logs");
    assert!(body.contains("### Logs\n```\nfirst failure line\nsecond failure line\n```"));
    assert!(body.contains("incident_id: incident-logs"));
    assert!(body.contains("event_type: workflow.run.failed"));
}

#[test]
fn build_issue_body_renders_deduped_evidence_refs() {
    let draft = IncidentMonitorDraftRecord {
        draft_id: "draft-evidence".to_string(),
        fingerprint: "evidence-fingerprint".to_string(),
        repo: "acme/platform".to_string(),
        status: "draft_ready".to_string(),
        created_at_ms: 1,
        evidence_refs: vec![
            "artifacts/shared.json".to_string(),
            "artifacts/draft-only.log".to_string(),
        ],
        ..IncidentMonitorDraftRecord::default()
    };
    let incident = IncidentMonitorIncidentRecord {
        incident_id: "incident-evidence".to_string(),
        fingerprint: draft.fingerprint.clone(),
        event_type: "workflow.run.failed".to_string(),
        status: "triage_queued".to_string(),
        repo: draft.repo.clone(),
        workspace_root: "/tmp/acme".to_string(),
        title: "Workflow failed".to_string(),
        evidence_refs: vec![
            "artifacts/shared.json".to_string(),
            "artifacts/incident-only.log".to_string(),
        ],
        ..IncidentMonitorIncidentRecord::default()
    };
    let body = build_issue_body(&draft, Some(&incident), None, "digest-evidence");
    assert!(body.contains("### Evidence"));
    assert_eq!(body.matches("- artifacts/shared.json").count(), 1);
    assert!(body.contains("- artifacts/draft-only.log"));
    assert!(body.contains("- artifacts/incident-only.log"));
}

#[test]
fn build_issue_body_renders_only_present_diagnostic_metadata() {
    let draft = IncidentMonitorDraftRecord {
        draft_id: "draft-metadata".to_string(),
        fingerprint: "metadata-fingerprint".to_string(),
        repo: "acme/platform".to_string(),
        status: "draft_ready".to_string(),
        created_at_ms: 1,
        ..IncidentMonitorDraftRecord::default()
    };
    let incident = IncidentMonitorIncidentRecord {
        incident_id: "incident-metadata".to_string(),
        fingerprint: draft.fingerprint.clone(),
        event_type: "workflow.run.failed".to_string(),
        status: "triage_queued".to_string(),
        repo: draft.repo.clone(),
        workspace_root: "/tmp/acme".to_string(),
        title: "Workflow failed".to_string(),
        run_id: Some("run-1".to_string()),
        component: Some("automation_v2".to_string()),
        occurrence_count: 3,
        last_seen_at_ms: Some(1_777_485_515_668),
        ..IncidentMonitorIncidentRecord::default()
    };
    let body = build_issue_body(&draft, Some(&incident), None, "digest-metadata");
    assert!(body.contains("### Diagnostic metadata"));
    assert!(body.contains("run_id: run-1"));
    assert!(body.contains("component: automation_v2"));
    assert!(body.contains("occurrence_count: 3"));
    assert!(body.contains("last_seen_at_ms: 2026-04-29T"));
    assert!(!body.contains("session_id:"));
    assert!(!body.contains("correlation_id:"));
    assert!(!body.contains("level:"));
}

#[test]
fn build_issue_body_renders_fallback_triage_status_for_known_states() {
    let mut draft = IncidentMonitorDraftRecord {
        draft_id: "draft-status".to_string(),
        fingerprint: "status-fingerprint".to_string(),
        repo: "acme/platform".to_string(),
        status: "draft_ready".to_string(),
        created_at_ms: 1,
        github_status: Some("triage_timed_out".to_string()),
        confidence: Some("medium".to_string()),
        risk_level: Some("medium".to_string()),
        expected_destination: Some("incident_monitor_issue_draft".to_string()),
        quality_gate: Some(crate::types::IncidentMonitorQualityGateReport {
            stage: "draft_to_proposal".to_string(),
            status: "blocked".to_string(),
            passed: false,
            passed_count: 2,
            total_count: 4,
            gates: Vec::new(),
            missing: vec!["research_performed".to_string()],
            blocked_reason: Some("triage timed out".to_string()),
        }),
        ..IncidentMonitorDraftRecord::default()
    };
    let body = build_issue_body(&draft, None, None, "digest-status");
    assert!(body.contains("### Triage signal"));
    assert!(body.contains("confidence: medium"));
    assert!(body.contains("quality_gate_status: blocked"));
    assert!(body.contains("- research_performed"));
    assert!(body.contains("quality_gate_reason: triage timed out"));
    assert!(body.contains("triage_status: triage_timed_out"));

    draft.github_status = Some("issue_draft_ready".to_string());
    let body = build_issue_body(&draft, None, None, "digest-status");
    assert!(!body.contains("triage_status:"));
    draft.github_status = None;
    let body = build_issue_body(&draft, None, None, "digest-status");
    assert!(!body.contains("triage_status:"));
}

#[test]
fn build_issue_body_truncates_long_excerpt() {
    let draft = IncidentMonitorDraftRecord {
        draft_id: "draft-long".to_string(),
        fingerprint: "long-fingerprint".to_string(),
        repo: "acme/platform".to_string(),
        status: "draft_ready".to_string(),
        created_at_ms: 1,
        ..IncidentMonitorDraftRecord::default()
    };
    let incident = IncidentMonitorIncidentRecord {
        incident_id: "incident-long".to_string(),
        fingerprint: draft.fingerprint.clone(),
        event_type: "workflow.run.failed".to_string(),
        status: "triage_queued".to_string(),
        repo: draft.repo.clone(),
        workspace_root: "/tmp/acme".to_string(),
        title: "Workflow failed".to_string(),
        excerpt: vec!["x".repeat(8_000)],
        ..IncidentMonitorIncidentRecord::default()
    };
    let body = build_issue_body(&draft, Some(&incident), None, "digest-long");
    assert!(body.len() < 12_500);
    assert!(body.contains("<!-- tandem:evidence:v1:digest-long -->"));
}

#[test]
fn extract_issues_from_official_github_mcp_result() {
    let result = ToolResult {
        output: String::new(),
        metadata: json!({
            "result": {
                "issues": [
                    {
                        "number": 42,
                        "title": "Incident Monitor issue",
                        "body": "details\n<!-- tandem:fingerprint:v1:deadbeef -->",
                        "state": "open",
                        "html_url": "https://github.com/acme/platform/issues/42"
                    }
                ]
            }
        }),
    };
    let issues = extract_issues_from_tool_result(&result);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 42);
    assert_eq!(issues[0].state, "open");
    assert!(issues[0].body.contains("deadbeef"));
}

#[test]
fn failed_create_posts_suppress_unsafe_create_retries() {
    let draft = IncidentMonitorDraftRecord {
        draft_id: "draft-1".to_string(),
        repo: "acme/source".to_string(),
        fingerprint: "fp-create".to_string(),
        ..Default::default()
    };
    let destination_id = INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID;
    let target_repo = "acme/incidents";
    let failed_create = IncidentMonitorPostRecord {
        post_id: "post-create".to_string(),
        repo: target_repo.to_string(),
        fingerprint: draft.fingerprint.clone(),
        operation: "create_issue".to_string(),
        status: "failed".to_string(),
        destination_id: Some(destination_id.to_string()),
        target_ref: Some(target_repo.to_string()),
        ..Default::default()
    };
    let failed_auto_post = IncidentMonitorPostRecord {
        operation: "auto_post".to_string(),
        ..failed_create.clone()
    };
    let failed_preflight_auto_post = IncidentMonitorPostRecord {
        operation: "auto_post".to_string(),
        error: Some(
            "Selected provider/model is unavailable. Incident Monitor is fail-closed.".to_string(),
        ),
        ..failed_create.clone()
    };
    let failed_comment = IncidentMonitorPostRecord {
        operation: "comment".to_string(),
        ..failed_create.clone()
    };
    let posted_create = IncidentMonitorPostRecord {
        status: "posted".to_string(),
        ..failed_create.clone()
    };
    let different_fingerprint = IncidentMonitorPostRecord {
        fingerprint: "other-fingerprint".to_string(),
        ..failed_create.clone()
    };
    let different_destination = IncidentMonitorPostRecord {
        destination_id: Some("github-secondary".to_string()),
        ..failed_create.clone()
    };

    assert!(failed_post_suppresses_create(
        &draft,
        &failed_create,
        destination_id,
        target_repo
    ));
    assert!(failed_post_suppresses_create(
        &draft,
        &failed_auto_post,
        destination_id,
        target_repo
    ));
    assert!(!failed_post_suppresses_create(
        &draft,
        &failed_preflight_auto_post,
        destination_id,
        target_repo
    ));
    assert!(!failed_post_suppresses_create(
        &draft,
        &failed_comment,
        destination_id,
        target_repo
    ));
    assert!(!failed_post_suppresses_create(
        &draft,
        &posted_create,
        destination_id,
        target_repo
    ));
    assert!(!failed_post_suppresses_create(
        &draft,
        &different_fingerprint,
        destination_id,
        target_repo
    ));
    assert!(!failed_post_suppresses_create(
        &draft,
        &different_destination,
        destination_id,
        target_repo
    ));
    assert!(!failed_post_suppresses_create(
        &draft,
        &failed_create,
        destination_id,
        "acme/other-incidents"
    ));
}

#[test]
fn post_target_repo_matching_prefers_target_ref() {
    let legacy_post = IncidentMonitorPostRecord {
        repo: "acme/platform".to_string(),
        ..Default::default()
    };
    assert!(post_matches_target_repo(&legacy_post, "acme/platform"));
    assert!(!post_matches_target_repo(&legacy_post, "acme/incidents"));

    let routed_post = IncidentMonitorPostRecord {
        repo: "acme/source".to_string(),
        target_ref: Some("acme/incidents".to_string()),
        ..Default::default()
    };
    assert!(post_matches_target_repo(&routed_post, "acme/incidents"));
    assert!(!post_matches_target_repo(&routed_post, "acme/source"));
}

#[test]
fn idempotency_key_is_destination_aware() {
    let legacy = build_idempotency_key(
        INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID,
        "acme/platform",
        "fp-create",
        "create_issue",
        "digest",
    );
    let secondary = build_idempotency_key(
        "github-secondary",
        "acme/platform",
        "fp-create",
        "create_issue",
        "digest",
    );

    assert_ne!(legacy, secondary);
}

#[test]
fn fallback_issue_body_can_explain_triage_enrichment_pending() {
    assert_eq!(
        fallback_issue_triage_status(Some("triage_enrichment_pending_fallback_publish")),
        Some("triage_enrichment_pending_fallback_publish")
    );
}

#[test]
fn publish_not_ready_reason_does_not_blame_model_when_github_is_ready() {
    let status = IncidentMonitorStatus {
        last_error: Some(
            "Selected provider/model is unavailable. Incident Monitor is fail-closed.".to_string(),
        ),
        readiness: crate::types::IncidentMonitorReadiness {
            repo_valid: true,
            mcp_connected: true,
            github_read_ready: true,
            github_write_ready: true,
            selected_model_ready: false,
            publish_ready: true,
            runtime_ready: false,
            ..Default::default()
        },
        ..Default::default()
    };

    assert_eq!(
        incident_monitor_publish_not_ready_reason(&status),
        "Incident Monitor is not ready for GitHub posting"
    );
}

fn make_incident_with_excerpt_and_last_error(
    excerpt: Vec<String>,
    last_error: Option<String>,
) -> IncidentMonitorIncidentRecord {
    IncidentMonitorIncidentRecord {
        incident_id: "incident-pick".to_string(),
        fingerprint: "fp".to_string(),
        event_type: "automation_v2.run.failed".to_string(),
        status: "queued".to_string(),
        repo: "acme/platform".to_string(),
        workspace_root: "/tmp/example".to_string(),
        title: "Workflow X failed at Y: automation run blocked by upstream node outcome"
            .to_string(),
        excerpt,
        last_error,
        ..Default::default()
    }
}

#[test]
fn pick_error_message_for_provenance_prefers_excerpt_over_last_error_after_timeout() {
    // Mirror the post-timeout state: last_error is the multi-line
    // diagnostic; the original failure literal lives on incident.excerpt.
    let incident = make_incident_with_excerpt_and_last_error(
        vec!["automation run blocked by upstream node outcome".to_string()],
        Some(
            "triage run X did not reach a terminal status within 300000ms\nelapsed_ms: 301053\n"
                .to_string(),
        ),
    );
    let draft = IncidentMonitorDraftRecord::default();
    let picked = pick_error_message_for_provenance(&draft, Some(&incident));
    assert_eq!(
        picked.as_deref(),
        Some("automation run blocked by upstream node outcome")
    );
}

#[test]
fn pick_error_message_for_provenance_falls_back_to_title_suffix() {
    let incident = make_incident_with_excerpt_and_last_error(Vec::new(), None);
    let draft = IncidentMonitorDraftRecord::default();
    let picked = pick_error_message_for_provenance(&draft, Some(&incident));
    assert_eq!(
        picked.as_deref(),
        Some("automation run blocked by upstream node outcome")
    );
}

#[test]
fn extract_error_after_colon_keeps_short_titles_intact() {
    // Too short to be a useful suffix → return None so caller falls
    // back to the full title.
    assert!(extract_error_after_colon("Something: short").is_none());
    assert!(extract_error_after_colon("Just one part with no colon").is_none());
}

#[test]
fn extract_error_after_colon_uses_rightmost_split() {
    let title = "Workflow auto-v2-foo failed at bar: real error message goes here";
    assert_eq!(
        extract_error_after_colon(title).as_deref(),
        Some("real error message goes here")
    );
}

/// Stability regressions: digest must not move on
/// `occurrence_count` (#45/#46), `triage_run_id` (#69-#194 spam,
/// recreated for stale/blocked triages), or `incident.run_id` /
/// `session_id` (redundant with fingerprint). Sanity: still moves
/// on a real fingerprint change.
#[test]
fn compute_evidence_digest_stability_contract() {
    let base = IncidentMonitorDraftRecord {
        repo: "frumu-ai/tandem".to_string(),
        fingerprint: "abc123".to_string(),
        title: Some("Failure".to_string()),
        detail: Some("reason: foo".to_string()),
        triage_run_id: Some("triage-1".to_string()),
        ..Default::default()
    };
    let baseline = compute_evidence_digest(&base, None);
    let mk_inc = |run, sess, count| IncidentMonitorIncidentRecord {
        run_id: Some(String::from(run)),
        session_id: Some(String::from(sess)),
        occurrence_count: count,
        ..Default::default()
    };
    assert_eq!(
        baseline,
        compute_evidence_digest(&base, Some(&mk_inc("r", "s", 1)))
    );
    assert_eq!(
        baseline,
        compute_evidence_digest(&base, Some(&mk_inc("r", "s", 99)))
    );
    assert_eq!(
        baseline,
        compute_evidence_digest(&base, Some(&mk_inc("r2", "s2", 1)))
    );
    let mut recreated = base.clone();
    recreated.triage_run_id = Some("triage-2".to_string());
    assert_eq!(baseline, compute_evidence_digest(&recreated, None));
    let mut other_fp = base.clone();
    other_fp.fingerprint = "fingerprint-B".to_string();
    assert_ne!(baseline, compute_evidence_digest(&other_fp, None));
}
