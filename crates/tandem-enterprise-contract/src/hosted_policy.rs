//! Hosted control-plane policy input. Transport authentication belongs to the
//! fetcher; this contract validates a complete snapshot before publication.
use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::VerifiedTenantContext;

mod projection;

/// Reserved ownership namespace for control-plane-managed unit identities.
pub const HOSTED_TAXONOMY_ID: &str = "hosted-control-plane";

pub fn hosted_unit_principal(unit_id: &str) -> crate::PrincipalRef {
    crate::PrincipalRef::organization_unit(format!("{HOSTED_TAXONOMY_ID}/{unit_id}"))
}

pub const MAX_POLICY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_POLICY_AGE_MS: u64 = 120_000;
const FUTURE_SKEW_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedPolicyBundle {
    pub schema_version: u32,
    pub policy_version: u64,
    pub organization_id: String,
    pub deployment_id: String,
    pub generated_at: DateTime<Utc>,
    pub users: Vec<HostedPolicyUser>,
    pub org_units: Vec<HostedPolicyUnit>,
    pub org_unit_memberships: Vec<HostedPolicyMembership>,
    pub deployment_grants: Vec<HostedPolicyGrant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedPolicyUser {
    pub id: String,
    pub email: Option<String>,
    pub username: Option<String>,
    pub role: String,
    pub capabilities: Vec<String>,
    pub is_active: bool,
    pub email_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedPolicyUnit {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub kind: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedPolicyMembership {
    pub unit_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedPolicyGrant {
    pub id: String,
    pub deployment_id: Option<String>,
    pub principal_kind: String,
    pub principal_id: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedPolicyRevision {
    pub version: u64,
    pub digest: String,
}

#[derive(Debug, Clone)]
pub struct ValidatedHostedPolicy {
    bundle: HostedPolicyBundle,
    revision: HostedPolicyRevision,
    expires_at_ms: u64,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 160 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Result<BTreeSet<&'a str>, &'static str> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !valid_id(id) || !seen.insert(id) {
            return Err("invalid_or_duplicate_policy_id");
        }
    }
    Ok(seen)
}

impl HostedPolicyBundle {
    pub fn from_json(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_POLICY_BYTES {
            return Err("hosted_policy_too_large");
        }
        serde_json::from_slice(bytes).map_err(|_| "invalid_hosted_policy_json")
    }

    pub fn validate(
        mut self,
        organization_id: &str,
        deployment_id: &str,
        now_ms: u64,
        previous: Option<&HostedPolicyRevision>,
    ) -> Result<ValidatedHostedPolicy, &'static str> {
        if self.schema_version != 1 || self.policy_version == 0 {
            return Err("unsupported_hosted_policy_version");
        }
        if self.organization_id != organization_id
            || self.deployment_id != deployment_id
            || !valid_id(organization_id)
            || !valid_id(deployment_id)
        {
            return Err("hosted_policy_scope_mismatch");
        }
        let generated = u64::try_from(self.generated_at.timestamp_millis())
            .map_err(|_| "invalid_hosted_policy_time")?;
        let expires_at_ms = generated
            .checked_add(MAX_POLICY_AGE_MS)
            .ok_or("invalid_hosted_policy_time")?;
        if generated > now_ms.saturating_add(FUTURE_SKEW_MS) || expires_at_ms <= now_ms {
            return Err("hosted_policy_not_fresh");
        }
        let users = unique_ids(self.users.iter().map(|row| row.id.as_str()))?;
        let units = unique_ids(self.org_units.iter().map(|row| row.id.as_str()))?;
        unique_ids(self.deployment_grants.iter().map(|row| row.id.as_str()))?;
        for user in &self.users {
            if !matches!(user.role.as_str(), "owner" | "admin" | "member" | "viewer")
                || user
                    .capabilities
                    .iter()
                    .any(|cap| !role_capabilities(&user.role).contains(&cap.as_str()))
            {
                return Err("invalid_hosted_user_authority");
            }
        }
        for unit in &self.org_units {
            if !matches!(unit.state.as_str(), "active" | "archived")
                || !matches!(
                    unit.kind.as_str(),
                    "group" | "department" | "team" | "custom"
                )
            {
                return Err("invalid_hosted_unit");
            }
        }
        let mut memberships = BTreeSet::new();
        for row in &self.org_unit_memberships {
            if !users.contains(row.user_id.as_str())
                || !units.contains(row.unit_id.as_str())
                || !memberships.insert((&row.unit_id, &row.user_id))
            {
                return Err("invalid_hosted_membership");
            }
        }
        for grant in &self.deployment_grants {
            let principal_exists = match grant.principal_kind.as_str() {
                "member" => users.contains(grant.principal_id.as_str()),
                "org_unit" => units.contains(grant.principal_id.as_str()),
                _ => false,
            };
            let expected_resource = grant.deployment_id.as_deref().unwrap_or("*");
            if !principal_exists
                || grant.resource_kind != "deployment"
                || grant.resource_id != expected_resource
                || grant
                    .deployment_id
                    .as_deref()
                    .is_some_and(|id| id != deployment_id)
                || grant.permissions.is_empty()
                || grant.permissions.iter().any(|p| {
                    !matches!(
                        p.as_str(),
                        "hosted.view"
                            | "hosted.use"
                            | "hosted.admin"
                            | "automation.read"
                            | "automation.execute"
                            | "automation.write"
                            | "automation.share"
                            | "workflow.read"
                            | "workflow.share"
                    )
                })
            {
                return Err("invalid_hosted_grant");
            }
        }
        // Authority tables have no API ordering guarantee. Ignore fetch time in
        // the semantic digest while preserving every authority-bearing field.
        self.users.sort_by(|a, b| a.id.cmp(&b.id));
        self.org_units.sort_by(|a, b| a.id.cmp(&b.id));
        self.org_unit_memberships
            .sort_by(|a, b| (&a.unit_id, &a.user_id).cmp(&(&b.unit_id, &b.user_id)));
        self.deployment_grants.sort_by(|a, b| a.id.cmp(&b.id));
        for user in &mut self.users {
            user.capabilities.sort();
            user.capabilities.dedup();
        }
        for grant in &mut self.deployment_grants {
            grant.permissions.sort();
            grant.permissions.dedup();
        }
        let mut semantic = self.clone();
        for user in &mut semantic.users {
            user.email = None;
            user.username = None;
        }
        semantic.generated_at =
            DateTime::from_timestamp_millis(0).ok_or("invalid_hosted_policy_time")?;
        let bytes = serde_json::to_vec(&semantic).map_err(|_| "invalid_hosted_policy_json")?;
        let revision = HostedPolicyRevision {
            version: self.policy_version,
            digest: format!("{:x}", Sha256::digest(bytes)),
        };
        if let Some(previous) = previous {
            if revision.version < previous.version {
                return Err("hosted_policy_rollback");
            }
            if revision.version == previous.version && revision.digest != previous.digest {
                return Err("hosted_policy_revision_conflict");
            }
        }
        Ok(ValidatedHostedPolicy {
            bundle: self,
            revision,
            expires_at_ms,
        })
    }
}

pub fn role_capabilities(role: &str) -> Vec<&'static str> {
    let mut capabilities = vec!["hosted.panel", "hosted.view"];
    if matches!(role, "member" | "admin" | "owner") {
        capabilities.extend([
            "hosted.use",
            "automation.read",
            "automation.execute",
            "workflow.read",
        ]);
    }
    if matches!(role, "admin" | "owner") {
        capabilities.extend([
            "hosted.admin",
            "org.units.manage",
            "org.invites.manage",
            "automation.share",
            "automation.write",
            "workflow.share",
        ]);
    }
    if role == "owner" {
        capabilities.push("hosted.owner");
    }
    capabilities
}

impl ValidatedHostedPolicy {
    pub fn revision(&self) -> &HostedPolicyRevision {
        &self.revision
    }
    pub fn bundle(&self) -> &HostedPolicyBundle {
        &self.bundle
    }
    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }

    pub fn authorize_identity(
        &self,
        verified: &VerifiedTenantContext,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        if now_ms >= self.expires_at_ms || verified.is_expired_at(now_ms) {
            return Err("hosted_policy_or_identity_expired");
        }
        let tenant = &verified.tenant_context;
        if tenant.org_id != self.bundle.organization_id
            || tenant.workspace_id != self.bundle.deployment_id
            || tenant.deployment_id.as_deref() != Some(self.bundle.deployment_id.as_str())
            || tenant.actor_id.as_deref() != Some(verified.human_actor.actor_id.as_str())
        {
            return Err("hosted_identity_scope_mismatch");
        }
        if verified.policy_version != Some(self.revision.version) {
            return Err("hosted_identity_policy_revision_changed");
        }
        let user = self
            .bundle
            .users
            .iter()
            .find(|row| row.id == verified.human_actor.actor_id)
            .filter(|row| row.is_active && row.email_verified)
            .ok_or("hosted_membership_revoked")?;
        let mut roles = vec![
            "hosted:panel".to_string(),
            "hosted:view".to_string(),
            format!("hosted:role:{}", user.role),
        ];
        if matches!(user.role.as_str(), "member" | "admin" | "owner") {
            roles.push("hosted:use".into());
        }
        if matches!(user.role.as_str(), "admin" | "owner") {
            roles.push("hosted:admin".into());
        }
        if user.role == "owner" {
            roles.push("hosted:owner".into());
        }
        if verified.roles.iter().any(|role| !roles.contains(role))
            || verified
                .capabilities
                .iter()
                .any(|cap| !user.capabilities.contains(cap))
            || verified.org_units.iter().any(|unit| {
                !self.bundle.org_unit_memberships.iter().any(|row| {
                    row.user_id == user.id
                        && row.unit_id == *unit
                        && self
                            .bundle
                            .org_units
                            .iter()
                            .any(|u| u.id == *unit && u.state == "active")
                })
            })
        {
            return Err("hosted_identity_authority_changed");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
