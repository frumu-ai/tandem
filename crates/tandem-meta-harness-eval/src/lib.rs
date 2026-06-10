//! Evaluation data models for Tandem's meta-harness.
//!
//! `tandem-meta-harness-eval` owns the durable trace capture/replay model and
//! the scored workflow/version evaluation model used by the meta-harness.  The
//! crate intentionally stays at the data-model boundary: it describes replayable
//! traces, stable workflow/version identities, score dimensions, and the records
//! needed to compare evaluated versions deterministically. Stable trace
//! identifiers, sequence ordering, timestamps, and metadata are included here so
//! later replay can be deterministic without coupling evaluation fixtures to live
//! product services, runtime orchestration, or issue-tracking systems.
//!
//! This crate does **not** own live orchestration, GitHub issue tracking
//! automation, or product runtime behavior.  The old GitHub tracking issue task
//! for this roadmap item is ignored here because Linear is the superseding
//! tracking source.

pub mod scoring;
pub mod trace;

pub use scoring::{ScoreDimension, ScoreValue, ScoredWorkflowVersion, VersionId, WorkflowId};
pub use trace::{Trace, TraceEvent, TraceEventId, TraceMetadata, TraceStep, TraceStepId};
