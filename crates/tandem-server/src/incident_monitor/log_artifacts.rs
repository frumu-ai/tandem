use crate::{AppState, IncidentMonitorLogCandidate};

pub async fn write_log_evidence_artifact(
    state: &AppState,
    candidate: &IncidentMonitorLogCandidate,
) -> anyhow::Result<String> {
    tandem_incident_monitor::log_artifacts::write_log_evidence_artifact(
        &state.incident_monitor_log_evidence_dir,
        candidate,
    )
    .await
}
