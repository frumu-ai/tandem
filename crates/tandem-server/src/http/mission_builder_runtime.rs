// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::Serialize;
use tandem_orchestrator::{MissionSpec, WorkItem};
use tandem_plan_compiler::api as compiler_api;
use tandem_workflows::{MissionBlueprint, ValidationMessage};
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MissionCompilePreview {
    pub blueprint: MissionBlueprint,
    pub automation: crate::AutomationV2Spec,
    pub mission_spec: MissionSpec,
    pub work_items: Vec<WorkItem>,
    pub node_previews: Vec<compiler_api::CompiledNodePreview>,
    pub validation: Vec<ValidationMessage>,
}

pub(crate) fn compile_to_automation(
    blueprint: MissionBlueprint,
    schedule: Option<crate::AutomationV2Schedule>,
    creator_id: &str,
) -> crate::AutomationV2Spec {
    let seed = compiler_api::project_mission_runtime_materialization_seed(&blueprint);
    super::automation_projection_runtime::compile_materialization_seed_to_spec_into(
        seed,
        crate::AutomationV2Status::Draft,
        schedule.unwrap_or_else(|| {
            compiler_api::manual_schedule("UTC".to_string(), crate::RoutineMisfirePolicy::RunOnce)
        }),
        creator_id,
    )
}
