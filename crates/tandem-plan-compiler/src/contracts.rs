// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_workflows::plan_package::{
    AutomationV2Schedule, AutomationV2ScheduleType, WorkflowPlan, WorkflowPlanDraftRecord,
    WorkflowPlanStep,
};

use crate::host::{Clock, PlanStore, PlannerLoopHost, WorkspaceResolver};
use crate::plan_bundle::compare_plan_package_replay;
use crate::plan_package::compile_workflow_plan_preview_package;
use crate::planner_build::{
    build_workflow_plan_with_planner, PlannerBuildConfig, PlannerBuildRequest, PlannerBuildResult,
};
use crate::planner_drafts::{
    revise_workflow_plan_draft, PlannerDraftError, PlannerDraftRevisionResult,
};
use crate::planner_loop::PlannerLoopConfig;
use crate::workflow_plan::{inferred_output_validator_kind, WorkflowInputRefLike};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedOutputValidatorKind {
    ResearchBrief,
    ReviewDecision,
    StructuredJson,
    CodePatch,
    GenericArtifact,
}

pub fn projected_output_validator_kind_from_key(kind: &str) -> ProjectedOutputValidatorKind {
    match kind.trim().to_ascii_lowercase().as_str() {
        "research_brief" => ProjectedOutputValidatorKind::ResearchBrief,
        "review_decision" => ProjectedOutputValidatorKind::ReviewDecision,
        "structured_json" => ProjectedOutputValidatorKind::StructuredJson,
        "code_patch" => ProjectedOutputValidatorKind::CodePatch,
        _ => ProjectedOutputValidatorKind::GenericArtifact,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputContractSeed {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator_kind: Option<ProjectedOutputValidatorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforcement: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_guidance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputContractPolicySeed {
    pub validation_profile: String,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub required_evidence: Vec<String>,
    #[serde(default)]
    pub required_sections: Vec<String>,
    #[serde(default)]
    pub prewrite_gates: Vec<String>,
    #[serde(default)]
    pub retry_on_missing: Vec<String>,
    #[serde(default)]
    pub terminal_on: Vec<String>,
    pub repair_budget: u32,
    pub session_text_recovery: String,
}

impl WorkflowInputRefLike for Value {
    fn from_step_id(&self) -> &str {
        self.get("from_step_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn clone_with_from_step_id(&self, from_step_id: &str) -> Option<Self> {
        let mut cloned = self.clone();
        let object = cloned.as_object_mut()?;
        object.insert(
            "from_step_id".to_string(),
            Value::String(from_step_id.to_string()),
        );
        Some(cloned)
    }
}

pub type AutomationV2ScheduleJson = AutomationV2Schedule<Value>;
pub type WorkflowPlanStepJson = WorkflowPlanStep<Value, Value>;
pub type WorkflowPlanJson = WorkflowPlan<AutomationV2ScheduleJson, WorkflowPlanStepJson>;
pub type WorkflowPlanDraftRecordJson = WorkflowPlanDraftRecord<WorkflowPlanJson>;

pub type PlannerBuildRequestJson = PlannerBuildRequest<Value>;
pub type PlannerBuildResultJson = PlannerBuildResult<Value, Value, Value>;
pub type PlannerDraftRevisionResultJson = PlannerDraftRevisionResult<Value, Value, Value>;

pub fn workflow_plan_to_json<S, Step>(
    plan: &WorkflowPlan<S, Step>,
) -> Result<WorkflowPlanJson, String>
where
    S: Serialize,
    Step: Serialize,
{
    serde_json::from_value(serde_json::to_value(plan).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

pub fn output_contract_seed(
    kind: impl Into<String>,
    schema: Option<Value>,
    summary_guidance: Option<String>,
) -> OutputContractSeed {
    let kind = kind.into();
    OutputContractSeed {
        validator_kind: Some(inferred_output_validator_kind(&kind)),
        kind,
        enforcement: None,
        schema,
        summary_guidance,
    }
}

pub fn default_execute_goal_output_contract_seed() -> OutputContractSeed {
    output_contract_seed("structured_json", None, None)
}

pub fn code_patch_output_contract_seed() -> OutputContractSeed {
    output_contract_seed(
        "code_patch",
        None,
        Some("Produce a code-oriented artifact backed by an inspect -> patch -> apply -> test -> repair loop.".to_string()),
    )
}

pub fn review_summary_output_contract_seed() -> OutputContractSeed {
    output_contract_seed(
        "review_summary",
        None,
        Some("Summarize the review outcome and required follow-ups.".to_string()),
    )
}

pub fn approval_gate_output_contract_seed() -> OutputContractSeed {
    output_contract_seed(
        "approval_gate",
        None,
        Some("Summarize the review outcome and required follow-ups.".to_string()),
    )
}

pub fn compile_workflow_plan_preview_package_with_revision(
    plan: &WorkflowPlanJson,
    owner_id: Option<&str>,
    plan_revision: u32,
) -> crate::plan_package::PlanPackage {
    let mut package = compile_workflow_plan_preview_package(plan, owner_id);
    package.plan_revision = plan_revision.max(1);
    package
}

pub fn compare_workflow_plan_preview_replay_with_revision(
    current: &WorkflowPlanJson,
    current_revision: u32,
    initial: &WorkflowPlanJson,
    initial_revision: u32,
) -> crate::plan_bundle::PlanReplayReport {
    let current_package = compile_workflow_plan_preview_package_with_revision(
        current,
        Some("workflow_planner"),
        current_revision,
    );
    let initial_package = compile_workflow_plan_preview_package_with_revision(
        initial,
        Some("workflow_planner"),
        initial_revision,
    );
    compare_plan_package_replay(&initial_package, &current_package)
}

pub fn research_output_contract_policy_seed(
    normalized_kind: &str,
    expects_web_research: bool,
    repair_budget: u32,
) -> OutputContractPolicySeed {
    let validation_profile = if (normalized_kind == "citations" && expects_web_research)
        || (expects_web_research && normalized_kind != "brief")
    {
        "external_research"
    } else if normalized_kind == "citations" {
        "artifact_only"
    } else if matches!(
        normalized_kind,
        "brief" | "draft_deliverable" | "final_deliverable" | "report" | "report_writing"
    ) {
        "research_synthesis"
    } else {
        "local_research"
    };

    OutputContractPolicySeed {
        validation_profile: validation_profile.to_string(),
        required_tools: match validation_profile {
            "external_research" => vec!["websearch".to_string()],
            "local_research" => vec!["read".to_string()],
            _ => Vec::new(),
        },
        required_evidence: match validation_profile {
            "external_research" => vec!["external_sources".to_string()],
            "local_research" => vec!["local_source_reads".to_string()],
            // Synthesis/final-brief steps consume upstream artifacts and MCP/web evidence.
            // They should not be forced to perform fresh workspace file reads unless a
            // dedicated local-research step asked for that evidence earlier.
            _ if expects_web_research => vec!["external_sources".to_string()],
            _ => Vec::new(),
        },
        required_sections: match validation_profile {
            "external_research" => vec!["citations".to_string()],
            "research_synthesis" if expects_web_research => vec!["citations".to_string()],
            _ => Vec::new(),
        },
        prewrite_gates: match validation_profile {
            "external_research" => vec!["successful_web_research".to_string()],
            "local_research" => vec![
                "workspace_inspection".to_string(),
                "concrete_reads".to_string(),
            ],
            _ => Vec::new(),
        },
        retry_on_missing: match validation_profile {
            "external_research" => vec![
                "external_sources".to_string(),
                "citations".to_string(),
                "successful_web_research".to_string(),
            ],
            "local_research" => vec![
                "local_source_reads".to_string(),
                "workspace_inspection".to_string(),
                "concrete_reads".to_string(),
            ],
            _ if expects_web_research => {
                vec!["external_sources".to_string(), "citations".to_string()]
            }
            _ => Vec::new(),
        },
        terminal_on: vec![
            "tool_unavailable".to_string(),
            "repair_budget_exhausted".to_string(),
        ],
        repair_budget,
        session_text_recovery: "require_prewrite_satisfied".to_string(),
    }
}

pub fn default_fallback_schedule_json() -> AutomationV2ScheduleJson {
    AutomationV2Schedule {
        schedule_type: AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: json!({}),
    }
}

pub fn default_fallback_step_json() -> WorkflowPlanStepJson {
    WorkflowPlanStep {
        step_id: "collect_inputs".to_string(),
        kind: "collect_inputs".to_string(),
        objective: "Collect required inputs for the workflow.".to_string(),
        depends_on: Vec::new(),
        agent_role: "worker".to_string(),
        input_refs: Vec::new(),
        output_contract: None,
        metadata: None,
    }
}

pub async fn build_workflow_plan_with_planner_json<H>(
    host: &H,
    request: PlannerBuildRequestJson,
    config: PlannerBuildConfig,
) -> PlannerBuildResultJson
where
    H: PlannerLoopHost + WorkspaceResolver,
{
    build_workflow_plan_with_planner::<Value, Value, Value, H>(
        host,
        request,
        config,
        |_| {},
        default_fallback_step_json(),
    )
    .await
}

pub async fn revise_workflow_plan_draft_json<H>(
    host: &H,
    plan_id: &str,
    message: &str,
    config: PlannerLoopConfig,
) -> Result<PlannerDraftRevisionResultJson, PlannerDraftError>
where
    H: PlannerLoopHost + PlanStore + Clock,
{
    revise_workflow_plan_draft::<Value, Value, Value, H>(host, plan_id, message, config, |_| {})
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_execute_goal_output_contract_seed_is_structured_json() {
        let seed = default_execute_goal_output_contract_seed();
        assert_eq!(seed.kind, "structured_json");
        assert_eq!(
            seed.validator_kind,
            Some(ProjectedOutputValidatorKind::StructuredJson)
        );
        assert!(seed.enforcement.is_none());
    }

    #[test]
    fn code_patch_output_contract_seed_is_code_patch() {
        let seed = code_patch_output_contract_seed();
        assert_eq!(seed.kind, "code_patch");
        assert_eq!(
            seed.validator_kind,
            Some(ProjectedOutputValidatorKind::CodePatch)
        );
        assert!(seed
            .summary_guidance
            .as_deref()
            .is_some_and(|value| value.contains("inspect -> patch -> apply -> test -> repair")));
    }

    #[test]
    fn research_output_contract_policy_seed_matches_external_research_defaults() {
        let seed = research_output_contract_policy_seed("research_sources", true, 3);
        assert_eq!(seed.validation_profile, "external_research");
        assert_eq!(seed.required_tools, vec!["websearch".to_string()]);
        assert!(seed
            .retry_on_missing
            .contains(&"successful_web_research".to_string()));
    }

    #[test]
    fn citations_contract_without_required_web_uses_artifact_only_defaults() {
        let seed = research_output_contract_policy_seed("citations", false, 3);
        assert_eq!(seed.validation_profile, "artifact_only");
        assert!(!seed.required_tools.iter().any(|tool| tool == "websearch"));
        assert!(!seed
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research"));
        assert!(!seed
            .required_evidence
            .iter()
            .any(|evidence| evidence == "external_sources"));
    }

    #[test]
    fn research_synthesis_contract_does_not_require_fresh_local_reads() {
        let seed = research_output_contract_policy_seed("brief", true, 3);
        assert_eq!(seed.validation_profile, "research_synthesis");
        assert!(!seed.required_tools.iter().any(|tool| tool == "read"));
        assert!(!seed
            .required_evidence
            .iter()
            .any(|evidence| evidence == "local_source_reads"));
        assert!(!seed
            .retry_on_missing
            .iter()
            .any(|requirement| requirement == "local_source_reads"));
        assert!(seed
            .required_evidence
            .iter()
            .any(|evidence| evidence == "external_sources"));
    }

    #[test]
    fn draft_deliverable_contract_does_not_require_fresh_local_reads() {
        let seed = research_output_contract_policy_seed("draft_deliverable", false, 3);
        assert_eq!(seed.validation_profile, "research_synthesis");
        assert!(!seed.required_tools.iter().any(|tool| tool == "read"));
        assert!(!seed
            .required_evidence
            .iter()
            .any(|evidence| evidence == "local_source_reads"));
        assert!(!seed
            .prewrite_gates
            .iter()
            .any(|gate| gate == "concrete_reads"));
    }
}
