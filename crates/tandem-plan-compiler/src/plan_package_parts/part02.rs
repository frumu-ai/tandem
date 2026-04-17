pub fn derive_model_routing_resolution_for_plan(plan: &PlanPackage) -> ModelRoutingReport {
    let mut entries = plan
        .routine_graph
        .iter()
        .flat_map(|routine| {
            routine.steps.iter().map(|step| {
                let tier = step
                    .model_policy
                    .primary
                    .as_ref()
                    .map(|selection| selection.tier.clone())
                    .unwrap_or(ModelTier::Mid);
                let resolved = step.model_policy.primary.is_some();
                ModelRoutingEntry {
                    step_id: step.step_id.clone(),
                    tier,
                    provider_id: None,
                    model_id: None,
                    resolved,
                    status: if resolved {
                        "tier_assigned".to_string()
                    } else {
                        "unrouted".to_string()
                    },
                    reason: if resolved {
                        Some("step declares a routing tier but provider/model selection is still pending".to_string())
                    } else {
                        Some("step does not declare a model routing tier yet".to_string())
                    },
                }
            })
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| left.step_id.cmp(&right.step_id));

    let tier_assigned_count = entries.iter().filter(|entry| entry.resolved).count();
    let provider_unresolved_count = entries
        .iter()
        .filter(|entry| entry.provider_id.is_none())
        .count();

    ModelRoutingReport {
        tier_assigned_count,
        provider_unresolved_count,
        entries,
    }
}

fn success_criteria_declared_fields(criteria: &SuccessCriteria) -> Vec<String> {
    let mut declared_fields = Vec::new();
    if !criteria.required_artifacts.is_empty() {
        declared_fields.push("required_artifacts".to_string());
    }
    if criteria
        .minimum_viable_completion
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        declared_fields.push("minimum_viable_completion".to_string());
    }
    if criteria
        .minimum_output
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        declared_fields.push("minimum_output".to_string());
    }
    if criteria.freshness_window_hours.is_some() {
        declared_fields.push("freshness_window_hours".to_string());
    }
    declared_fields
}

fn success_criteria_entry(
    subject: SuccessCriteriaSubjectKind,
    routine_id: Option<&str>,
    step_id: Option<&str>,
    criteria: &SuccessCriteria,
) -> SuccessCriteriaEvaluationEntry {
    let declared_fields = success_criteria_declared_fields(criteria);
    let status = if declared_fields.is_empty() {
        SuccessCriteriaEvaluationStatus::Missing
    } else {
        SuccessCriteriaEvaluationStatus::Defined
    };
    SuccessCriteriaEvaluationEntry {
        subject,
        routine_id: routine_id.map(|value| value.to_string()),
        step_id: step_id.map(|value| value.to_string()),
        required_artifacts: criteria.required_artifacts.clone(),
        minimum_viable_completion: criteria.minimum_viable_completion.clone(),
        minimum_output: criteria.minimum_output.clone(),
        freshness_window_hours: criteria.freshness_window_hours,
        declared_fields,
        status,
    }
}

pub fn derive_success_criteria_evaluation_for_plan(
    plan: &PlanPackage,
) -> SuccessCriteriaEvaluationReport {
    let mut entries = Vec::new();
    entries.push(success_criteria_entry(
        SuccessCriteriaSubjectKind::Plan,
        None,
        None,
        &plan.success_criteria,
    ));
    for routine in &plan.routine_graph {
        entries.push(success_criteria_entry(
            SuccessCriteriaSubjectKind::Routine,
            Some(&routine.routine_id),
            None,
            &routine.success_criteria,
        ));
        for step in &routine.steps {
            entries.push(success_criteria_entry(
                SuccessCriteriaSubjectKind::Step,
                Some(&routine.routine_id),
                Some(&step.step_id),
                &step.success_criteria,
            ));
        }
    }
    let defined_count = entries
        .iter()
        .filter(|entry| entry.status == SuccessCriteriaEvaluationStatus::Defined)
        .count();
    let missing_count = entries.len().saturating_sub(defined_count);
    SuccessCriteriaEvaluationReport {
        total_subjects: entries.len(),
        defined_count,
        missing_count,
        entries,
    }
}

pub fn derive_credential_envelopes_for_plan(plan: &PlanPackage) -> Vec<CredentialEnvelope> {
    derive_credential_envelopes(&plan.routine_graph, &plan.connector_bindings)
}

fn derive_credential_envelopes(
    routines: &[RoutinePackage],
    connector_bindings: &[ConnectorBinding],
) -> Vec<CredentialEnvelope> {
    let binding_refs = connector_bindings
        .iter()
        .map(|binding| CredentialBindingRef {
            capability: binding.capability.clone(),
            binding_id: binding.binding_id.clone(),
        })
        .collect::<Vec<_>>();

    routines
        .iter()
        .map(|routine| {
            let required_capabilities = required_capabilities_for_routine(routine);

            let entitled_connectors = binding_refs
                .iter()
                .filter(|binding| required_capabilities.contains(&binding.capability))
                .cloned()
                .collect::<Vec<_>>();
            let denied_connectors = binding_refs
                .iter()
                .filter(|binding| !required_capabilities.contains(&binding.capability))
                .cloned()
                .collect::<Vec<_>>();

            CredentialEnvelope {
                routine_id: routine.routine_id.clone(),
                entitled_connectors,
                denied_connectors,
                envelope_issued_at: None,
                envelope_expires_at: None,
                issuing_authority: Some("engine".to_string()),
            }
        })
        .collect()
}

fn derive_context_objects(
    plan_id: &str,
    mission_goal: &str,
    workspace_root: &str,
    routine: &RoutinePackage,
) -> Vec<ContextObject> {
    let mut context_objects = vec![
        ContextObject {
            context_object_id: mission_goal_context_object_id(&routine.routine_id),
            name: "Mission goal".to_string(),
            kind: "mission_goal".to_string(),
            scope: ContextObjectScope::Mission,
            owner_routine_id: routine.routine_id.clone(),
            producer_step_id: None,
            declared_consumers: vec![routine.routine_id.clone()],
            artifact_ref: None,
            data_scope_refs: vec!["mission.goal".to_string()],
            freshness_window_hours: None,
            validation_status: ContextValidationStatus::Pending,
            provenance: ContextObjectProvenance {
                plan_id: plan_id.to_string(),
                routine_id: routine.routine_id.clone(),
                step_id: None,
            },
            summary: Some(mission_goal.to_string()),
        },
        ContextObject {
            context_object_id: workspace_environment_context_object_id(&routine.routine_id),
            name: "Workspace environment".to_string(),
            kind: "workspace_environment".to_string(),
            scope: ContextObjectScope::Plan,
            owner_routine_id: routine.routine_id.clone(),
            producer_step_id: None,
            declared_consumers: vec![routine.routine_id.clone()],
            artifact_ref: None,
            data_scope_refs: routine
                .data_scope
                .readable_paths
                .iter()
                .filter(|path| path.as_str() != "mission.goal")
                .take(1)
                .cloned()
                .collect(),
            freshness_window_hours: None,
            validation_status: ContextValidationStatus::Pending,
            provenance: ContextObjectProvenance {
                plan_id: plan_id.to_string(),
                routine_id: routine.routine_id.clone(),
                step_id: None,
            },
            summary: Some(workspace_root.to_string()),
        },
    ];

    context_objects.extend(routine.steps.iter().flat_map(|step| {
        step.artifacts.iter().map(|artifact| ContextObject {
            context_object_id: handoff_context_object_id(
                &routine.routine_id,
                &step.step_id,
                artifact,
            ),
            name: format!("{} handoff", step.label),
            kind: "step_output_handoff".to_string(),
            scope: ContextObjectScope::Handoff,
            owner_routine_id: routine.routine_id.clone(),
            producer_step_id: Some(step.step_id.clone()),
            declared_consumers: vec![routine.routine_id.clone()],
            artifact_ref: Some(artifact.clone()),
            data_scope_refs: routine.data_scope.writable_paths.clone(),
            freshness_window_hours: None,
            validation_status: ContextValidationStatus::Pending,
            provenance: ContextObjectProvenance {
                plan_id: plan_id.to_string(),
                routine_id: routine.routine_id.clone(),
                step_id: Some(step.step_id.clone()),
            },
            summary: step.success_criteria.minimum_output.clone(),
        })
    }));

    context_objects
}

fn mission_goal_context_object_id(routine_id: &str) -> String {
    format!("ctx:{routine_id}:mission.goal")
}

fn workspace_environment_context_object_id(routine_id: &str) -> String {
    format!("ctx:{routine_id}:workspace.environment")
}

fn handoff_context_object_id(routine_id: &str, step_id: &str, artifact: &str) -> String {
    format!("ctx:{routine_id}:{step_id}:{artifact}")
}

fn step_context_reads(routine_id: &str) -> Vec<String> {
    vec![
        mission_goal_context_object_id(routine_id),
        workspace_environment_context_object_id(routine_id),
    ]
}

fn step_context_writes(routine_id: &str, step: &WorkflowPlanStep<Value, Value>) -> Vec<String> {
    step_artifacts(step)
        .into_iter()
        .map(|artifact| handoff_context_object_id(routine_id, &step.step_id, &artifact))
        .collect()
}

fn trigger_from_schedule(
    schedule: &crate::contracts::AutomationV2ScheduleJson,
) -> TriggerDefinition {
    let trigger_type = match schedule.schedule_type {
        AutomationV2ScheduleType::Manual => TriggerKind::Manual,
        AutomationV2ScheduleType::Cron | AutomationV2ScheduleType::Interval => {
            TriggerKind::Scheduled
        }
    };

    let schedule_string = match schedule.schedule_type {
        AutomationV2ScheduleType::Cron => schedule.cron_expression.clone(),
        AutomationV2ScheduleType::Interval => schedule
            .interval_seconds
            .map(|seconds| format!("interval:{seconds}")),
        AutomationV2ScheduleType::Manual => None,
    };
    let timezone = match schedule.schedule_type {
        AutomationV2ScheduleType::Cron | AutomationV2ScheduleType::Interval => {
            Some(schedule.timezone.clone())
        }
        AutomationV2ScheduleType::Manual => None,
    };

    TriggerDefinition {
        trigger_type,
        schedule: schedule_string,
        timezone,
    }
}

fn step_label(step: &WorkflowPlanStep<Value, Value>) -> String {
    if !step.objective.trim().is_empty() {
        step.objective.trim().to_string()
    } else {
        step.step_id.replace(['_', '-'], " ")
    }
}

fn input_names(step: &WorkflowPlanStep<Value, Value>) -> Vec<String> {
    step.input_refs
        .iter()
        .filter_map(|input| {
            input
                .get("alias")
                .and_then(Value::as_str)
                .or_else(|| input.get("from_step_id").and_then(Value::as_str))
                .map(|value| value.to_string())
        })
        .collect()
}

fn output_contract_kind(contract: &Value) -> Option<String> {
    contract
        .get("kind")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn step_outputs(step: &WorkflowPlanStep<Value, Value>) -> Vec<String> {
    match step.output_contract.as_ref().and_then(output_contract_kind) {
        Some(kind) => vec![format!("{}:{kind}", step.step_id)],
        None => Vec::new(),
    }
}

fn step_artifacts(step: &WorkflowPlanStep<Value, Value>) -> Vec<String> {
    if step.output_contract.is_some() {
        vec![format!("{}.artifact", step.step_id)]
    } else {
        Vec::new()
    }
}

fn step_connector_requirements(step: &WorkflowPlanStep<Value, Value>) -> Vec<ConnectorRequirement> {
    let mut requirements = Vec::new();

    if step
        .metadata
        .as_ref()
        .and_then(|value| value.get("builder"))
        .and_then(|value| value.get("web_research_expected"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        requirements.push(ConnectorRequirement {
            capability: "websearch".to_string(),
            required: true,
        });
    }

    if let Some(required_tools) = step
        .output_contract
        .as_ref()
        .and_then(|value| value.get("enforcement"))
        .and_then(|value| value.get("required_tools"))
        .and_then(Value::as_array)
    {
        for tool in required_tools.iter().filter_map(Value::as_str) {
            let capability = tool.trim();
            if capability.is_empty()
                || requirements
                    .iter()
                    .any(|existing| existing.capability == capability)
            {
                continue;
            }
            requirements.push(ConnectorRequirement {
                capability: capability.to_string(),
                required: true,
            });
        }
    }

    requirements
}

fn semantic_kind_for_plan(plan: &crate::contracts::WorkflowPlanJson) -> RoutineSemanticKind {
    if plan
        .steps
        .iter()
        .any(|step| step.kind.to_ascii_lowercase().contains("research"))
    {
        RoutineSemanticKind::Research
    } else {
        RoutineSemanticKind::Mixed
    }
}

fn normalize_token(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn canonical_overlap_hash(plan: &crate::contracts::WorkflowPlanJson) -> String {
    let output_kinds = plan
        .steps
        .iter()
        .filter_map(|step| step.output_contract.as_ref().and_then(output_contract_kind))
        .collect::<Vec<_>>()
        .join("|");
    let routine_semantics = plan
        .steps
        .iter()
        .map(|step| {
            format!(
                "{}:{}",
                normalize_token(&step.step_id),
                normalize_token(&step.kind)
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let source_set = {
        let mut values = plan
            .requires_integrations
            .iter()
            .chain(plan.allowed_mcp_servers.iter())
            .map(|value| normalize_token(value))
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values.join("|")
    };
    let topic = plan
        .description
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&plan.title);
    let normalized = [
        format!("goal={}", normalize_token(&plan.normalized_prompt)),
        format!("topic={}", normalize_token(topic)),
        format!("source_set={source_set}"),
        format!("outputs={output_kinds}"),
        format!("routine_semantics={routine_semantics}"),
    ]
    .join("\n");
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

fn default_overlap_policy(plan: &crate::contracts::WorkflowPlanJson) -> OverlapPolicy {
    OverlapPolicy {
        exact_identity: Some(OverlapIdentity {
            hash_version: Some(1),
            canonical_hash: Some(canonical_overlap_hash(plan)),
            normalized_fields: vec![
                "goal".to_string(),
                "topic".to_string(),
                "source_set".to_string(),
                "outputs".to_string(),
                "routine_semantics".to_string(),
            ],
        }),
        semantic_identity: Some(SemanticIdentity {
            similarity_model: Some("text-embedding-3-large".to_string()),
            semantic_signature: None,
            similarity_threshold: Some(0.85),
        }),
        overlap_log: Vec::new(),
    }
}

pub fn compile_workflow_plan_preview_package(
    plan: &crate::contracts::WorkflowPlanJson,
    owner_id: Option<&str>,
) -> PlanPackage {
    let owner_id = owner_id.unwrap_or("workflow_planner");
    let routine_id = format!("{}_routine", plan.plan_id);
    let steps = plan
        .steps
        .iter()
        .map(|step| StepPackage {
            step_id: step.step_id.clone(),
            label: step_label(step),
            kind: step.kind.clone(),
            action: step.objective.clone(),
            inputs: input_names(step),
            outputs: step_outputs(step),
            dependencies: step.depends_on.clone(),
            context_reads: step_context_reads(&routine_id),
            context_writes: step_context_writes(&routine_id, step),
            connector_requirements: step_connector_requirements(step),
            model_policy: StepModelPolicy::default(),
            approval_policy: ApprovalMode::InternalOnly,
            success_criteria: SuccessCriteria {
                required_artifacts: step_artifacts(step),
                minimum_viable_completion: None,
                minimum_output: step
                    .output_contract
                    .as_ref()
                    .and_then(output_contract_kind)
                    .map(|kind| format!("produce {kind} output")),
                freshness_window_hours: None,
            },
            failure_policy: StepFailurePolicy::default(),
            retry_policy: StepRetryPolicy::default(),
            artifacts: step_artifacts(step),
            provenance: None,
            notes: step.metadata.clone().map(|value| value.to_string()),
        })
        .collect::<Vec<_>>();

    let connector_intents = plan
        .requires_integrations
        .iter()
        .map(|capability| ConnectorIntent {
            capability: capability.clone(),
            why: "Required by workflow plan preview".to_string(),
            required: true,
            degraded_mode_allowed: false,
        })
        .collect::<Vec<_>>();

    let required_artifacts = steps
        .iter()
        .flat_map(|step| step.artifacts.clone())
        .collect::<Vec<_>>();

    let mut package = PlanPackage {
        plan_id: plan.plan_id.clone(),
        plan_revision: 1,
        lifecycle_state: PlanLifecycleState::Preview,
        owner: PlanOwner {
            owner_id: owner_id.to_string(),
            scope: "workspace".to_string(),
            audience: "internal".to_string(),
        },
        mission: MissionDefinition {
            goal: plan.original_prompt.clone(),
            summary: plan
                .description
                .clone()
                .or_else(|| Some(plan.title.clone())),
            domain: Some("workflow".to_string()),
        },
        success_criteria: SuccessCriteria {
            required_artifacts: required_artifacts.clone(),
            minimum_viable_completion: Some(format!(
                "Preview a routine graph with {} step(s)",
                steps.len()
            )),
            minimum_output: None,
            freshness_window_hours: None,
        },
        budget_policy: Some(default_budget_policy()),
        budget_enforcement: Some(default_budget_enforcement()),
        approval_policy: Some(ApprovalMatrix {
            internal_reports: Some(ApprovalMode::AutoApproved),
            public_posts: Some(ApprovalMode::ApprovalRequired),
            public_replies: Some(ApprovalMode::ApprovalRequired),
            outbound_email: Some(ApprovalMode::ApprovalRequired),
            connector_mutations: Some(ApprovalMode::ApprovalRequired),
            destructive_actions: Some(ApprovalMode::ApprovalRequired),
        }),
        inter_routine_policy: Some(InterRoutinePolicy {
            communication_model: CommunicationModel::ArtifactOnly,
            shared_memory_access: false,
            shared_memory_justification: None,
            peer_visibility: PeerVisibility::DeclaredOutputsOnly,
            artifact_handoff_validation: true,
        }),
        trigger_policy: Some(TriggerPolicy {
            supported: vec![
                TriggerKind::Scheduled,
                TriggerKind::Manual,
                TriggerKind::ArtifactTriggered,
                TriggerKind::DependencyTriggered,
            ],
        }),
        output_roots: Some(default_output_roots(&plan.workspace_root)),
        precedence_log: Vec::new(),
        plan_diff: None,
        manual_trigger_record: None,
        validation_state: None,
        overlap_policy: Some(default_overlap_policy(plan)),
        routine_graph: vec![RoutinePackage {
            routine_id: routine_id.clone(),
            semantic_kind: semantic_kind_for_plan(plan),
            trigger: trigger_from_schedule(&plan.schedule),
            dependencies: Vec::new(),
            dependency_resolution: default_dependency_resolution(),
            connector_resolution: default_connector_resolution(),
            data_scope: default_data_scope(&plan.workspace_root, &routine_id),
            audit_scope: default_audit_scope(),
            success_criteria: SuccessCriteria {
                required_artifacts,
                minimum_viable_completion: Some(format!(
                    "At least {} step(s) remain inspectable in preview",
                    steps.len()
                )),
                minimum_output: None,
                freshness_window_hours: None,
            },
            steps,
        }],
        connector_intents,
        connector_bindings: Vec::new(),
        connector_binding_resolution: None,
        model_routing_resolution: None,
        credential_envelopes: Vec::new(),
        context_objects: Vec::new(),
        metadata: Some(serde_json::json!({
            "source": "workflow_plan_preview",
            "planner_version": plan.planner_version,
            "plan_source": plan.plan_source,
            "execution_target": plan.execution_target,
            "allowed_mcp_servers": plan.allowed_mcp_servers,
            "save_options": plan.save_options,
        })),
    };
    package.connector_binding_resolution =
        Some(derive_connector_binding_resolution_for_plan(&package));
    package.model_routing_resolution = Some(derive_model_routing_resolution_for_plan(&package));
    package.credential_envelopes =
        derive_credential_envelopes(&package.routine_graph, &package.connector_bindings);
    package.context_objects = package
        .routine_graph
        .iter()
        .flat_map(|routine| {
            derive_context_objects(
                &package.plan_id,
                &package.mission.goal,
                &plan.workspace_root,
                routine,
            )
        })
        .collect();
    let validation = validate_plan_package(&package);
    package.validation_state = Some(validation.validation_state);
    package
}

