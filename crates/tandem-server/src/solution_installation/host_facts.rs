use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::{AccessDecision, AccessPermission, VerifiedTenantContext};
use tandem_solutions::{
    canonical_json, sha256, validate_customer_config_scope, ConnectorBinding, Constraints,
    CustomerConfigInput, CustomerScope, ModelBinding,
};

use crate::AppState;

/// This section is read only from the existing operator ConfigStore. It is
/// never accepted as an API argument or inside customer configuration.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HostSettings {
    pub schema_version: u32,
    pub policy: Constraints,
    pub models: BTreeMap<String, HostModel>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HostModel {
    pub provider_id: String,
    pub model_id: String,
    /// Existing provider/account credential reference, never its value.
    pub credential_ref: String,
    /// Local echo is a diagnostic, never an implicit production fallback.
    #[serde(default)]
    pub allow_test_provider: bool,
    /// Optional runtime account-use binding. Existing installation metadata
    /// alone must never authorize access to a provider credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<super::model_accounts::HostAccountBinding>,
}

pub(super) struct HostFacts {
    pub context: VerifiedTenantContext,
    pub digest: String,
    pub policy: Constraints,
    pub references: BTreeSet<String>,
    pub connectors: BTreeMap<String, ConnectorBinding>,
    pub subjects: BTreeSet<String>,
    pub units: BTreeSet<String>,
    pub projects: BTreeSet<String>,
    pub models: BTreeMap<String, ModelBinding>,
    pub readiness: BTreeSet<String>,
}

impl HostFacts {
    pub fn configuration<'a>(&'a self, scope: &'a CustomerScope) -> CustomerConfigInput<'a> {
        CustomerConfigInput {
            verified_context: &self.context,
            selected_scope: scope,
            now_ms: crate::now_ms(),
            current_revision: None,
            expected_revision: None,
            host_policy: &self.policy,
            approved_references: &self.references,
            approved_connectors: &self.connectors,
            approved_subjects: &self.subjects,
            approved_org_units: &self.units,
            approved_projects: &self.projects,
        }
    }
}

impl AppState {
    pub(super) async fn solution_host_facts(
        &self,
        verified: &VerifiedTenantContext,
        scope: &CustomerScope,
    ) -> anyhow::Result<HostFacts> {
        let mut context = verified.clone();
        let memberships = self
            .enterprise
            .hosted_policy
            .project(&mut context)
            .map_err(anyhow::Error::msg)?
            .context("synchronized hosted identity required")?;
        self.enterprise
            .hosted_policy
            .authorize_permission(Some(&context), AccessPermission::HostedAdmin)
            .map_err(anyhow::Error::msg)?;
        crate::http::enrich_verified_context_with_org_unit_grants(
            self,
            &mut context,
            Some(memberships),
        )
        .await;
        validate_customer_config_scope(&context, scope, crate::now_ms())?;
        let layers = self.config.get_layers_value().await;
        // Project/global/runtime overlays are writable through ordinary config
        // APIs. Only the operator's managed or startup CLI layer can authorize
        // installation policy. A customer override must not widen it.
        let settings: HostSettings = serde_json::from_value(
            layers
                .get("cli")
                .and_then(|value| value.get("solution_installation"))
                .or_else(|| {
                    layers
                        .get("managed")
                        .and_then(|value| value.get("solution_installation"))
                })
                .cloned()
                .context("operator solution_installation configuration required")?,
        )?;
        ensure!(
            settings.schema_version == 1,
            "unsupported host installation settings version"
        );
        let providers = self.providers.installation_models().await;
        let mut models = BTreeMap::new();
        let mut model_routes = BTreeMap::new();
        for (binding_id, binding) in &settings.models {
            let matches: Vec<_> = providers
                .iter()
                .filter(|(info, _)| info.id == binding.provider_id)
                .collect();
            // Repeated IDs after configuration normalization are ambiguous.
            if matches.len() != 1 {
                continue;
            }
            let (info, Some(metadata)) = matches[0] else {
                continue;
            };
            if metadata.is_test_provider && !binding.allow_test_provider {
                continue;
            }
            if !info.models.iter().any(|model| {
                model.id == binding.model_id && model.provider_id == binding.provider_id
            }) {
                continue;
            }
            // A configured account must be usable by this current installer,
            // including its reviewed revision and actual loaded credential.
            // Unrelated denied bindings are omitted; the resolver will reject
            // a selected binding that is absent. Metadata alone grants nothing.
            if binding.account.is_some()
                && self
                    .authorize_solution_model_account_binding(
                        verified,
                        scope,
                        binding_id,
                        binding.clone(),
                    )
                    .await
                    .is_err()
            {
                continue;
            }
            models.insert(
                binding_id.clone(),
                ModelBinding {
                    provider: binding.provider_id.clone(),
                    model: binding.model_id.clone(),
                    credential_ref: binding.credential_ref.clone(),
                    uses_network: metadata.uses_network,
                },
            );
            model_routes.insert(binding_id.clone(), metadata.clone());
        }
        let tenant = &context.tenant_context;
        let view = self
            .enterprise_org_unit_view(tenant)
            .await
            .map_err(anyhow::Error::msg)?;
        let units: BTreeSet<_> = view
            .units
            .iter()
            .filter(|unit| unit.state.is_active())
            .map(|unit| unit.unit_id.clone())
            .collect();
        let mut references = BTreeSet::from([format!("profile-ref:{}", scope.org_id)]);
        let mut projects = BTreeSet::new();
        let mut sources = BTreeMap::new();
        let mut source_connectors = BTreeMap::new();
        let strict = context
            .strict_projection
            .as_ref()
            .context("strict identity required")?;
        // Hold both native registry read locks while projecting source references.
        let source_rows = self.enterprise.source_bindings.read().await;
        let connector_rows = self.enterprise.connectors.read().await;
        for source in source_rows.values() {
            if source.tenant_context.org_id != scope.org_id
                || source.tenant_context.workspace_id != scope.workspace_id
                || source.tenant_context.deployment_id.as_deref()
                    != Some(scope.deployment_id.as_str())
                || !source.state.allows_ingestion()
                || !source.ingestion_policy.allow_prompt_context
            {
                continue;
            }
            let matching_connectors: Vec<_> = connector_rows
                .values()
                .filter(|row| row.connector_id == source.connector_id && row.tenant_matches(tenant))
                .collect();
            ensure!(
                matching_connectors.len() <= 1,
                "ambiguous source connector ID"
            );
            let Some(connector) = matching_connectors
                .first()
                .filter(|row| row.state.allows_ingestion())
            else {
                continue;
            };
            if strict
                .evaluate_access(
                    &source.resource_ref,
                    AccessPermission::Read,
                    source.data_class,
                    crate::now_ms(),
                )
                .decision
                != AccessDecision::Allow
            {
                continue;
            }
            ensure!(
                !sources.contains_key(&source.binding_id),
                "ambiguous source binding ID"
            );
            references.insert(format!("data-ref:{}", source.binding_id));
            if let Some(project) = &source.resource_ref.project_id {
                projects.insert(project.clone());
            }
            sources.insert(source.binding_id.clone(), source.clone());
            source_connectors.insert(connector.connector_id.clone(), (**connector).clone());
        }
        drop(connector_rows);
        drop(source_rows);
        self.enterprise
            .hosted_policy
            .authorize_permission(Some(&context), AccessPermission::HostedAdmin)
            .map_err(anyhow::Error::msg)?;
        // Include complete source records and concrete route metadata so an ID
        // that is rebound to another destination cannot reuse a reviewed plan.
        // All private details stay host-side; only this digest enters the lock.
        let digest = sha256(&canonical_json(&serde_json::json!({
            "schema_version": 1, "settings": settings, "models": models, "routes": model_routes,
            "sources": sources, "connectors": source_connectors, "units": units,
            "hosted_revision": view.hosted_policy_revision,
        }))?);
        Ok(HostFacts {
            subjects: BTreeSet::from([context.human_actor.actor_id.clone()]),
            context,
            digest,
            policy: settings.policy,
            references,
            connectors: BTreeMap::new(),
            units,
            projects,
            models,
            // These are existing native facilities, not customer-declared flags.
            readiness: BTreeSet::from(["governed-memory".into(), "verified-user-context".into()]),
        })
    }
}
