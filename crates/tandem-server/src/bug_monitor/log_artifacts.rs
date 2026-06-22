use crate::{AppState, BugMonitorLogCandidate};

pub async fn write_log_evidence_artifact(
    state: &AppState,
    candidate: &BugMonitorLogCandidate,
) -> anyhow::Result<String> {
    tandem_bug_monitor::log_artifacts::write_log_evidence_artifact(
        &state.bug_monitor_log_evidence_dir,
        candidate,
    )
    .await
}
