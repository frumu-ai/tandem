// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};

use crate::dependency_planner::{plan_routine_execution, DependencyPlanningError};
use crate::plan_package::{
    derive_credential_envelopes_for_plan, derive_success_criteria_evaluation_for_plan, PlanPackage,
    PlanValidationState, PrecedenceSourceTier, SuccessCriteriaEvaluationReport, TriggerKind,
};

// Treat directory roots and recursive globs as the same prefix family.
fn normalize_scope_path(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix("/**") {
        prefix.trim_end_matches('/').to_string()
    } else {
        trimmed.to_string()
    }
}

fn path_patterns_overlap(left: &str, right: &str) -> bool {
    let left_prefix = normalize_scope_path(left);
    let right_prefix = normalize_scope_path(right);
    if left_prefix.is_empty() || right_prefix.is_empty() {
        return false;
    }
    if left_prefix == right_prefix {
        return true;
    }
    left_prefix.starts_with(&format!("{right_prefix}/"))
        || right_prefix.starts_with(&format!("{left_prefix}/"))
}

fn is_seeded_context_kind(kind: &str) -> bool {
    matches!(kind, "mission_goal" | "workspace_environment")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanValidationIssue {
    pub code: String,
    pub severity: PlanValidationSeverity,
    pub path: String,
    pub message: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanValidationReport {
    pub ready_for_apply: bool,
    pub ready_for_activation: bool,
    pub blocker_count: usize,
    pub warning_count: usize,
    pub validation_state: PlanValidationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria_evaluation: Option<SuccessCriteriaEvaluationReport>,
    #[serde(default)]
    pub issues: Vec<PlanValidationIssue>,
}

pub fn validate_plan_package(plan: &PlanPackage) -> PlanValidationReport {
    let mut issues = Vec::new();
    let mut seen_binding_capabilities = std::collections::BTreeSet::new();
    let mapped_capabilities = plan
        .connector_bindings
        .iter()
        .filter(|binding| binding.status == "mapped")
        .map(|binding| binding.capability.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mapped_bindings = plan
        .connector_bindings
        .iter()
        .map(|binding| (binding.binding_id.as_str(), binding.capability.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();

    for (index, binding) in plan.connector_bindings.iter().enumerate() {
        if !seen_binding_capabilities.insert(binding.capability.as_str()) {
            issues.push(PlanValidationIssue {
                code: "duplicate_connector_binding".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("connector_bindings[{index}]"),
                message: format!(
                    "Connector capability `{}` is bound more than once in the plan package.",
                    binding.capability
                ),
                blocking: true,
            });
        }

        if binding.status == "mapped"
            && (binding.binding_type.trim().is_empty() || binding.binding_id.trim().is_empty())
        {
            issues.push(PlanValidationIssue {
                code: "mapped_connector_binding_missing_metadata".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("connector_bindings[{index}]"),
                message: format!(
                    "Mapped connector capability `{}` must include binding_type and binding_id.",
                    binding.capability
                ),
                blocking: true,
            });
        }
    }

    for (index, intent) in plan.connector_intents.iter().enumerate() {
        if mapped_capabilities.contains(intent.capability.as_str()) {
            continue;
        }
        let blocking = intent.required;
        issues.push(PlanValidationIssue {
            code: if blocking {
                "required_connector_unresolved".to_string()
            } else {
                "optional_connector_unresolved".to_string()
            },
            severity: if blocking {
                PlanValidationSeverity::Error
            } else {
                PlanValidationSeverity::Warning
            },
            path: format!("connector_intents[{index}]"),
            message: format!(
                "Connector capability `{}` is not mapped for preview activation.",
                intent.capability
            ),
            blocking,
        });
    }

    if let Some(budget_policy) = &plan.budget_policy {
        if let Some(max_cost) = budget_policy.max_cost_per_run_usd {
            if max_cost <= 0.0 {
                issues.push(PlanValidationIssue {
                    code: "invalid_budget_max_cost".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: "budget_policy.max_cost_per_run_usd".to_string(),
                    message: "max_cost_per_run_usd must be greater than 0.".to_string(),
                    blocking: true,
                });
            }
        }
    }

    if let Some(budget_enforcement) = &plan.budget_enforcement {
        if let Some(behavior) = &budget_enforcement.hard_limit_behavior {
            if behavior != "pause_before_step" && behavior != "cancel_run" {
                issues.push(PlanValidationIssue {
                    code: "invalid_budget_hard_limit_behavior".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: "budget_enforcement.hard_limit_behavior".to_string(),
                    message: "hard_limit_behavior must be 'pause_before_step' or 'cancel_run'."
                        .to_string(),
                    blocking: true,
                });
            }
        }
    }

    let mut routine_ids = std::collections::BTreeSet::new();
    for (routine_index, routine) in plan.routine_graph.iter().enumerate() {
        if !routine_ids.insert(routine.routine_id.as_str()) {
            issues.push(PlanValidationIssue {
                code: "duplicate_routine_id".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("routine_graph[{routine_index}].routine_id"),
                message: format!(
                    "Routine id `{}` appears more than once in the plan package.",
                    routine.routine_id
                ),
                blocking: true,
            });
        }
    }

    for (routine_index, routine) in plan.routine_graph.iter().enumerate() {
        match routine.trigger.trigger_type {
            TriggerKind::Scheduled => {
                if routine.trigger.schedule.is_none() || routine.trigger.timezone.is_none() {
                    issues.push(PlanValidationIssue {
                        code: "invalid_schedule".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!("routine_graph[{routine_index}].trigger"),
                        message: "Scheduled routines must include both schedule and timezone."
                            .to_string(),
                        blocking: true,
                    });
                }
            }
            _ => {
                if routine.trigger.schedule.is_some() || routine.trigger.timezone.is_some() {
                    issues.push(PlanValidationIssue {
                        code: "trigger_schedule_mismatch".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!("routine_graph[{routine_index}].trigger"),
                        message: "Only scheduled routines may declare schedule or timezone fields."
                            .to_string(),
                        blocking: true,
                    });
                }
            }
        }

        for (dependency_index, dependency) in routine.dependencies.iter().enumerate() {
            if !routine_ids.contains(dependency.routine_id.as_str()) {
                issues.push(PlanValidationIssue {
                    code: "missing_routine_dependency".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!(
                        "routine_graph[{routine_index}].dependencies[{dependency_index}]"
                    ),
                    message: format!(
                        "Routine dependency `{}` does not exist in the plan package.",
                        dependency.routine_id
                    ),
                    blocking: true,
                });
            }
        }

        let mut step_ids = std::collections::BTreeSet::new();
        for (step_index, step) in routine.steps.iter().enumerate() {
            if !step_ids.insert(step.step_id.as_str()) {
                issues.push(PlanValidationIssue {
                    code: "duplicate_step_id".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("routine_graph[{routine_index}].steps[{step_index}].step_id"),
                    message: format!(
                        "Step id `{}` appears more than once in routine `{}`.",
                        step.step_id, routine.routine_id
                    ),
                    blocking: true,
                });
            }
        }

        for (step_index, step) in routine.steps.iter().enumerate() {
            for dependency in &step.dependencies {
                if !step_ids.contains(dependency.as_str())
                    && !routine_ids.contains(dependency.as_str())
                {
                    issues.push(PlanValidationIssue {
                        code: "missing_step_dependency".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!("routine_graph[{routine_index}].steps[{step_index}]"),
                        message: format!(
                            "Dependency `{}` does not resolve to a known step or routine.",
                            dependency
                        ),
                        blocking: true,
                    });
                }
            }
        }

        for (step_index, step) in routine.steps.iter().enumerate() {
            for (read_index, context_read) in step.context_reads.iter().enumerate() {
                let Some(context_object) = plan
                    .context_objects
                    .iter()
                    .find(|context| context.context_object_id == *context_read)
                else {
                    issues.push(PlanValidationIssue {
                        code: "missing_context_object_ref".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!(
                            "routine_graph[{routine_index}].steps[{step_index}].context_reads[{read_index}]"
                        ),
                        message: format!(
                            "Step `{}` references unknown context object `{}`.",
                            step.step_id, context_read
                        ),
                        blocking: true,
                    });
                    continue;
                };

                if !context_object
                    .declared_consumers
                    .iter()
                    .any(|consumer| consumer == &routine.routine_id)
                {
                    issues.push(PlanValidationIssue {
                        code: "context_read_consumer_violation".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!(
                            "routine_graph[{routine_index}].steps[{step_index}].context_reads[{read_index}]"
                        ),
                        message: format!(
                            "Step `{}` in routine `{}` cannot read context object `{}` because it is not a declared consumer.",
                            step.step_id, routine.routine_id, context_read
                        ),
                        blocking: true,
                    });
                    issues.push(PlanValidationIssue {
                        code: "cross_routine_prompt_injection_attempt".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!(
                            "routine_graph[{routine_index}].steps[{step_index}].context_reads[{read_index}]"
                        ),
                        message: format!(
                            "Step `{}` in routine `{}` attempted to read context object `{}` outside its declared consumer set.",
                            step.step_id, routine.routine_id, context_read
                        ),
                        blocking: true,
                    });
                }
            }

            for (write_index, context_write) in step.context_writes.iter().enumerate() {
                let Some(context_object) = plan
                    .context_objects
                    .iter()
                    .find(|context| context.context_object_id == *context_write)
                else {
                    issues.push(PlanValidationIssue {
                        code: "missing_context_object_ref".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!(
                            "routine_graph[{routine_index}].steps[{step_index}].context_writes[{write_index}]"
                        ),
                        message: format!(
                            "Step `{}` references unknown context object `{}` for write.",
                            step.step_id, context_write
                        ),
                        blocking: true,
                    });
                    continue;
                };

                if context_object.owner_routine_id != routine.routine_id
                    || context_object.producer_step_id.as_deref() != Some(step.step_id.as_str())
                {
                    issues.push(PlanValidationIssue {
                        code: "context_write_producer_mismatch".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!(
                            "routine_graph[{routine_index}].steps[{step_index}].context_writes[{write_index}]"
                        ),
                        message: format!(
                            "Step `{}` in routine `{}` cannot write context object `{}` because ownership or producer step does not match.",
                            step.step_id, routine.routine_id, context_write
                        ),
                        blocking: true,
                    });
                    issues.push(PlanValidationIssue {
                        code: "direct_peer_invocation_attempt".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!(
                            "routine_graph[{routine_index}].steps[{step_index}].context_writes[{write_index}]"
                        ),
                        message: format!(
                            "Step `{}` in routine `{}` attempted to write context object `{}` outside its producer ownership.",
                            step.step_id, routine.routine_id, context_write
                        ),
                        blocking: true,
                    });
                }
            }
        }

        if let Err(error) = plan_routine_execution(routine) {
            match error {
                DependencyPlanningError::MissingStepDependency {
                    step_id,
                    dependency,
                } => issues.push(PlanValidationIssue {
                    code: "missing_step_dependency".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("routine_graph[{routine_index}]"),
                    message: format!(
                        "Dependency planner could not resolve dependency `{}` for step `{}`.",
                        dependency, step_id
                    ),
                    blocking: true,
                }),
                DependencyPlanningError::CyclicStepDependencies { remaining_step_ids } => {
                    issues.push(PlanValidationIssue {
                        code: "cyclic_step_dependencies".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!("routine_graph[{routine_index}]"),
                        message: format!(
                            "Routine `{}` contains cyclic step dependencies involving: {}.",
                            routine.routine_id,
                            remaining_step_ids.join(", ")
                        ),
                        blocking: true,
                    });
                }
            }
        }

        for denied in &routine.data_scope.denied_paths {
            if routine.data_scope.readable_paths.contains(denied)
                || routine.data_scope.writable_paths.contains(denied)
            {
                issues.push(PlanValidationIssue {
                    code: "conflicting_data_scope".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("routine_graph[{routine_index}].data_scope"),
                    message: format!(
                        "Denied path `{}` also appears in readable or writable paths.",
                        denied
                    ),
                    blocking: true,
                });
            }
        }

        if routine.data_scope.readable_paths.is_empty() {
            issues.push(PlanValidationIssue {
                code: "empty_readable_scope".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("routine_graph[{routine_index}].data_scope.readable_paths"),
                message: "Routine data_scope must declare at least one readable path.".to_string(),
                blocking: true,
            });
        }

        if routine.data_scope.writable_paths.is_empty() {
            issues.push(PlanValidationIssue {
                code: "empty_writable_scope".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("routine_graph[{routine_index}].data_scope.writable_paths"),
                message: "Routine data_scope must declare at least one writable path.".to_string(),
                blocking: true,
            });
        }

        if matches!(
            routine.data_scope.mission_context_scope,
            crate::plan_package::MissionContextScope::FullPlan
        ) && routine
            .data_scope
            .mission_context_justification
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            issues.push(PlanValidationIssue {
                code: "full_plan_scope_requires_justification".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("routine_graph[{routine_index}].data_scope"),
                message: "mission_context_scope=full_plan requires explicit justification."
                    .to_string(),
                blocking: true,
            });
        }

        if matches!(
            routine.audit_scope.run_history_visibility,
            crate::plan_package::RunHistoryVisibility::NamedRoles
        ) && routine.audit_scope.named_audit_roles.is_empty()
        {
            issues.push(PlanValidationIssue {
                code: "named_audit_roles_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("routine_graph[{routine_index}].audit_scope"),
                message:
                    "run_history_visibility=named_roles requires at least one named audit role."
                        .to_string(),
                blocking: true,
            });
        }
    }

    for left_index in 0..plan.routine_graph.len() {
        let left = &plan.routine_graph[left_index];
        for right_index in (left_index + 1)..plan.routine_graph.len() {
            let right = &plan.routine_graph[right_index];

            for (read_index, readable_path) in left.data_scope.readable_paths.iter().enumerate() {
                for writable_path in &right.data_scope.writable_paths {
                    if path_patterns_overlap(readable_path, writable_path) {
                        issues.push(PlanValidationIssue {
                            code: "cross_routine_scope_overlap".to_string(),
                            severity: PlanValidationSeverity::Error,
                            path: format!(
                                "routine_graph[{left_index}].data_scope.readable_paths[{read_index}]"
                            ),
                            message: format!(
                                "Routine `{}` readable path `{}` overlaps routine `{}` writable path `{}` without a declared artifact handoff.",
                                left.routine_id, readable_path, right.routine_id, writable_path
                            ),
                            blocking: true,
                        });
                    }
                }
            }

            for (read_index, readable_path) in right.data_scope.readable_paths.iter().enumerate() {
                for writable_path in &left.data_scope.writable_paths {
                    if path_patterns_overlap(readable_path, writable_path) {
                        issues.push(PlanValidationIssue {
                            code: "cross_routine_scope_overlap".to_string(),
                            severity: PlanValidationSeverity::Error,
                            path: format!(
                                "routine_graph[{right_index}].data_scope.readable_paths[{read_index}]"
                            ),
                            message: format!(
                                "Routine `{}` readable path `{}` overlaps routine `{}` writable path `{}` without a declared artifact handoff.",
                                right.routine_id, readable_path, left.routine_id, writable_path
                            ),
                            blocking: true,
                        });
                    }
                }
            }
        }
    }

    if let Some(output_roots) = plan.output_roots.as_ref() {
        let roots = [
            output_roots.plan.as_deref(),
            output_roots.history.as_deref(),
            output_roots.proof.as_deref(),
            output_roots.drafts.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        for (routine_index, routine) in plan.routine_graph.iter().enumerate() {
            for (write_index, writable_path) in routine.data_scope.writable_paths.iter().enumerate()
            {
                if !roots
                    .iter()
                    .any(|output_root| path_patterns_overlap(writable_path, output_root))
                {
                    issues.push(PlanValidationIssue {
                        code: "writable_path_outside_output_roots".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!(
                            "routine_graph[{routine_index}].data_scope.writable_paths[{write_index}]"
                        ),
                        message: format!(
                            "Writable path `{}` falls outside the declared plan output roots.",
                            writable_path
                        ),
                        blocking: true,
                    });
                }
            }

            for (denied_index, denied_path) in routine.data_scope.denied_paths.iter().enumerate() {
                if roots
                    .iter()
                    .any(|output_root| path_patterns_overlap(denied_path, output_root))
                {
                    issues.push(PlanValidationIssue {
                        code: "denied_path_overlaps_output_root".to_string(),
                        severity: PlanValidationSeverity::Error,
                        path: format!("routine_graph[{routine_index}].data_scope.denied_paths[{denied_index}]"),
                        message: format!(
                            "Denied path `{}` overlaps a declared plan output root, which would block expected artifact writes.",
                            denied_path
                        ),
                        blocking: true,
                    });
                }
            }
        }
    }

    let expected_envelopes = derive_credential_envelopes_for_plan(plan)
        .into_iter()
        .map(|envelope| (envelope.routine_id.clone(), envelope))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut seen_envelope_bindings = std::collections::BTreeMap::new();
    for (routine_index, routine) in plan.routine_graph.iter().enumerate() {
        match plan
            .credential_envelopes
            .iter()
            .find(|envelope| envelope.routine_id == routine.routine_id)
        {
            None => issues.push(PlanValidationIssue {
                code: "credential_envelope_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("routine_graph[{routine_index}]"),
                message: format!(
                    "Routine `{}` is missing a credential envelope scaffold.",
                    routine.routine_id
                ),
                blocking: true,
            }),
            Some(envelope) => {
                if let Some(expected) = expected_envelopes.get(&routine.routine_id) {
                    if envelope.entitled_connectors != expected.entitled_connectors {
                        issues.push(PlanValidationIssue {
                            code: "credential_envelope_entitlements_mismatch".to_string(),
                            severity: PlanValidationSeverity::Error,
                            path: format!("credential_envelopes[{routine_index}]"),
                            message: format!(
                                "Routine `{}` credential envelope entitlements do not match the connectors required by its steps and mapped bindings.",
                                routine.routine_id
                            ),
                            blocking: true,
                        });
                    }
                    if envelope.denied_connectors != expected.denied_connectors {
                        issues.push(PlanValidationIssue {
                            code: "credential_envelope_denied_mismatch".to_string(),
                            severity: PlanValidationSeverity::Error,
                            path: format!("credential_envelopes[{routine_index}]"),
                            message: format!(
                                "Routine `{}` credential envelope denied connectors do not cover every mapped binding outside its entitled set.",
                                routine.routine_id
                            ),
                            blocking: true,
                        });
                    }
                }

                for connector in &envelope.entitled_connectors {
                    match mapped_bindings.get(connector.binding_id.as_str()) {
                        None => issues.push(PlanValidationIssue {
                            code: "credential_envelope_unknown_binding".to_string(),
                            severity: PlanValidationSeverity::Error,
                            path: format!("credential_envelopes[{routine_index}]"),
                            message: format!(
                                "Credential envelope for routine `{}` references unknown binding `{}`.",
                                routine.routine_id, connector.binding_id
                            ),
                            blocking: true,
                        }),
                        Some(capability) if *capability != connector.capability.as_str() => {
                            issues.push(PlanValidationIssue {
                                code: "credential_envelope_binding_capability_mismatch".to_string(),
                                severity: PlanValidationSeverity::Error,
                                path: format!("credential_envelopes[{routine_index}]"),
                                message: format!(
                                    "Credential envelope for routine `{}` maps binding `{}` to capability `{}`, but the plan binding is `{}`.",
                                    routine.routine_id,
                                    connector.binding_id,
                                    connector.capability,
                                    capability
                                ),
                                blocking: true,
                            });
                        }
                        _ => {}
                    }

                    if let Some(other_routine_id) = seen_envelope_bindings
                        .insert(connector.binding_id.as_str(), envelope.routine_id.as_str())
                    {
                        issues.push(PlanValidationIssue {
                    code: "shared_credential_envelope_entry".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("credential_envelopes[{routine_index}]"),
                    message: format!(
                        "Binding `{}` is entitled to both routine `{}` and routine `{}` without an explicit sharing justification.",
                        connector.binding_id, other_routine_id, envelope.routine_id
                    ),
                    blocking: true,
                });
                        issues.push(PlanValidationIssue {
                    code: "credential_leakage_attempt".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("credential_envelopes[{routine_index}]"),
                    message: format!(
                        "Credential binding `{}` appears in more than one routine envelope, which indicates an attempted cross-routine credential leak.",
                        connector.binding_id
                    ),
                    blocking: true,
                });
                    }
                }
            }
        }
    }

    let mut seen_context_object_ids = std::collections::BTreeSet::new();
    for (context_index, context_object) in plan.context_objects.iter().enumerate() {
        if !seen_context_object_ids.insert(context_object.context_object_id.as_str()) {
            issues.push(PlanValidationIssue {
                code: "duplicate_context_object_id".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].context_object_id"),
                message: format!(
                    "Context object id `{}` appears more than once in the plan package.",
                    context_object.context_object_id
                ),
                blocking: true,
            });
        }

        let Some(owner_routine) = plan
            .routine_graph
            .iter()
            .find(|routine| routine.routine_id == context_object.owner_routine_id)
        else {
            issues.push(PlanValidationIssue {
                code: "context_object_invalid_routine_reference".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].owner_routine_id"),
                message: format!(
                    "Context object `{}` references unknown owner routine `{}`.",
                    context_object.context_object_id, context_object.owner_routine_id
                ),
                blocking: true,
            });
            continue;
        };

        if is_seeded_context_kind(&context_object.kind) {
            let expected_scope = match context_object.kind.as_str() {
                "mission_goal" => crate::plan_package::ContextObjectScope::Mission,
                "workspace_environment" => crate::plan_package::ContextObjectScope::Plan,
                _ => unreachable!(),
            };
            if context_object.scope != expected_scope {
                issues.push(PlanValidationIssue {
                    code: "context_object_invalid_seed_shape".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("context_objects[{context_index}].scope"),
                    message: format!(
                        "Seeded context object `{}` must use scope `{:?}`.",
                        context_object.context_object_id, expected_scope
                    ),
                    blocking: true,
                });
            }
            if context_object.producer_step_id.is_some() || context_object.artifact_ref.is_some() {
                issues.push(PlanValidationIssue {
                    code: "context_object_invalid_seed_shape".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("context_objects[{context_index}]"),
                    message: format!(
                        "Seeded context object `{}` must not declare producer_step_id or artifact_ref.",
                        context_object.context_object_id
                    ),
                    blocking: true,
                });
            }
            if context_object.kind == "mission_goal"
                && !context_object
                    .data_scope_refs
                    .iter()
                    .any(|scope_ref| scope_ref == "mission.goal")
            {
                issues.push(PlanValidationIssue {
                    code: "context_object_invalid_seed_shape".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("context_objects[{context_index}].data_scope_refs"),
                    message: format!(
                        "Seeded mission goal context object `{}` must reference `mission.goal`.",
                        context_object.context_object_id
                    ),
                    blocking: true,
                });
            }
        }

        if context_object.provenance.plan_id != plan.plan_id
            || context_object.provenance.routine_id != context_object.owner_routine_id
            || context_object.provenance.plan_id.trim().is_empty()
            || context_object.provenance.routine_id.trim().is_empty()
        {
            issues.push(PlanValidationIssue {
                code: "context_object_invalid_provenance".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].provenance"),
                message: format!(
                    "Context object `{}` provenance must match the current plan and owner routine.",
                    context_object.context_object_id
                ),
                blocking: true,
            });
        }

        if context_object.producer_step_id != context_object.provenance.step_id {
            issues.push(PlanValidationIssue {
                code: "context_object_invalid_provenance".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].provenance"),
                message: format!(
                    "Context object `{}` provenance step does not match the producer step reference.",
                    context_object.context_object_id
                ),
                blocking: true,
            });
        }

        if matches!(
            context_object.validation_status,
            crate::plan_package::ContextValidationStatus::Valid
                | crate::plan_package::ContextValidationStatus::Invalid
        ) {
            issues.push(PlanValidationIssue {
                code: "context_object_invalid_validation_shape".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].validation_status"),
                message: format!(
                    "Context object `{}` cannot be marked resolved in the preview-only implementation.",
                    context_object.context_object_id
                ),
                blocking: true,
            });
        }

        if matches!(context_object.freshness_window_hours, Some(0)) {
            issues.push(PlanValidationIssue {
                code: "context_object_invalid_freshness".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].freshness_window_hours"),
                message: format!(
                    "Context object `{}` must use a positive freshness window when one is declared.",
                    context_object.context_object_id
                ),
                blocking: true,
            });
        }

        if context_object.declared_consumers.is_empty() {
            issues.push(PlanValidationIssue {
                code: "context_object_missing_declared_consumers".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].declared_consumers"),
                message: format!(
                    "Context object `{}` must declare at least one consumer routine.",
                    context_object.context_object_id
                ),
                blocking: true,
            });
        }

        for consumer in &context_object.declared_consumers {
            if !plan
                .routine_graph
                .iter()
                .any(|routine| routine.routine_id == *consumer)
            {
                issues.push(PlanValidationIssue {
                    code: "context_object_invalid_routine_reference".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("context_objects[{context_index}].declared_consumers"),
                    message: format!(
                        "Context object `{}` references unknown consumer routine `{}`.",
                        context_object.context_object_id, consumer
                    ),
                    blocking: true,
                });
            }
        }

        if context_object
            .declared_consumers
            .iter()
            .any(|consumer| consumer != &context_object.owner_routine_id)
            && matches!(
                owner_routine.data_scope.cross_routine_visibility,
                crate::plan_package::CrossRoutineVisibility::None
            )
        {
            issues.push(PlanValidationIssue {
                code: "context_object_scope_leak".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}]"),
                message: format!(
                    "Context object `{}` declares cross-routine consumers, but owner routine `{}` does not allow cross-routine visibility.",
                    context_object.context_object_id, context_object.owner_routine_id
                ),
                blocking: true,
            });
            issues.push(PlanValidationIssue {
                code: "context_scope_escalation_attempt".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}]"),
                message: format!(
                    "Context object `{}` attempted to escalate scope beyond owner routine `{}` visibility.",
                    context_object.context_object_id, context_object.owner_routine_id
                ),
                blocking: true,
            });
        }

        let producer_step = context_object
            .producer_step_id
            .as_ref()
            .and_then(|step_id| {
                owner_routine
                    .steps
                    .iter()
                    .find(|step| &step.step_id == step_id)
            });
        if context_object.producer_step_id.is_some() && producer_step.is_none() {
            issues.push(PlanValidationIssue {
                code: "context_object_missing_producer".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].producer_step_id"),
                message: format!(
                    "Context object `{}` references producer step `{}` that does not exist in owner routine `{}`.",
                    context_object.context_object_id,
                    context_object.producer_step_id.as_deref().unwrap_or_default(),
                    context_object.owner_routine_id
                ),
                blocking: true,
            });
        }

        if let Some(artifact_ref) = context_object.artifact_ref.as_ref() {
            let artifact_resolves = producer_step
                .map(|step| {
                    step.artifacts
                        .iter()
                        .any(|artifact| artifact == artifact_ref)
                })
                .unwrap_or(false);
            if !artifact_resolves {
                issues.push(PlanValidationIssue {
                    code: "context_object_unbacked_by_artifact".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("context_objects[{context_index}].artifact_ref"),
                    message: format!(
                        "Context object `{}` references artifact `{}` that is not produced by its producer step.",
                        context_object.context_object_id, artifact_ref
                    ),
                    blocking: true,
                });
            }
        } else if !is_seeded_context_kind(&context_object.kind) {
            issues.push(PlanValidationIssue {
                code: "context_object_missing_artifact_ref".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("context_objects[{context_index}].artifact_ref"),
                message: format!(
                    "Context object `{}` must reference a producer artifact in the first implementation.",
                    context_object.context_object_id
                ),
                blocking: true,
            });
        }

        for scope_ref in &context_object.data_scope_refs {
            let covered = owner_routine
                .data_scope
                .readable_paths
                .iter()
                .chain(owner_routine.data_scope.writable_paths.iter())
                .any(|allowed| path_patterns_overlap(scope_ref, allowed));
            let denied = owner_routine
                .data_scope
                .denied_paths
                .iter()
                .any(|denied| path_patterns_overlap(scope_ref, denied));
            if !covered || denied {
                issues.push(PlanValidationIssue {
                    code: "context_object_invalid_data_scope_ref".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: format!("context_objects[{context_index}].data_scope_refs"),
                    message: format!(
                        "Context object `{}` references scope `{}` outside the owner routine's allowed data scope.",
                        context_object.context_object_id, scope_ref
                    ),
                    blocking: true,
                });
            }
        }
    }

    if plan.approval_policy.is_none() {
        issues.push(PlanValidationIssue {
            code: "approval_policy_missing".to_string(),
            severity: PlanValidationSeverity::Error,
            path: "approval_policy".to_string(),
            message: "Plan package must declare an approval policy before apply or activation."
                .to_string(),
            blocking: true,
        });
    }

    match plan.inter_routine_policy.as_ref() {
        None => {
            issues.push(PlanValidationIssue {
                code: "inter_routine_policy_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "inter_routine_policy".to_string(),
                message:
                    "Plan package must declare inter_routine_policy before apply or activation."
                        .to_string(),
                blocking: true,
            });
        }
        Some(policy) => {
            if policy.shared_memory_access
                && policy
                    .shared_memory_justification
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none()
            {
                issues.push(PlanValidationIssue {
                    code: "shared_memory_requires_justification".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: "inter_routine_policy".to_string(),
                    message: "shared_memory_access=true requires explicit justification."
                        .to_string(),
                    blocking: true,
                });
            }

            if !policy.artifact_handoff_validation {
                issues.push(PlanValidationIssue {
                    code: "artifact_handoff_validation_required".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: "inter_routine_policy".to_string(),
                    message: "artifact_handoff_validation must remain enabled for compartmentalized plans."
                        .to_string(),
                    blocking: true,
                });
            }

            if matches!(
                policy.peer_visibility,
                crate::plan_package::PeerVisibility::GoalOnly
            ) {
                issues.push(PlanValidationIssue {
                    code: "peer_visibility_too_broad".to_string(),
                    severity: PlanValidationSeverity::Error,
                    path: "inter_routine_policy".to_string(),
                    message: "peer_visibility=goal_only is broader than the current compartmentalized default."
                        .to_string(),
                    blocking: true,
                });
            }
        }
    }

    for (index, entry) in plan.precedence_log.iter().enumerate() {
        if entry.path.trim().is_empty() {
            issues.push(PlanValidationIssue {
                code: "precedence_log_path_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("precedence_log[{index}].path"),
                message: "Precedence log entries must include a resolved field path.".to_string(),
                blocking: true,
            });
        }
        if entry.resolution_rule.trim().is_empty() {
            issues.push(PlanValidationIssue {
                code: "precedence_log_resolution_rule_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("precedence_log[{index}].resolution_rule"),
                message: "Precedence log entries must record the applied resolution rule."
                    .to_string(),
                blocking: true,
            });
        }
        let source_value_present = match entry.source_tier {
            PrecedenceSourceTier::CompilerDefault => entry.compiler_default.is_some(),
            PrecedenceSourceTier::UserOverride => entry.user_override.is_some(),
            PrecedenceSourceTier::ApprovedPlanState => entry.approved_plan_state.is_some(),
        };
        if !source_value_present {
            issues.push(PlanValidationIssue {
                code: "precedence_log_source_value_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("precedence_log[{index}]"),
                message:
                    "Precedence log entries must include the source-tier value they resolved from."
                        .to_string(),
                blocking: true,
            });
        }
        if entry.resolved_value.is_none() {
            issues.push(PlanValidationIssue {
                code: "precedence_log_resolved_value_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: format!("precedence_log[{index}].resolved_value"),
                message: "Precedence log entries must include the final resolved value."
                    .to_string(),
                blocking: true,
            });
        }
    }

    if let Some(plan_diff) = plan.plan_diff.as_ref() {
        if plan_diff.to_revision <= plan_diff.from_revision {
            issues.push(PlanValidationIssue {
                code: "plan_diff_revision_order_invalid".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "plan_diff".to_string(),
                message: "plan_diff.to_revision must be greater than plan_diff.from_revision."
                    .to_string(),
                blocking: true,
            });
        }
        if plan_diff.to_revision != plan.plan_revision {
            issues.push(PlanValidationIssue {
                code: "plan_diff_revision_mismatch".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "plan_diff.to_revision".to_string(),
                message: "plan_diff.to_revision must match the plan package revision.".to_string(),
                blocking: true,
            });
        }
        let breaking_count = plan_diff
            .changed_fields
            .iter()
            .filter(|field| field.breaking)
            .count();
        let revalidation_required = plan_diff
            .changed_fields
            .iter()
            .any(|field| field.requires_revalidation);
        let reapproval_required = plan_diff
            .changed_fields
            .iter()
            .any(|field| field.requires_reapproval);
        if plan_diff.summary.changed_count != plan_diff.changed_fields.len() {
            issues.push(PlanValidationIssue {
                code: "plan_diff_summary_count_mismatch".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "plan_diff.summary.changed_count".to_string(),
                message: "plan_diff summary changed_count must match changed_fields length."
                    .to_string(),
                blocking: true,
            });
        }
        if plan_diff.summary.breaking_count != breaking_count {
            issues.push(PlanValidationIssue {
                code: "plan_diff_summary_breaking_count_mismatch".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "plan_diff.summary.breaking_count".to_string(),
                message: "plan_diff summary breaking_count must match changed breaking fields."
                    .to_string(),
                blocking: true,
            });
        }
        if plan_diff.summary.revalidation_required != revalidation_required {
            issues.push(PlanValidationIssue {
                code: "plan_diff_summary_revalidation_mismatch".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "plan_diff.summary.revalidation_required".to_string(),
                message:
                    "plan_diff summary revalidation_required must reflect changed field flags."
                        .to_string(),
                blocking: true,
            });
        }
        if plan_diff.summary.reapproval_required != reapproval_required {
            issues.push(PlanValidationIssue {
                code: "plan_diff_summary_reapproval_mismatch".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "plan_diff.summary.reapproval_required".to_string(),
                message: "plan_diff summary reapproval_required must reflect changed field flags."
                    .to_string(),
                blocking: true,
            });
        }
    }

    if let Some(trigger_record) = plan.manual_trigger_record.as_ref() {
        if trigger_record.trigger_id.trim().is_empty() {
            issues.push(PlanValidationIssue {
                code: "manual_trigger_record_trigger_id_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "manual_trigger_record.trigger_id".to_string(),
                message: "manual_trigger_record must include a trigger_id.".to_string(),
                blocking: true,
            });
        }
        if trigger_record.triggered_by.trim().is_empty() {
            issues.push(PlanValidationIssue {
                code: "manual_trigger_record_triggered_by_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "manual_trigger_record.triggered_by".to_string(),
                message: "manual_trigger_record must record who triggered the run.".to_string(),
                blocking: true,
            });
        }
        if trigger_record.triggered_at.trim().is_empty() {
            issues.push(PlanValidationIssue {
                code: "manual_trigger_record_triggered_at_missing".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "manual_trigger_record.triggered_at".to_string(),
                message: "manual_trigger_record must record when the run was triggered."
                    .to_string(),
                blocking: true,
            });
        }
        if trigger_record.plan_id != plan.plan_id {
            issues.push(PlanValidationIssue {
                code: "manual_trigger_record_plan_id_mismatch".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "manual_trigger_record.plan_id".to_string(),
                message: "manual_trigger_record.plan_id must match the plan package id."
                    .to_string(),
                blocking: true,
            });
        }
        if trigger_record.plan_revision != plan.plan_revision {
            issues.push(PlanValidationIssue {
                code: "manual_trigger_record_plan_revision_mismatch".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "manual_trigger_record.plan_revision".to_string(),
                message:
                    "manual_trigger_record.plan_revision must match the plan package revision."
                        .to_string(),
                blocking: true,
            });
        }
        if !plan
            .routine_graph
            .iter()
            .any(|routine| routine.routine_id == trigger_record.routine_id)
        {
            issues.push(PlanValidationIssue {
                code: "manual_trigger_record_invalid_routine".to_string(),
                severity: PlanValidationSeverity::Error,
                path: "manual_trigger_record.routine_id".to_string(),
                message: "manual_trigger_record.routine_id must reference a routine in the plan."
                    .to_string(),
                blocking: true,
            });
        }
    }

    let blocker_count = issues.iter().filter(|issue| issue.blocking).count();
    let warning_count = issues.len() - blocker_count;
    let success_criteria_evaluation = derive_success_criteria_evaluation_for_plan(plan);
    let validation_state = PlanValidationState {
        required_connectors_mapped: Some(
            !issues
                .iter()
                .any(|issue| issue.code == "required_connector_unresolved"),
        ),
        directories_writable: None,
        schedules_valid: Some(!issues.iter().any(|issue| issue.code == "invalid_schedule")),
        models_resolved: None,
        dependencies_resolvable: Some(!issues.iter().any(|issue| {
            issue.code == "missing_routine_dependency"
                || issue.code == "missing_step_dependency"
                || issue.code == "cyclic_step_dependencies"
        })),
        approvals_complete: Some(
            !issues
                .iter()
                .any(|issue| issue.code == "approval_policy_missing"),
        ),
        degraded_modes_acknowledged: Some(
            !issues
                .iter()
                .any(|issue| issue.code == "required_connector_unresolved"),
        ),
        data_scopes_valid: Some(!issues.iter().any(|issue| {
            issue.code == "conflicting_data_scope"
                || issue.code == "empty_readable_scope"
                || issue.code == "empty_writable_scope"
                || issue.code == "cross_routine_scope_overlap"
                || issue.code == "denied_path_overlaps_output_root"
                || issue.code == "writable_path_outside_output_roots"
        })),
        audit_scopes_valid: Some(
            !issues
                .iter()
                .any(|issue| issue.code == "named_audit_roles_missing"),
        ),
        mission_context_scopes_valid: Some(
            !issues
                .iter()
                .any(|issue| issue.code == "full_plan_scope_requires_justification"),
        ),
        inter_routine_policy_complete: Some(!issues.iter().any(|issue| {
            issue.code == "inter_routine_policy_missing"
                || issue.code == "shared_memory_requires_justification"
                || issue.code == "artifact_handoff_validation_required"
                || issue.code == "peer_visibility_too_broad"
        })),
        credential_envelopes_valid: Some(!issues.iter().any(|issue| {
            issue.code == "credential_envelope_missing"
                || issue.code == "credential_envelope_entitlements_mismatch"
                || issue.code == "credential_envelope_denied_mismatch"
                || issue.code == "credential_envelope_unknown_binding"
                || issue.code == "credential_envelope_binding_capability_mismatch"
                || issue.code == "shared_credential_envelope_entry"
                || issue.code == "credential_leakage_attempt"
        })),
        compartmentalized_activation_ready: Some(!issues.iter().any(|issue| {
            issue.code == "cross_routine_scope_overlap"
                || issue.code == "denied_path_overlaps_output_root"
                || issue.code == "writable_path_outside_output_roots"
                || issue.code == "credential_envelope_missing"
                || issue.code == "credential_envelope_entitlements_mismatch"
                || issue.code == "credential_envelope_denied_mismatch"
                || issue.code == "credential_envelope_unknown_binding"
                || issue.code == "credential_envelope_binding_capability_mismatch"
                || issue.code == "shared_credential_envelope_entry"
                || issue.code == "shared_memory_requires_justification"
                || issue.code == "artifact_handoff_validation_required"
                || issue.code == "peer_visibility_too_broad"
                || issue.code == "missing_context_object_ref"
                || issue.code == "context_read_consumer_violation"
                || issue.code == "cross_routine_prompt_injection_attempt"
                || issue.code == "context_write_producer_mismatch"
                || issue.code == "direct_peer_invocation_attempt"
                || issue.code == "duplicate_context_object_id"
                || issue.code == "context_object_invalid_routine_reference"
                || issue.code == "context_object_invalid_seed_shape"
                || issue.code == "context_object_invalid_provenance"
                || issue.code == "context_object_invalid_validation_shape"
                || issue.code == "context_object_invalid_freshness"
                || issue.code == "context_object_missing_declared_consumers"
                || issue.code == "context_object_scope_leak"
                || issue.code == "context_scope_escalation_attempt"
                || issue.code == "context_object_missing_producer"
                || issue.code == "context_object_unbacked_by_artifact"
                || issue.code == "context_object_missing_artifact_ref"
                || issue.code == "context_object_invalid_data_scope_ref"
                || issue.code == "credential_leakage_attempt"
        })),
        context_objects_valid: Some(!issues.iter().any(|issue| {
            issue.code == "missing_context_object_ref"
                || issue.code == "context_read_consumer_violation"
                || issue.code == "cross_routine_prompt_injection_attempt"
                || issue.code == "context_write_producer_mismatch"
                || issue.code == "direct_peer_invocation_attempt"
                || issue.code == "duplicate_context_object_id"
                || issue.code == "context_object_invalid_routine_reference"
                || issue.code == "context_object_invalid_seed_shape"
                || issue.code == "context_object_invalid_provenance"
                || issue.code == "context_object_invalid_validation_shape"
                || issue.code == "context_object_invalid_freshness"
                || issue.code == "context_object_missing_declared_consumers"
                || issue.code == "context_object_scope_leak"
                || issue.code == "context_scope_escalation_attempt"
                || issue.code == "context_object_missing_producer"
                || issue.code == "context_object_unbacked_by_artifact"
                || issue.code == "context_object_missing_artifact_ref"
                || issue.code == "context_object_invalid_data_scope_ref"
        })),
        success_criteria_evaluation: Some(success_criteria_evaluation.clone()),
    };

    PlanValidationReport {
        ready_for_apply: blocker_count == 0,
        ready_for_activation: blocker_count == 0,
        blocker_count,
        warning_count,
        validation_state,
        success_criteria_evaluation: Some(success_criteria_evaluation),
        issues,
    }
}

