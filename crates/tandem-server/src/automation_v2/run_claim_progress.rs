// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use tandem_automation::{AutomationRunExecutionClaim, AutomationV2RunRecord};

// The native execution claim lease protects launch, not the duration of work
// already started. Callers separately check status and the current claim's
// identity/epoch; this predicate is only progress evidence from that run.
pub(crate) fn run_has_execution_progress(
    run: &AutomationV2RunRecord,
    claim: &AutomationRunExecutionClaim,
) -> bool {
    !run.active_session_ids.is_empty()
        || !run.active_instance_ids.is_empty()
        || run.checkpoint.lifecycle_history.iter().any(|record| {
            record.recorded_at_ms >= claim.claimed_at_ms
                && !matches!(
                    record.event.as_str(),
                    "run_execution_claimed" | "run_execution_claim_expired_requeued"
                )
        })
}
