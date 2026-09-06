use anyhow::{ensure, Context};
use serde::Deserialize;
use tandem_enterprise_contract::{AccessPermission, VerifiedTenantContext};
use tandem_orchestrator::{AgentTemplate, SolutionTemplateOwner};
use tandem_solutions::{ComponentKind, CustomerScope};

use crate::stateful_runtime::orchestration_store::{
    CustomerConfigVersion, OrchestrationStateStore, SolutionComponentProgress,
    SolutionInstallation, SolutionInstallationInput, SolutionInstallationTransition,
};
use crate::{AppState, RoutineIdentity, SolutionRoutineOwner};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolutionStagingRequest {
    pub pack_selector: String,
    pub scope: CustomerScope,
    pub config_version: CustomerConfigVersion,
    pub reviewed_composition: String,
    pub expected_generation: Option<u64>,
}

enum Transition {
    Begin,
    Claim {
        component: String,
        attempt: String,
    },
    Record {
        component: String,
        attempt: String,
        fingerprint: String,
    },
}

impl AppState {
    async fn transition_solution(
        &self,
        verified: &VerifiedTenantContext,
        request: &SolutionStagingRequest,
        expected_generation: Option<u64>,
        transition: Transition,
    ) -> anyhow::Result<SolutionInstallation> {
        let facts = self.solution_host_facts(verified, &request.scope).await?;
        let pack = self
            .pack_manager
            .solution_artifacts(&request.pack_selector)
            .await?;
        let request = request.clone();
        let state = self.clone();
        crate::encrypted_file_store::spawn_protected_blocking(move || {
            state
                .enterprise
                .hosted_policy
                .authorize_permission(Some(&facts.context), AccessPermission::HostedAdmin)
                .map_err(anyhow::Error::msg)?;
            let store =
                OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path)?;
            let transition = match &transition {
                Transition::Begin => SolutionInstallationTransition::Begin,
                Transition::Claim { component, attempt } => SolutionInstallationTransition::Claim {
                    component_id: component,
                    attempt_id: attempt,
                },
                Transition::Record {
                    component,
                    attempt,
                    fingerprint,
                } => SolutionInstallationTransition::RecordStaged {
                    component_id: component,
                    attempt_id: attempt,
                    resource_sha256: fingerprint,
                },
            };
            store.transition_solution_installation(
                SolutionInstallationInput {
                    host_facts_sha256: Some(&facts.digest),
                    configuration: facts.configuration(&request.scope),
                    expected_config: &request.config_version,
                    blueprint: &pack.blueprint,
                    engine_version: env!("CARGO_PKG_VERSION"),
                    available_deployment_requirements: &facts.readiness,
                    approved_models: &facts.models,
                    artifacts: &pack.artifacts,
                    reviewed_composition: &request.reviewed_composition,
                },
                expected_generation,
                transition,
            )
        })
        .await?
    }

    /// Apply the reviewed intent only to disabled native resources. Each
    /// transition reloads configuration, current authority, routes and signed
    /// pack bytes. Claimed native effects reconcile at their stable IDs; this
    /// protocol must not be reused for arbitrary external connector effects.
    pub async fn stage_solution_installation(
        &self,
        verified: &VerifiedTenantContext,
        request: SolutionStagingRequest,
    ) -> anyhow::Result<SolutionInstallation> {
        ensure!(request.pack_selector.len() <= 512, "invalid pack selector");
        let mut journal = self
            .transition_solution(
                verified,
                &request,
                request.expected_generation,
                Transition::Begin,
            )
            .await?;
        ensure!(
            journal.plan.components.values().all(|component| matches!(
                component.kind,
                ComponentKind::AgentTemplate | ComponentKind::Routine
            )),
            "selected component requires an installation adapter that is not available"
        );
        let workspace = self.workspace_index.snapshot().await.root;
        for component in journal.plan.install_order.clone() {
            let attempt = match &journal.components[&component] {
                SolutionComponentProgress::Pending => {
                    let attempt = uuid::Uuid::new_v4().to_string();
                    journal = self
                        .transition_solution(
                            verified,
                            &request,
                            Some(journal.generation),
                            Transition::Claim {
                                component: component.clone(),
                                attempt: attempt.clone(),
                            },
                        )
                        .await?;
                    attempt
                }
                SolutionComponentProgress::Claimed { attempt_id }
                | SolutionComponentProgress::Staged { attempt_id, .. } => attempt_id.clone(),
            };
            // Repeating Begin revalidates current intent even on an already
            // claimed/staged resource; no lease expiry or takeover occurs.
            journal = self
                .transition_solution(
                    verified,
                    &request,
                    Some(journal.generation),
                    Transition::Begin,
                )
                .await?;
            let pack = self
                .pack_manager
                .solution_artifacts(&request.pack_selector)
                .await?;
            ensure!(
                tandem_solutions::blueprint_hash(&pack.blueprint)? == journal.plan.blueprint_sha256,
                "pack changed before native effect"
            );
            self.enterprise
                .hosted_policy
                .authorize_permission(Some(verified), AccessPermission::HostedAdmin)
                .map_err(anyhow::Error::msg)?;
            let locked = &journal.plan.components[&component];
            let artifact = pack
                .artifacts
                .get(&component)
                .context("signed component artifact missing")?;
            ensure!(
                tandem_solutions::sha256(artifact) == locked.artifact.sha256,
                "component artifact changed"
            );
            let already_staged = matches!(
                journal.components[&component],
                SolutionComponentProgress::Staged { .. }
            );
            let template_owner = SolutionTemplateOwner {
                org_id: request.scope.org_id.clone(),
                workspace_id: request.scope.workspace_id.clone(),
                deployment_id: request.scope.deployment_id.clone(),
                instance_id: request.scope.instance_id.clone(),
                component_id: component.clone(),
                composition_sha256: journal.composition_sha256.clone(),
            };
            let fingerprint = match locked.kind {
                ComponentKind::AgentTemplate => {
                    if already_staged {
                        self.agent_teams
                            .observe_solution_template(
                                &workspace,
                                &locked.resource_id,
                                &template_owner,
                            )
                            .await?
                    } else {
                        let mut template: AgentTemplate = serde_json::from_slice(artifact)?;
                        // The first text adapter is deliberately tool-free. A
                        // future capability adapter must validate real grants.
                        ensure!(
                            template.capabilities.tool_allowlist.is_empty()
                                && !template.capabilities.net_scopes.enabled,
                            "text agent requests unsupported capabilities"
                        );
                        template.template_id = locked.resource_id.clone();
                        template.default_budget.max_tokens = Some(
                            template
                                .default_budget
                                .max_tokens
                                .unwrap_or(u64::MAX)
                                .min(journal.plan.constraints.max_tokens_per_run),
                        );
                        let classes = &pack.blueprint.components[&component].model_classes;
                        ensure!(
                            classes.len() <= 1,
                            "text adapter requires one default model class"
                        );
                        if let Some(class) = classes.iter().next() {
                            let binding = &journal
                                .plan
                                .models
                                .get(class)
                                .context("reviewed model binding missing")?
                                .binding;
                            template.default_model = Some(
                                serde_json::json!({"provider_id": binding.provider, "model_id": binding.model}),
                            );
                        } else {
                            template.default_model = None;
                        }
                        self.agent_teams
                            .stage_solution_template(&workspace, template, template_owner)
                            .await?
                    }
                }
                ComponentKind::Routine => {
                    let owner = SolutionRoutineOwner {
                        instance_id: request.scope.instance_id.clone(),
                        component_id: component.clone(),
                        composition_sha256: journal.composition_sha256.clone(),
                        enabled: false,
                    };
                    let tenant = &verified.tenant_context;
                    if already_staged {
                        self.observe_solution_routine(
                            &RoutineIdentity::new(&locked.resource_id, tenant),
                            &owner,
                        )
                        .await
                    } else {
                        let mut routine = crate::solution_routine_from_artifact(
                            artifact,
                            &locked.resource_id,
                            tenant,
                        )
                        .map_err(|error| anyhow::anyhow!("routine artifact rejected: {error:?}"))?;
                        ensure!(
                            routine.allowed_tools.is_empty()
                                && routine.output_targets.is_empty()
                                && !routine.external_integrations_allowed,
                            "text routine requests unsupported external effects"
                        );
                        routine.timezone = self
                            .solution_configuration_timezone(verified, &request)
                            .await?;
                        self.stage_solution_routine(routine, owner).await
                    }
                    .map_err(|error| anyhow::anyhow!("native routine conflict: {error:?}"))?
                }
                _ => anyhow::bail!("unsupported native component"),
            };
            if let SolutionComponentProgress::Staged {
                resource_sha256, ..
            } = &journal.components[&component]
            {
                ensure!(
                    *resource_sha256 == fingerprint,
                    "staged resource drift; explicit reconciliation required"
                );
            } else {
                journal = self
                    .transition_solution(
                        verified,
                        &request,
                        Some(journal.generation),
                        Transition::Record {
                            component,
                            attempt,
                            fingerprint,
                        },
                    )
                    .await?;
            }
        }
        self.transition_solution(
            verified,
            &request,
            Some(journal.generation),
            Transition::Begin,
        )
        .await
    }

    async fn solution_configuration_timezone(
        &self,
        verified: &VerifiedTenantContext,
        request: &SolutionStagingRequest,
    ) -> anyhow::Result<String> {
        let facts = self.solution_host_facts(verified, &request.scope).await?;
        let request = request.clone();
        let path = self.automation_v2_runs_path.clone();
        crate::encrypted_file_store::spawn_protected_blocking(move || {
            let store = OrchestrationStateStore::from_automation_runs_path(&path)?;
            let stored = store
                .customer_configuration(&facts.context, &request.scope, crate::now_ms())?
                .context("customer configuration missing")?;
            ensure!(
                stored.version == request.config_version,
                "customer configuration changed before native effect"
            );
            Ok(stored.config.timezone)
        })
        .await?
    }
}
