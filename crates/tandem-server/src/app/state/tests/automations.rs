use super::*;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use tandem_types::{MessageRole, PrewriteCoverageMode, Session};

use crate::app::state::automation::collect_automation_attempt_receipt_events;
use crate::app::state::automation::node_output::{
    build_automation_attempt_evidence, build_automation_validator_summary,
    detect_automation_blocker_category, detect_automation_node_failure_kind,
    detect_automation_node_phase, detect_automation_node_status, wrap_automation_node_output,
};

mod brief_coverage;
mod brief_outcomes;
mod brief_validation;
mod integration;
mod prompting;
mod replay_suite;
mod runtime_paths;
mod structured_handoff;
mod telemetry;
mod tool_discovery;
mod validation;
mod validation_recovery;
mod workflow_policy;

include!("automations_parts/part01.rs");
include!("automations_parts/part02.rs");
include!("automations_parts/part03.rs");
