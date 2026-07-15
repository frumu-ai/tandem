// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(crate) fn compile_plan_to_automation_v2(
    plan: &crate::WorkflowPlan,
    plan_package: Option<&tandem_plan_compiler::api::PlanPackage>,
    creator_id: &str,
) -> crate::AutomationV2Spec {
    let mut seed = tandem_plan_compiler::api::project_workflow_runtime_materialization_seed(
        plan,
        crate::normalize_allowed_tools,
    );
    seed.context =
        plan_package.map(tandem_plan_compiler::api::project_plan_context_materialization);
    super::automation_projection_runtime::compile_materialization_seed_to_spec_into(
        seed,
        crate::AutomationV2Status::Active,
        plan.schedule.clone(),
        creator_id,
    )
}
