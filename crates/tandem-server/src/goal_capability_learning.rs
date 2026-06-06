//! Goal Capability Learning runtime: discovery, composition, and decision persistence.

mod decision_store;
mod discovery;

pub use decision_store::{DiscoveryDecision, GoalCapabilityLearningDecisionStore};
pub use discovery::discover_capabilities_for_goal;

#[cfg(test)]
mod tests;
