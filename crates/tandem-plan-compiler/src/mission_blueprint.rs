// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::{json, Value};
use std::collections::HashMap;
use tandem_orchestrator::{
    KnowledgeBinding, KnowledgeReuseMode, KnowledgeScope, KnowledgeSpaceRef, KnowledgeTrustLevel,
    MissionSpec, WorkItem, WorkItemStatus,
};
use tandem_workflows::{
    MissionBlueprint, MissionPhaseExecutionMode, ReviewStageKind, WorkstreamBlueprint,
};

use crate::contracts::research_output_contract_policy_seed;

pub const MISSION_EXECUTION_KIND_CODER_RUN: &str = "coder_run";
pub const MISSION_EXECUTION_KIND_GOVERNANCE: &str = "governance";

pub fn derive_mission_spec(blueprint: &MissionBlueprint) -> MissionSpec {
    let mut spec = MissionSpec::new(blueprint.title.clone(), blueprint.goal.clone());
    spec.mission_id = blueprint.mission_id.clone();
    spec.knowledge = mission_knowledge_defaults(
        Some(blueprint.goal.as_str()),
        Some(format!("mission/{}", blueprint.mission_id)),
    );
    spec.success_criteria = blueprint.success_criteria.clone();
    spec.entrypoint = Some("automation_v2".to_string());
    spec.metadata = Some(json!({
        "builder_kind": "mission_blueprint",
        "shared_context": blueprint.shared_context,
        "orchestrator_template_id": blueprint.orchestrator_template_id,
        "phases": blueprint.phases,
        "milestones": blueprint.milestones,
    }));
    spec
}

pub fn derive_work_items(blueprint: &MissionBlueprint) -> Vec<WorkItem> {
    let mut items = blueprint
        .workstreams
        .iter()
        .map(|workstream| WorkItem {
            work_item_id: workstream.workstream_id.clone(),
            title: workstream.title.clone(),
            detail: Some(workstream.objective.clone()),
            status: WorkItemStatus::Todo,
            depends_on: workstream.depends_on.clone(),
            assigned_agent: Some(format!("agent_{}", workstream.workstream_id)),
            run_id: None,
            artifact_refs: Vec::new(),
            metadata: Some(json!({
                "execution_kind": MISSION_EXECUTION_KIND_CODER_RUN,
                "role": workstream.role,
                "template_id": workstream.template_id,
                "stage_kind": "workstream",
                "priority": workstream.priority,
                "phase_id": workstream.phase_id,
                "lane": workstream.lane,
                "milestone": workstream.milestone,
            })),
        })
        .collect::<Vec<_>>();

    items.extend(blueprint.review_stages.iter().map(|stage| WorkItem {
        work_item_id: stage.stage_id.clone(),
        title: stage.title.clone(),
        detail: Some(stage.prompt.clone()),
        status: WorkItemStatus::Todo,
        depends_on: stage.target_ids.clone(),
        assigned_agent: Some(format!("agent_{}", stage.stage_id)),
        run_id: None,
        artifact_refs: Vec::new(),
        metadata: Some(json!({
            "execution_kind": MISSION_EXECUTION_KIND_GOVERNANCE,
            "stage_kind": review_stage_kind_to_lower_string(stage.stage_kind.clone()),
            "template_id": stage.template_id,
            "role": stage.role,
            "priority": stage.priority,
            "phase_id": stage.phase_id,
            "lane": stage.lane,
            "milestone": stage.milestone,
        })),
    }));

    items
}

pub fn phase_rank_map(blueprint: &MissionBlueprint) -> HashMap<String, usize> {
    blueprint
        .phases
        .iter()
        .enumerate()
        .map(|(index, phase)| (phase.phase_id.clone(), index))
        .collect()
}

pub fn compile_barrier_dependencies(
    blueprint: &MissionBlueprint,
    phase_rank: &HashMap<String, usize>,
) -> HashMap<String, Vec<String>> {
    let mut stage_phase = HashMap::<String, String>::new();
    for workstream in &blueprint.workstreams {
        if let Some(phase_id) = workstream.phase_id.as_ref() {
            stage_phase.insert(workstream.workstream_id.clone(), phase_id.clone());
        }
    }
    for stage in &blueprint.review_stages {
        if let Some(phase_id) = stage.phase_id.as_ref() {
            stage_phase.insert(stage.stage_id.clone(), phase_id.clone());
        }
    }
    let mut out = HashMap::<String, Vec<String>>::new();
    for phase in &blueprint.phases {
        if phase.execution_mode != Some(MissionPhaseExecutionMode::Barrier) {
            continue;
        }
        let Some(&rank) = phase_rank.get(&phase.phase_id) else {
            continue;
        };
        let prior_stage_ids = stage_phase
            .iter()
            .filter_map(|(stage_id, stage_phase_id)| {
                phase_rank
                    .get(stage_phase_id)
                    .filter(|dep_rank| **dep_rank < rank)
                    .map(|_| stage_id.clone())
            })
            .collect::<Vec<_>>();
        for workstream in &blueprint.workstreams {
            if workstream.phase_id.as_deref() == Some(phase.phase_id.as_str()) {
                out.insert(workstream.workstream_id.clone(), prior_stage_ids.clone());
            }
        }
        for stage in &blueprint.review_stages {
            if stage.phase_id.as_deref() == Some(phase.phase_id.as_str()) {
                out.insert(stage.stage_id.clone(), prior_stage_ids.clone());
            }
        }
    }
    out
}

pub fn mission_workstream_builder_defaults(
    workstream: &WorkstreamBlueprint,
) -> serde_json::Map<String, Value> {
    let mut builder = serde_json::Map::new();
    builder.insert("title".to_string(), json!(workstream.title));
    builder.insert("role".to_string(), json!(workstream.role));
    builder.insert("prompt".to_string(), json!(workstream.prompt));
    builder.insert("priority".to_string(), json!(workstream.priority));
    builder.insert("phase_id".to_string(), json!(workstream.phase_id));
    builder.insert("lane".to_string(), json!(workstream.lane));
    builder.insert("milestone".to_string(), json!(workstream.milestone));
    builder.insert(
        "knowledge".to_string(),
        serde_json::to_value(mission_knowledge_defaults(
            Some(workstream.objective.as_str()),
            workstream
                .lane
                .clone()
                .or_else(|| workstream.phase_id.clone())
                .or_else(|| Some(workstream.role.clone())),
        ))
        .unwrap_or(Value::Null),
    );
    let expects_web_research = workstream_expects_web_research(workstream);
    if workstream.output_contract.kind.trim().to_ascii_lowercase() == "brief" {
        builder.insert(
            "web_research_expected".to_string(),
            Value::Bool(expects_web_research),
        );
    }
    builder
}

pub fn mission_workstream_node_metadata(workstream: &WorkstreamBlueprint) -> Option<Value> {
    let mut root = match workstream.metadata.clone() {
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = serde_json::Map::new();
            map.insert("blueprint_metadata".to_string(), other);
            map
        }
        None => serde_json::Map::new(),
    };
    let builder = root
        .entry("builder".to_string())
        .or_insert_with(|| json!({}));
    if !builder.is_object() {
        *builder = json!({});
    }
    let Some(builder_map) = builder.as_object_mut() else {
        return Some(Value::Object(root));
    };
    builder_map
        .entry("execution_kind".to_string())
        .or_insert_with(|| json!(MISSION_EXECUTION_KIND_CODER_RUN));
    for (key, value) in mission_workstream_builder_defaults(workstream) {
        builder_map.entry(key).or_insert(value);
    }
    Some(Value::Object(root))
}

pub fn mission_workstream_enforcement_defaults(workstream: &WorkstreamBlueprint) -> Option<Value> {
    let expects_web_research = workstream_expects_web_research(workstream);
    let normalized_kind = workstream.output_contract.kind.trim().to_ascii_lowercase();
    serde_json::to_value(research_output_contract_policy_seed(
        &normalized_kind,
        expects_web_research,
        3,
    ))
    .ok()
}

pub fn workstream_expects_web_research(workstream: &WorkstreamBlueprint) -> bool {
    workstream
        .workstream_id
        .to_ascii_lowercase()
        .contains("research")
        || workstream.role.to_ascii_lowercase().contains("research")
        || workstream.objective.to_ascii_lowercase().contains("web")
        || workstream.objective.to_ascii_lowercase().contains("online")
        || workstream
            .objective
            .to_ascii_lowercase()
            .contains("current")
        || workstream.objective.to_ascii_lowercase().contains("latest")
        || workstream.prompt.to_ascii_lowercase().contains("web")
        || workstream.prompt.to_ascii_lowercase().contains("online")
        || workstream.prompt.to_ascii_lowercase().contains("current")
        || workstream.prompt.to_ascii_lowercase().contains("latest")
}

pub fn mission_knowledge_defaults(
    subject: Option<impl AsRef<str>>,
    namespace: Option<String>,
) -> KnowledgeBinding {
    let subject = subject
        .as_ref()
        .map(|value| value.as_ref().trim().to_string())
        .filter(|value| !value.is_empty());
    let namespace = namespace
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    KnowledgeBinding {
        enabled: true,
        reuse_mode: KnowledgeReuseMode::Preflight,
        trust_floor: KnowledgeTrustLevel::Promoted,
        read_spaces: vec![KnowledgeSpaceRef {
            scope: KnowledgeScope::Project,
            ..KnowledgeSpaceRef::default()
        }],
        promote_spaces: vec![KnowledgeSpaceRef {
            scope: KnowledgeScope::Project,
            ..KnowledgeSpaceRef::default()
        }],
        namespace,
        subject,
        freshness_ms: None,
    }
}

fn review_stage_kind_to_lower_string(kind: ReviewStageKind) -> String {
    match kind {
        ReviewStageKind::Review => "review".to_string(),
        ReviewStageKind::Test => "test".to_string(),
        ReviewStageKind::Approval => "approval".to_string(),
    }
}
