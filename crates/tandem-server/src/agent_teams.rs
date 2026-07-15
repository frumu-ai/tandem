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
    any_policy_matches, tool_risk_tier_from_name_and_descriptor, SpawnAgentHook,
    SpawnAgentToolContext, SpawnAgentToolResult, ToolPolicyContext, ToolPolicyDecision,
    ToolPolicyHook, FINTECH_STRICT_PROFILE,
};
use tandem_types::{
    DataClass, GateRequest, PolicyDecisionEffect, PolicyDecisionRecord, ToolRiskTier,
    ToolSecurityDescriptor,
};

include!("agent_teams_parts/egress_preflight.rs");
include!("agent_teams_parts/enterprise_authored_policy.rs");
include!("agent_teams_parts/phase_tool_policy.rs");
include!("agent_teams_parts/part01.rs");
include!("agent_teams_parts/action_gate_approval.rs");
include!("agent_teams_parts/part03.rs");
include!("agent_teams_parts/part02.rs");
