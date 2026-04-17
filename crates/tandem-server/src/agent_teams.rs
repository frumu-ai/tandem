use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use tandem_core::{
    any_policy_matches, SpawnAgentHook, SpawnAgentToolContext, SpawnAgentToolResult,
    ToolPolicyContext, ToolPolicyDecision, ToolPolicyHook,
};

include!("agent_teams_parts/part01.rs");
include!("agent_teams_parts/part02.rs");
