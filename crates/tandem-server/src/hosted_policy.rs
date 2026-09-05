//! Current hosted authority is one immutable snapshot. The host policy agent
//! owns the input file; the engine never receives its control-plane credential.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::Context;
use tandem_enterprise_contract::hosted_policy::{
    HostedPolicyBundle, HostedPolicyRevision, ValidatedHostedPolicy, MAX_POLICY_BYTES,
};
use tandem_enterprise_contract::{
    AccessDecision, AccessPermission, DataClass, OrganizationUnitMembership,
};
use tandem_types::{RuntimeAuthMode, TenantContext, VerifiedTenantContext};

use crate::governance_store::{for_state, GovernanceStoreFile};
use crate::AppState;

#[cfg(test)]
#[path = "hosted_policy_tests.rs"]
mod tests;

#[derive(Clone)]
struct PolicySource {
    organization_id: String,
    deployment_id: String,
    path: PathBuf,
    started_at_ms: u64,
}

#[derive(Default)]
pub(crate) struct HostedPolicyRuntime {
    source: RwLock<Option<PolicySource>>,
    snapshot: RwLock<Option<Arc<ValidatedHostedPolicy>>>,
    update: tokio::sync::Mutex<()>,
}

impl HostedPolicyRuntime {
    #[cfg(test)]
    pub(crate) fn configure_test_source(
        &self,
        organization_id: &str,
        deployment_id: &str,
        path: PathBuf,
    ) {
        *self.source.write().unwrap() = Some(PolicySource {
            organization_id: organization_id.into(),
            deployment_id: deployment_id.into(),
            path,
            started_at_ms: 0,
        });
    }

    fn current(&self) -> Result<Option<Arc<ValidatedHostedPolicy>>, &'static str> {
        if self
            .source
            .read()
            .map_err(|_| "hosted_policy_lock_failed")?
            .is_none()
        {
            return Ok(None);
        }
        self.snapshot
            .read()
            .map_err(|_| "hosted_policy_lock_failed")?
            .clone()
            .map(Some)
            .ok_or("hosted_policy_not_synchronized")
    }

    pub(crate) fn project(
        &self,
        verified: &mut VerifiedTenantContext,
    ) -> Result<Option<Vec<OrganizationUnitMembership>>, &'static str> {
        let Some(policy) = self.current()? else {
            return Ok(None);
        };
        let now = crate::now_ms();
        let memberships = policy.memberships_for_identity(verified, now)?;
        verified.strict_projection = Some(policy.project_identity(verified, now)?);
        Ok(Some(memberships))
    }

    pub(crate) fn authorize_execution(
        &self,
        verified: Option<&VerifiedTenantContext>,
    ) -> Result<(), &'static str> {
        let Some(policy) = self.current()? else {
            return Ok(());
        };
        let now = crate::now_ms();
        let projection =
            policy.project_identity(verified.ok_or("hosted_policy_identity_required")?, now)?;
        if projection
            .evaluate_access(
                &policy.deployment_resource(),
                AccessPermission::HostedUse,
                DataClass::Internal,
                now,
            )
            .decision
            != AccessDecision::Allow
        {
            return Err("hosted_use_required");
        }
        Ok(())
    }

    pub(crate) fn is_ready(&self) -> bool {
        let Ok(source) = self.source.read() else {
            return false;
        };
        if source.is_none() {
            return true;
        }
        self.snapshot.read().is_ok_and(|snapshot| {
            snapshot
                .as_ref()
                .is_some_and(|policy| policy.expires_at_ms() > crate::now_ms())
        })
    }

    pub(crate) fn authorize(
        &self,
        verified: Option<&VerifiedTenantContext>,
    ) -> Result<(), &'static str> {
        let source = self
            .source
            .read()
            .map_err(|_| "hosted_policy_lock_failed")?;
        if source.is_none() {
            return Ok(());
        }
        drop(source);
        let snapshot = self
            .snapshot
            .read()
            .map_err(|_| "hosted_policy_lock_failed")?
            .clone()
            .ok_or("hosted_policy_not_synchronized")?;
        snapshot.authorize_identity(
            verified.ok_or("hosted_policy_identity_required")?,
            crate::now_ms(),
        )
    }

    pub(crate) fn revision(&self) -> Result<Option<HostedPolicyRevision>, &'static str> {
        Ok(self
            .snapshot
            .read()
            .map_err(|_| "hosted_policy_lock_failed")?
            .as_ref()
            .map(|policy| policy.revision().clone()))
    }
}

impl AppState {
    pub(crate) fn start_hosted_policy_sync(&self, mode: RuntimeAuthMode) -> anyhow::Result<()> {
        let input_path = std::env::var("TANDEM_HOSTED_POLICY_FILE").ok();
        if mode != RuntimeAuthMode::HostedSingleTenant
            && input_path.is_none()
            && !crate::config::env::hosted_control_plane_configured()
        {
            return Ok(());
        }
        let required = |name: &str| -> anyhow::Result<String> {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("hosted policy requires {name}"))
        };
        let source = PolicySource {
            organization_id: required("TANDEM_HOSTED_ORGANIZATION_ID")?,
            deployment_id: required("TANDEM_HOSTED_DEPLOYMENT_ID")?,
            path: PathBuf::from(required("TANDEM_HOSTED_POLICY_FILE")?),
            started_at_ms: crate::now_ms(),
        };
        anyhow::ensure!(
            source.path.is_absolute(),
            "hosted policy input path must be absolute"
        );
        let mut configured = self
            .enterprise
            .hosted_policy
            .source
            .write()
            .map_err(|_| anyhow::anyhow!("hosted policy source lock failed"))?;
        anyhow::ensure!(
            configured.is_none(),
            "hosted policy sync is already configured"
        );
        *configured = Some(source);
        drop(configured);
        let state = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = state.reload_hosted_policy().await {
                    tracing::warn!(target: "tandem_server::hosted_policy", %error,
                        "hosted policy synchronization failed; authority remains bounded by snapshot expiry");
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
        Ok(())
    }

    pub(crate) async fn reload_hosted_policy(&self) -> anyhow::Result<()> {
        let runtime = &self.enterprise.hosted_policy;
        let _update = runtime.update.lock().await;
        let source = runtime
            .source
            .read()
            .map_err(|_| anyhow::anyhow!("hosted policy source lock failed"))?
            .clone()
            .ok_or_else(|| anyhow::anyhow!("hosted policy source is not configured"))?;
        let bytes = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let file = crate::context_assertion_security::open_keyring_file(&source.path, true)
                .map_err(anyhow::Error::msg)?;
            let mut bytes = Vec::new();
            file.take((MAX_POLICY_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            Ok((source, bytes))
        })
        .await??;
        let (source, bytes) = bytes;
        let bundle = HostedPolicyBundle::from_json(&bytes).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            bundle.generated_at.timestamp_millis() >= source.started_at_ms as i64,
            "hosted policy needs a fresh control-plane fetch after engine startup"
        );
        let store = for_state(self);
        let previous: BTreeMap<String, HostedPolicyRevision> = store
            .read_json(GovernanceStoreFile::HostedPolicyRevision)
            .await?
            .unwrap_or_default();
        let key = format!("{}/{}", source.organization_id, source.deployment_id);
        let policy = bundle
            .validate(
                &source.organization_id,
                &source.deployment_id,
                crate::now_ms(),
                previous.get(&key),
            )
            .map_err(anyhow::Error::msg)?;
        // Publish only after the existing encrypted, anchored store records the
        // accepted high-water mark. A restart never loads live authority here.
        if previous.get(&key) != Some(policy.revision()) {
            anyhow::ensure!(
                previous.is_empty() || previous.contains_key(&key),
                "hosted policy persisted scope mismatch"
            );
            let tenant = TenantContext::explicit_user_workspace(
                &source.organization_id,
                &source.deployment_id,
                Some(source.deployment_id.clone()),
                "hosted-policy-agent",
            );
            let record = GovernanceStoreFile::HostedPolicyRevision.json_record(
                key,
                policy.revision(),
                &tenant,
                None,
            )?;
            store
                .write_json_records(GovernanceStoreFile::HostedPolicyRevision, &[record])
                .await
                .context("persist hosted policy high-water mark")?;
        }
        *runtime
            .snapshot
            .write()
            .map_err(|_| anyhow::anyhow!("hosted policy snapshot lock failed"))? =
            Some(Arc::new(policy));
        Ok(())
    }
}
