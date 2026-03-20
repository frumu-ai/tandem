use serde_json::Value;
use tandem_types::ModelSpec;

use crate::app::state::{default_model_spec_from_effective_config, AppState};
use crate::routines::types::{RoutineRunArtifact, RoutineRunRecord};
use crate::util::time::now_ms;
use crate::EngineEvent;

pub async fn build_routine_prompt(state: &AppState, run: &RoutineRunRecord) -> String {
    let normalized_entrypoint = run.entrypoint.trim();
    let known_tool = state
        .tools
        .list()
        .await
        .into_iter()
        .any(|schema| schema.name == normalized_entrypoint);
    if known_tool {
        let args = if run.args.is_object() {
            run.args.clone()
        } else {
            serde_json::json!({})
        };
        return format!("/tool {} {}", normalized_entrypoint, args);
    }

    if let Some(objective) = routine_objective_from_args(run) {
        return build_routine_mission_prompt(run, &objective);
    }

    format!(
        "Execute routine '{}' using entrypoint '{}' with args: {}",
        run.routine_id, run.entrypoint, run.args
    )
}

pub fn routine_objective_from_args(run: &RoutineRunRecord) -> Option<String> {
    run.args
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn routine_mode_from_args(args: &Value) -> &str {
    args.get("mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("standalone")
}

fn routine_success_criteria_from_args(args: &Value) -> Vec<String> {
    args.get("success_criteria")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str())
                .map(str::trim)
                .filter(|row| !row.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub fn build_routine_mission_prompt(run: &RoutineRunRecord, objective: &str) -> String {
    let mode = routine_mode_from_args(&run.args);
    let success_criteria = routine_success_criteria_from_args(&run.args);
    let orchestrator_only_tool_calls = run
        .args
        .get("orchestrator_only_tool_calls")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut lines = vec![
        format!("Automation ID: {}", run.routine_id),
        format!("Run ID: {}", run.run_id),
        format!("Mode: {}", mode),
        format!("Mission Objective: {}", objective),
    ];

    if !success_criteria.is_empty() {
        lines.push("Success Criteria:".to_string());
        for criterion in success_criteria {
            lines.push(format!("- {}", criterion));
        }
    }

    if run.allowed_tools.is_empty() {
        lines.push("Allowed Tools: all available by current policy".to_string());
    } else {
        lines.push(format!("Allowed Tools: {}", run.allowed_tools.join(", ")));
    }

    if run.output_targets.is_empty() {
        lines.push("Output Targets: none configured".to_string());
    } else {
        lines.push("Output Targets:".to_string());
        for target in &run.output_targets {
            lines.push(format!("- {}", target));
        }
    }

    if mode.eq_ignore_ascii_case("orchestrated") {
        lines.push("Execution Pattern: Plan -> Do -> Verify -> Notify".to_string());
        lines
            .push("Role Contract: Orchestrator owns final decisions and final output.".to_string());
        if orchestrator_only_tool_calls {
            lines.push(
                "Tool Policy: only the orchestrator may execute tools; helper roles propose actions/results."
                    .to_string(),
            );
        }
    } else {
        lines.push("Execution Pattern: Standalone mission run".to_string());
    }

    lines.push(
        "Deliverable: produce a concise final report that states what was done, what was verified, and final artifact locations."
            .to_string(),
    );

    lines.join("\n")
}

pub async fn append_configured_output_artifacts(state: &AppState, run: &RoutineRunRecord) {
    if run.output_targets.is_empty() {
        return;
    }
    for target in &run.output_targets {
        let artifact = RoutineRunArtifact {
            artifact_id: format!("artifact-{}", uuid::Uuid::new_v4()),
            uri: target.clone(),
            kind: "output_target".to_string(),
            label: Some("configured output target".to_string()),
            created_at_ms: now_ms(),
            metadata: Some(serde_json::json!({
                "source": "routine.output_targets",
                "runID": run.run_id,
                "routineID": run.routine_id,
            })),
        };
        let _ = state
            .append_routine_run_artifact(&run.run_id, artifact.clone())
            .await;
        state.event_bus.publish(EngineEvent::new(
            "routine.run.artifact_added",
            serde_json::json!({
                "runID": run.run_id,
                "routineID": run.routine_id,
                "artifact": artifact,
            }),
        ));
    }
}

pub async fn resolve_routine_model_spec_for_run(
    state: &AppState,
    run: &RoutineRunRecord,
) -> (Option<ModelSpec>, String) {
    let providers = state.providers.list().await;
    let mode = routine_mode_from_args(&run.args);
    let mut requested: Vec<(ModelSpec, &str)> = Vec::new();

    if mode.eq_ignore_ascii_case("orchestrated") {
        if let Some(orchestrator) = model_spec_for_role_from_args(&run.args, "orchestrator") {
            requested.push((orchestrator, "args.model_policy.role_models.orchestrator"));
        }
    }
    if let Some(default_model) = default_model_spec_from_args(&run.args) {
        requested.push((default_model, "args.model_policy.default_model"));
    }
    let effective_config = state.config.get_effective_value().await;
    if let Some(config_default) = default_model_spec_from_effective_config(&effective_config) {
        requested.push((config_default, "config.default_provider"));
    }

    for (candidate, source) in requested {
        if provider_catalog_has_model(&providers, &candidate) {
            return (Some(candidate), source.to_string());
        }
    }

    let fallback = providers
        .into_iter()
        .find(|provider| !provider.models.is_empty())
        .and_then(|provider| {
            let model = provider.models.first()?;
            Some(ModelSpec {
                provider_id: provider.id,
                model_id: model.id.clone(),
            })
        });

    (fallback, "provider_catalog_fallback".to_string())
}

pub fn parse_model_spec(value: &Value) -> Option<ModelSpec> {
    let obj = value.as_object()?;
    let provider_id = obj.get("provider_id")?.as_str()?.trim();
    let model_id = obj.get("model_id")?.as_str()?.trim();
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some(ModelSpec {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

fn model_spec_for_role_from_args(args: &Value, role: &str) -> Option<ModelSpec> {
    args.get("model_policy")
        .and_then(|v| v.get("role_models"))
        .and_then(|v| v.get(role))
        .and_then(parse_model_spec)
}

fn default_model_spec_from_args(args: &Value) -> Option<ModelSpec> {
    args.get("model_policy")
        .and_then(|v| v.get("default_model"))
        .and_then(parse_model_spec)
}

pub fn provider_catalog_has_model(
    providers: &[tandem_types::ProviderInfo],
    spec: &ModelSpec,
) -> bool {
    providers.iter().any(|provider| {
        provider.id == spec.provider_id
            && provider
                .models
                .iter()
                .any(|model| model.id == spec.model_id)
    })
}
