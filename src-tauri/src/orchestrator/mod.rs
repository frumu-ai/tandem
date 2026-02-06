// Tandem Multi-Agent Orchestration Module
// Coordinates specialized sub-agents to accomplish complex objectives
// See: docs/orchestration_plan.md

pub mod agents;
pub mod budget;
pub mod engine;
pub mod locks;
pub mod policy;
pub mod scheduler;
pub mod store;
pub mod types;

pub use budget::BudgetTracker;
pub use engine::OrchestratorEngine;
pub use locks::PathLockManager;
pub use policy::PolicyEngine;
pub use scheduler::TaskScheduler;
pub use store::OrchestratorStore;
pub use types::*;

#[cfg(test)]
mod concurrency_tests;
