// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::{
    incident_monitor::service::recover_overdue_incident_monitor_triage_runs,
    IncidentMonitorApprovalPolicy, IncidentMonitorConfig, IncidentMonitorDraftRecord,
    IncidentMonitorIncidentRecord, IncidentMonitorLogSource, IncidentMonitorMonitoredProject,
};

use super::{test_state_with_path, tmp_resource_file};

fn incident_monitor_recovery_state(name: &str) -> crate::app::state::AppState {
    let mut state = test_state_with_path(tmp_resource_file(name));
    state.incident_monitor_config_path = tmp_resource_file(&format!("{name}-config"));
    state.incident_monitor_drafts_path = tmp_resource_file(&format!("{name}-drafts"));
    state.incident_monitor_incidents_path = tmp_resource_file(&format!("{name}-incidents"));
    state.incident_monitor_posts_path = tmp_resource_file(&format!("{name}-posts"));
    state.automation_v2_runs_path = tmp_resource_file(&format!("{name}-runs"));
    state
}

async fn ready_incident_monitor_recovery_state(name: &str) -> crate::app::state::AppState {
    let mut state = super::ready_test_state().await;
    state.incident_monitor_config_path = tmp_resource_file(&format!("{name}-config"));
    state.incident_monitor_drafts_path = tmp_resource_file(&format!("{name}-drafts"));
    state.incident_monitor_incidents_path = tmp_resource_file(&format!("{name}-incidents"));
    state.incident_monitor_posts_path = tmp_resource_file(&format!("{name}-posts"));
    state.automation_v2_runs_path = tmp_resource_file(&format!("{name}-runs"));
    state
}

fn portable_incident_monitor_recovery_workspace_root() -> String {
    let root = std::env::temp_dir().join("tandem-incident-monitor-recovery");
    std::fs::create_dir_all(root.join("logs"))
        .expect("portable incident monitor recovery workspace");
    root.display().to_string()
}

fn timed_out_draft(draft_id: &str, triage_run_id: &str) -> IncidentMonitorDraftRecord {
    IncidentMonitorDraftRecord {
        draft_id: draft_id.to_string(),
        fingerprint: "fingerprint-recovery".to_string(),
        repo: "frumu-ai/tandem".to_string(),
        status: "draft_ready".to_string(),
        created_at_ms: 1,
        triage_run_id: Some(triage_run_id.to_string()),
        title: Some("Failure detected in automation_v2.run.failed".to_string()),
        detail: Some("original workflow failure detail".to_string()),
        github_status: Some("triage_timed_out".to_string()),
        last_post_error: Some("triage run timed out before publishing".to_string()),
        ..Default::default()
    }
}

fn incident_for_draft(
    incident_id: &str,
    draft_id: &str,
    triage_run_id: &str,
) -> IncidentMonitorIncidentRecord {
    IncidentMonitorIncidentRecord {
        incident_id: incident_id.to_string(),
        fingerprint: "fingerprint-recovery".to_string(),
        event_type: "automation_v2.run.failed".to_string(),
        status: "triage_timed_out".to_string(),
        repo: "frumu-ai/tandem".to_string(),
        workspace_root: portable_incident_monitor_recovery_workspace_root(),
        title: "Failure detected in automation_v2.run.failed".to_string(),
        occurrence_count: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        draft_id: Some(draft_id.to_string()),
        triage_run_id: Some(triage_run_id.to_string()),
        ..Default::default()
    }
}

#[tokio::test]
async fn overdue_recovery_retries_unposted_timed_out_triage_drafts() {
    let state = incident_monitor_recovery_state("incident-monitor-retry-timed-out-draft");
    state
        .put_incident_monitor_config(IncidentMonitorConfig {
            enabled: true,
            paused: false,
            repo: Some("frumu-ai/tandem".to_string()),
            triage_timeout_ms: Some(0),
            ..Default::default()
        })
        .await
        .expect("put incident monitor config");

    let draft_id = "failure-draft-retry-timed-out";
    let triage_run_id = "automation-v2-run-retry-timed-out";
    let incident_id = "failure-incident-retry-timed-out";
    state
        .put_incident_monitor_draft(timed_out_draft(draft_id, triage_run_id))
        .await
        .expect("put timed out draft");
    state
        .put_incident_monitor_incident(incident_for_draft(incident_id, draft_id, triage_run_id))
        .await
        .expect("put incident");

    let recovered = recover_overdue_incident_monitor_triage_runs(&state)
        .await
        .expect("recover overdue triage");

    assert_eq!(
        recovered,
        vec![(draft_id.to_string(), Some(incident_id.to_string()))]
    );
}

#[tokio::test]
async fn overdue_recovery_skips_timed_out_triage_drafts_with_github_issue() {
    let state = incident_monitor_recovery_state("incident-monitor-skip-posted-timed-out-draft");
    state
        .put_incident_monitor_config(IncidentMonitorConfig {
            enabled: true,
            paused: false,
            repo: Some("frumu-ai/tandem".to_string()),
            triage_timeout_ms: Some(0),
            ..Default::default()
        })
        .await
        .expect("put incident monitor config");

    let mut draft = timed_out_draft(
        "failure-draft-posted-timed-out",
        "automation-v2-run-posted-timed-out",
    );
    draft.issue_number = Some(68);
    draft.github_issue_url = Some("https://github.com/frumu-ai/tandem/issues/68".to_string());
    state
        .put_incident_monitor_draft(draft)
        .await
        .expect("put posted timed out draft");

    let recovered = recover_overdue_incident_monitor_triage_runs(&state)
        .await
        .expect("recover overdue triage");

    assert!(recovered.is_empty());
}

#[tokio::test]
async fn recovery_publish_honors_source_approval_policy() {
    let state =
        ready_incident_monitor_recovery_state("incident-monitor-recovery-router-source-approval")
            .await;
    state
        .put_incident_monitor_config(IncidentMonitorConfig {
            enabled: true,
            paused: false,
            repo: Some("frumu-ai/tandem".to_string()),
            triage_timeout_ms: Some(0),
            monitored_projects: vec![IncidentMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "frumu-ai/tandem".to_string(),
                workspace_root: portable_incident_monitor_recovery_workspace_root(),
                log_sources: vec![IncidentMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    approval_policy: IncidentMonitorApprovalPolicy::Always,
                    ..IncidentMonitorLogSource::default()
                }],
                ..IncidentMonitorMonitoredProject::default()
            }],
            ..Default::default()
        })
        .await
        .expect("put incident monitor config");

    let draft_id = "failure-draft-source-approval";
    let triage_run_id = "automation-v2-run-source-approval";
    let incident_id = "failure-incident-source-approval";
    let mut draft = timed_out_draft(draft_id, triage_run_id);
    draft.project_id = Some("payments".to_string());
    draft.log_source_id = Some("ci".to_string());
    state
        .put_incident_monitor_draft(draft)
        .await
        .expect("put timed out draft");

    let mut incident = incident_for_draft(incident_id, draft_id, triage_run_id);
    incident.project_id = Some("payments".to_string());
    incident.log_source_id = Some("ci".to_string());
    state
        .put_incident_monitor_incident(incident)
        .await
        .expect("put incident");

    let outcome = crate::app::tasks::publish_incident_monitor_recovery_draft(
        &state,
        draft_id.to_string(),
        Some(incident_id.to_string()),
    )
    .await
    .expect("publish recovery draft through router");

    assert_eq!(outcome.action, "approval_required");
    assert!(outcome.post.is_none());
    let stored = state
        .get_incident_monitor_draft(draft_id)
        .await
        .expect("stored draft");
    assert_eq!(stored.status, "approval_required");
    assert_eq!(stored.github_status.as_deref(), Some("approval_required"));
}

#[tokio::test]
async fn overdue_recovery_backs_off_between_attempts_but_stays_retryable() {
    // TAN-554: a still-unpublished timed-out draft must not be re-surfaced (and
    // re-emit publish/probe events) on every sweep. Instead it backs off
    // exponentially between attempts, but a transient failure stays retryable —
    // it is never permanently abandoned after a fixed attempt cap.
    let state = incident_monitor_recovery_state("incident-monitor-recovery-backoff");
    state
        .put_incident_monitor_config(IncidentMonitorConfig {
            enabled: true,
            paused: false,
            repo: Some("frumu-ai/tandem".to_string()),
            triage_timeout_ms: Some(0),
            ..Default::default()
        })
        .await
        .expect("put incident monitor config");

    let draft_id = "failure-draft-backoff";
    let triage_run_id = "automation-v2-run-backoff";
    let incident_id = "failure-incident-backoff";
    state
        .put_incident_monitor_draft(timed_out_draft(draft_id, triage_run_id))
        .await
        .expect("put timed out draft");
    state
        .put_incident_monitor_incident(incident_for_draft(incident_id, draft_id, triage_run_id))
        .await
        .expect("put incident");

    // First sweep re-surfaces the draft and schedules the next attempt in the
    // future (exponential backoff), recording one attempt.
    let recovered = recover_overdue_incident_monitor_triage_runs(&state)
        .await
        .expect("recover overdue triage");
    assert_eq!(
        recovered,
        vec![(draft_id.to_string(), Some(incident_id.to_string()))]
    );
    let draft = state
        .get_incident_monitor_draft(draft_id)
        .await
        .expect("draft still present");
    assert_eq!(draft.recovery_attempts, 1);
    assert!(
        draft.next_recovery_at_ms.is_some(),
        "a re-surfaced draft must schedule its next attempt"
    );

    // An immediate second sweep is inside the backoff window, so the draft is
    // not re-surfaced (no churn) and the attempt count is unchanged.
    let recovered = recover_overdue_incident_monitor_triage_runs(&state)
        .await
        .expect("recover overdue triage");
    assert!(
        recovered.is_empty(),
        "a draft inside its backoff window must not be re-surfaced"
    );
    assert_eq!(
        state
            .get_incident_monitor_draft(draft_id)
            .await
            .expect("draft still present")
            .recovery_attempts,
        1
    );

    // Once the backoff elapses (simulated by clearing the schedule), the draft
    // is retried again — it is never permanently abandoned. The attempt count
    // keeps climbing so the backoff continues to widen.
    let mut due = draft.clone();
    due.next_recovery_at_ms = Some(0);
    state
        .put_incident_monitor_draft(due)
        .await
        .expect("mark draft due for retry");
    let recovered = recover_overdue_incident_monitor_triage_runs(&state)
        .await
        .expect("recover overdue triage");
    assert_eq!(
        recovered,
        vec![(draft_id.to_string(), Some(incident_id.to_string()))],
        "a draft past its backoff window stays retryable"
    );
    assert_eq!(
        state
            .get_incident_monitor_draft(draft_id)
            .await
            .expect("draft still present")
            .recovery_attempts,
        2
    );
}
