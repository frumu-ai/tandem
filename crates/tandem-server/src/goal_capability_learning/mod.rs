//! Goal Capability Learning runtime: discovers and composes capabilities to reach goals.

mod discovery;
mod fixtures;

pub use discovery::discover_capabilities_for_goal;

#[cfg(test)]
mod tests;
