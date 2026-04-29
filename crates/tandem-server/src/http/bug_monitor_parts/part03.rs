/// True if the triage run has reached a terminal status (`Failed` /
/// `Completed` / `Cancelled`). Returns `false` if the run record can
/// not be loaded — a missing/corrupt run is treated as non-terminal so
/// the deadline task proceeds to mark the draft `triage_timed_out` and
/// `publish_draft` falls through to the basic issue body. Returning
/// `true` here would short-circuit the deadline task before it sets
/// `triage_timed_out_at_ms`, leaving the draft stuck in
/// `triage_pending` indefinitely.
///
/// Lives in part03 (rather than alongside other Bug Monitor helpers in
/// part01) so that adding this function does not touch the already-
/// over-the-line-cap part01.rs.
pub(crate) async fn bug_monitor_triage_run_is_terminal(state: &AppState, run_id: &str) -> bool {
    match load_context_run_state(state, run_id).await {
        Ok(run) => super::context_runs::context_run_is_terminal(&run.status),
        Err(_) => false,
    }
}
