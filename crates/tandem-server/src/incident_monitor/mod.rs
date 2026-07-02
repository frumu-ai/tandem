pub use tandem_incident_monitor::{comment_summary, error_provenance, log_parser, types};
pub mod log_artifacts;
pub mod log_watcher;
pub mod router;
pub mod safety_context;
pub mod service;
pub mod source_readiness;

pub(crate) fn source_identity_matches_draft(
    draft: &crate::IncidentMonitorDraftRecord,
    submission: &crate::IncidentMonitorSubmission,
) -> bool {
    let draft_project = draft.project_id.as_deref();
    let draft_source = draft.log_source_id.as_deref();
    let submission_project = submission.project_id.as_deref();
    let submission_source = submission.log_source_id.as_deref();
    let source_bound = draft_project.is_some()
        || draft_source.is_some()
        || submission_project.is_some()
        || submission_source.is_some();
    !source_bound || (draft_project == submission_project && draft_source == submission_source)
}
