use super::*;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use tandem_types::{MessageRole, PrewriteCoverageMode, Session};

use crate::app::state::automation::collect_automation_attempt_receipt_events;
use crate::app::state::automation::node_output::{

include!("automations_parts/part01.rs");
include!("automations_parts/part02.rs");
include!("automations_parts/part03.rs");
