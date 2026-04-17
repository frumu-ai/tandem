#![allow(dead_code, clippy::all)]

pub mod agents;
pub mod cancellation;
pub mod config;
pub mod engine_api_token;
pub mod engine_loop;
pub mod event_bus;
pub mod hooks;
mod message_part_reducer;
pub mod mutation_checkpoints;
pub mod permission_defaults;
pub mod permissions;
pub mod plugins;
pub mod provider_auth_store;
pub mod session_title;
pub mod storage;
pub mod storage_paths;
pub mod tool_capabilities;
pub mod tool_effect_ledger;
pub mod tool_policy;
pub mod tool_router;

pub const DEFAULT_ENGINE_HOST: &str = "127.0.0.1";
pub const DEFAULT_ENGINE_PORT: u16 = 39731;

pub use agents::*;
pub use cancellation::*;
pub use config::*;
pub use engine_api_token::*;
pub use engine_loop::*;
pub use event_bus::*;
pub use mutation_checkpoints::*;
pub use permission_defaults::*;
pub use permissions::*;
pub use plugins::*;
pub use provider_auth_store::*;
pub use session_title::*;
pub use storage::*;
pub use storage_paths::*;
pub use tool_capabilities::*;
pub use tool_effect_ledger::*;
pub use tool_policy::*;
