//! Run/task mutability predicate (Invariant 3 of `docs/SPINE.md`).
//!
//! Whether a run/task accepts a mutation (retry, continue, requeue, repair,
//! claim) must be a single derived function of the run + task FSM state.
//! Today the UI infers this from raw status strings and falls back to a
//! client-side `withAutoPauseRetry` helper (commit `326c910`) when the
//! engine returns 409 with `AUTOMATION_V2_RUN_TASK_NOT_MUTABLE` /
//! `AUTOMATION_V2_RUN_NOT_RECOVERABLE`. Phase 3 collapses both sides to
//! this module.
//!
//! TODO(spine, phase-3):
//!   * Define
//!     ```ignore
//!     pub struct RunMutability {
//!         pub can_retry: bool,
//!         pub can_continue: bool,
//!         pub can_requeue: bool,
//!         pub can_repair: bool,
//!         pub can_claim: bool,
//!         pub reason: Option<NotMutableReason>,
//!     }
//!     ```
//!   * Define `pub fn mutability(run: &AutomationRun, task: &AutomationTask)
//!     -> RunMutability` as a pure function over `AutomationRunStatus` /
//!     task state (see `automation_v2/types.rs:879`).
//!   * Surface the derived booleans on the wire types so the UI consumes
//!     them directly; remove `withAutoPauseRetry` from
//!     `MyAutomationsContainer.tsx`.
//!   * Property test: every `(AutomationRunStatus, task_state)` tuple maps
//!     to one `RunMutability`; no panics.
