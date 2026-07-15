// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use uuid::Uuid;

pub(crate) fn compile_materialization_seed_to_spec_into<I, O>(
    seed: tandem_plan_compiler::api::ProjectedAutomationMaterializationSeed<I, O>,
    status: crate::AutomationV2Status,
    schedule: crate::AutomationV2Schedule,
    creator_id: &str,
) -> crate::AutomationV2Spec
where
    I: Into<crate::AutomationFlowInputRef>,
    O: Into<crate::AutomationFlowOutputContract>,
{
    let tandem_plan_compiler::api::ProjectedAutomationMaterializationSeed {
        name,
        description,
        workspace_root,
        output_targets,
        agents: projected_agents,
        nodes: projected_nodes,
        execution,
        context,
        metadata: seed_metadata,
    } = seed;
    let now = crate::now_ms();
    let agents = projected_agents
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let nodes = projected_nodes
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let mut metadata = match seed_metadata {
        serde_json::Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("projection_metadata".to_string(), other);
            map
        }
    };
    if let Some(context) = context {
        metadata.insert(
            "context_materialization".to_string(),
            serde_json::to_value(context).unwrap_or(serde_json::Value::Null),
        );
    }

    crate::AutomationV2Spec {
        automation_id: format!("automation-v2-{}", Uuid::new_v4()),
        name,
        description,
        status,
        schedule,
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents,
        flow: crate::AutomationFlowSpec { nodes },
        execution: execution.into(),
        output_targets,
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: creator_id.to_string(),
        workspace_root,
        metadata: Some(serde_json::Value::Object(metadata)),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}
