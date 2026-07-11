//! Backend-neutral memory schema migration vocabulary.
//!
//! This registry describes logical schema changes. Backend implementations are
//! responsible for translating them into their SQL dialect and for marking a
//! migration current only after that translation is complete.

mod registry;
mod sqlite;

pub use registry::*;
pub(crate) use sqlite::run_sqlite_migrations;
