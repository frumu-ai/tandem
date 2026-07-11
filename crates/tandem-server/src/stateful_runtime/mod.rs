pub mod adapters;
pub mod definition;
mod durable_io;
pub mod orchestration_store;
mod outbox_reconcile;
pub mod phases;
pub mod reliability;
mod reliability_retention;
mod reliability_retry;
pub mod scheduler;
mod sqlite_compat;
pub mod store;
pub mod types;
pub mod waits;

pub use adapters::{
    automation_status_to_stateful, stateful_run_from_automation_v2, stateful_run_from_workflow,
    workflow_status_to_stateful,
};
pub use definition::{
    automation_definition_snapshot_hash, automation_definition_version,
    automation_run_definition_fields, automation_run_definition_metadata,
    automation_run_definition_snapshot_hash_mismatch, ensure_automation_run_definition_metadata,
    stable_definition_snapshot_hash, stamp_automation_run_definition_metadata,
};
pub use orchestration_store::{
    AtomicHandoffCommit, GoalCancellationResult, GoalControlOutcome, GoalEventRow,
    GoalPauseOutcome, GoalResumeOutcome, GovernedTransitionRequest, GovernedTransitionResult,
    LegacyImportContext, LegacyRuntimeMigrationPaths, LegacyRuntimeMigrationReport,
    OrchestrationStateStore, OrchestrationStorePaths, OrchestrationTransitionAuthority,
    StartGoalOutcome, StatefulEngineLock, WorkflowCompletionResult, DRAFT_CONCURRENCY_CONFLICT,
    ORCHESTRATION_DRAFT_VERSION,
};
pub use phases::*;
pub use reliability::{
    execute_stateful_compensation, list_stateful_compensations, list_stateful_dead_letters,
    list_stateful_outbox, list_stateful_tool_effects, load_stateful_reliability,
    mark_compensation_status, mark_dead_letter_disposition, operator_principal,
    record_external_action_reliability_bridge, stateful_reliability_path_from_runtime_events_path,
    upsert_stateful_compensation, upsert_stateful_dead_letter, upsert_stateful_outbox,
    upsert_stateful_tool_effect, StatefulCompensationExecutionResult, StatefulCompensationRecord,
    StatefulCompensationStatus, StatefulDeadLetterRecord, StatefulDeadLetterStatus,
    StatefulOutboxRecord, StatefulOutboxStatus, StatefulRecoveryOption, StatefulReliabilityQuery,
    StatefulReliabilityStoragePaths, StatefulReliabilityStoreFile, StatefulToolEffectRecord,
    StatefulToolEffectStatus,
};
pub use reliability_retention::prune_stateful_reliability_store;
pub use reliability_retry::{
    dead_letter_retry_dispatch_count, dead_letter_retry_dispatched_at_ms,
    dead_letter_superseded_by_success, mark_dead_letter_retry_dispatched,
};
pub use scheduler::{
    process_due_stateful_waits, StatefulWaitSchedulerConfig, StatefulWaitSchedulerOutcome,
    StatefulWaitSchedulerTick,
};
pub use store::{
    append_stateful_run_event, append_stateful_run_event_once,
    append_stateful_run_event_once_with_next_seq, compact_stateful_run_event_log,
    list_stateful_run_snapshots, load_stateful_run_events, next_stateful_run_event_seq,
    prune_stateful_run_snapshots, query_stateful_run_events, read_stateful_run_snapshot,
    read_stateful_run_snapshot_for_run, stateful_run_event_compacted_event_ids,
    stateful_run_event_seq_by_id, stateful_run_snapshot_path, write_stateful_run_snapshot,
    StatefulRunEventQuery, StatefulRuntimeStoragePaths,
};
pub use types::*;
pub use waits::{
    begin_claimed_stateful_wait_reminder_completion,
    begin_claimed_stateful_wait_timeout_completion, begin_claimed_stateful_wait_wake_completion,
    cancel_stateful_wait_after_phase_guard_denial, claim_due_stateful_wait,
    claim_matching_stateful_webhook_wait, claim_stateful_wait_for_resolution, due_stateful_waits,
    finish_claimed_stateful_wait_completion, finish_claimed_stateful_wait_reminder_completion,
    list_stateful_waits, load_stateful_waits, mark_stateful_wait_timeout_result,
    mark_stateful_wait_woken, prune_stateful_wait_store, release_claimed_stateful_wait,
    stateful_webhook_wait_match_from_metadata, stateful_webhook_wait_metadata,
    upsert_stateful_wait, wait_matches_webhook_event, StatefulWaitQuery,
};
