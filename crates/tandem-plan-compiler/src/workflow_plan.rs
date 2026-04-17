// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tandem_workflows::plan_package::{
    AutomationV2Schedule, AutomationV2ScheduleType, WorkflowPlan, WorkflowPlanConversation,
    WorkflowPlanDraftRecord, WorkflowPlanStep,
};

include!("workflow_plan_parts/part01.rs");
include!("workflow_plan_parts/part02.rs");
