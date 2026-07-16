// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tandem_channels::channel_registry::{command_tier_for_profile, CommandTier};
use tandem_channels::config::ChannelSecurityProfile;
use tandem_types::EngineEvent;
use uuid::Uuid;

use crate::app::state::AppState;

const DEFAULT_ENROLLMENT_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelUserCapabilityRecord {
    pub channel: String,
    pub user_id: String,
    pub max_tier: StoredCommandTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrolled_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrolled_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_workspace_id: Option<String>,
    /// TAN-765: org units (departments) this enrollment established
    /// memberships in, recorded for audit/display. The authoritative
    /// membership rows live in the enterprise org-unit store.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub org_units: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StoredCommandTier {
    Read,
    Act,
    Approve,
    Reconfigure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelEnrollmentCodeRecord {
    pub code: String,
    pub channel: String,
    pub user_id: String,
    pub max_tier: StoredCommandTier,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_workspace_id: Option<String>,
    /// TAN-765: org units (departments) to bind on redemption. Entries match
    /// a unit's bare id (`engineering`) or principal id
    /// (`department/engineering`); redeeming establishes active org-unit
    /// memberships for the enrolled identity, not just an approval tier.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub org_units: Vec<String>,
    /// Tenant the `org_units` refs resolve in. Set together; when present,
    /// redemption only matches units bound to this tenant, so a shared unit
    /// name in another tenant can never satisfy this code's binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_org_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_workspace_id: Option<String>,
}

impl From<CommandTier> for StoredCommandTier {
    fn from(value: CommandTier) -> Self {
        match value {
            CommandTier::Read => Self::Read,
            CommandTier::Act => Self::Act,
            CommandTier::Approve => Self::Approve,
            CommandTier::Reconfigure => Self::Reconfigure,
        }
    }
}

impl From<StoredCommandTier> for CommandTier {
    fn from(value: StoredCommandTier) -> Self {
        match value {
            StoredCommandTier::Read => Self::Read,
            StoredCommandTier::Act => Self::Act,
            StoredCommandTier::Approve => Self::Approve,
            StoredCommandTier::Reconfigure => Self::Reconfigure,
        }
    }
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub async fn issue_channel_enrollment_code(
        &self,
        channel: impl Into<String>,
        user_id: impl Into<String>,
        max_tier: StoredCommandTier,
        ttl_ms: Option<u64>,
        issued_by: Option<String>,
        pinned_workspace_id: Option<String>,
        org_units: Vec<String>,
        tenant: Option<(String, String)>,
    ) -> anyhow::Result<ChannelEnrollmentCodeRecord> {
        let org_units = normalize_org_unit_refs(org_units);
        // Fail fast on typos: every referenced unit must exist at issue time
        // — within the enrollment's tenant when one is given, and rejecting
        // cross-tenant-ambiguous refs otherwise — so the operator learns
        // immediately rather than the redeeming user hitting a dead code.
        self.resolve_org_unit_refs(
            &org_units,
            tenant
                .as_ref()
                .map(|(org_id, workspace_id)| (org_id.as_str(), workspace_id.as_str())),
        )
        .await?;
        let issued_at_ms = crate::now_ms();
        let expires_at_ms =
            issued_at_ms.saturating_add(ttl_ms.unwrap_or(DEFAULT_ENROLLMENT_TTL_MS));
        let code = Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
            .to_ascii_uppercase();
        let record = ChannelEnrollmentCodeRecord {
            code: code.clone(),
            channel: channel.into(),
            user_id: user_id.into(),
            max_tier,
            issued_at_ms,
            expires_at_ms,
            issued_by,
            pinned_workspace_id,
            org_units,
            tenant_org_id: tenant.as_ref().map(|(org_id, _)| org_id.clone()),
            tenant_workspace_id: tenant.map(|(_, workspace_id)| workspace_id),
        };
        self.channel_enrollment_codes
            .write()
            .await
            .insert(code, record.clone());
        Ok(record)
    }

    pub async fn confirm_channel_enrollment_code(
        &self,
        code: &str,
        enrolled_by: Option<String>,
    ) -> anyhow::Result<ChannelUserCapabilityRecord> {
        let key = normalize_enrollment_code(code);
        let pending = {
            let mut guard = self.channel_enrollment_codes.write().await;
            guard.remove(&key)
        }
        .ok_or_else(|| anyhow::anyhow!("pairing code not found"))?;

        if pending.expires_at_ms < crate::now_ms() {
            return Err(anyhow::anyhow!("pairing code expired"));
        }

        // TAN-765: establish the department memberships BEFORE the capability
        // grant so a failure here leaves no partial authority. The code is
        // already consumed either way — a failed binding needs a fresh code.
        if !pending.org_units.is_empty() {
            let tenant = match (&pending.tenant_org_id, &pending.tenant_workspace_id) {
                (Some(org_id), Some(workspace_id)) => {
                    Some((org_id.as_str(), workspace_id.as_str()))
                }
                _ => None,
            };
            self.grant_org_unit_memberships_for_principal(
                &pending.user_id,
                &pending.org_units,
                tenant,
            )
            .await?;
        }

        let record = ChannelUserCapabilityRecord {
            channel: pending.channel,
            user_id: pending.user_id,
            max_tier: pending.max_tier,
            enrolled_at_ms: Some(crate::now_ms()),
            enrolled_by: enrolled_by.or(pending.issued_by),
            pinned_workspace_id: pending.pinned_workspace_id,
            org_units: pending.org_units,
        };
        self.upsert_channel_user_capability(record.clone()).await?;
        Ok(record)
    }

    /// Resolve org-unit references (bare unit id or `taxonomy/unit_id`
    /// principal id) against the enterprise org-unit store. Errors on any
    /// unknown reference. When `tenant` is given, only units bound to that
    /// `(org_id, workspace_id)` are considered; without it, a reference that
    /// matches units in more than one tenant is REJECTED rather than resolved
    /// to an arbitrary one — enrolling a sender must never grant authority in
    /// a different tenant because two tenants share a unit name.
    pub async fn resolve_org_unit_refs(
        &self,
        refs: &[String],
        tenant: Option<(&str, &str)>,
    ) -> anyhow::Result<Vec<tandem_enterprise_contract::OrganizationUnit>> {
        let units = self.enterprise.org_units.read().await;
        refs.iter()
            .map(|entry| {
                let wanted = entry.trim();
                let matches = units
                    .values()
                    .filter(|unit| unit.unit_id == wanted || unit.principal_ref().id == wanted)
                    .filter(|unit| {
                        tenant.is_none_or(|(org_id, workspace_id)| {
                            unit.tenant_context.org_id == org_id
                                && unit.tenant_context.workspace_id == workspace_id
                        })
                    })
                    .collect::<Vec<_>>();
                let mut tenants = matches
                    .iter()
                    .map(|unit| {
                        (
                            unit.tenant_context.org_id.as_str(),
                            unit.tenant_context.workspace_id.as_str(),
                        )
                    })
                    .collect::<Vec<_>>();
                tenants.sort();
                tenants.dedup();
                match tenants.len() {
                    0 => Err(anyhow::anyhow!("unknown org unit: {wanted}")),
                    1 => Ok(matches[0].clone()),
                    _ => Err(anyhow::anyhow!(
                        "org unit ref {wanted} is ambiguous across tenants; scope the enrollment to a tenant"
                    )),
                }
            })
            .collect()
    }

    /// TAN-765: establish active org-unit memberships for a principal (e.g. a
    /// Slack identity `channel:slack:{team}:{app}:{user}`). Existing active
    /// memberships are kept as-is; new rows persist through the governance
    /// store. Returns the membership ids now in effect for the references.
    pub async fn grant_org_unit_memberships_for_principal(
        &self,
        member_id: &str,
        unit_refs: &[String],
        tenant: Option<(&str, &str)>,
    ) -> anyhow::Result<Vec<String>> {
        use tandem_types::{OrganizationUnitMembership, OrganizationUnitMembershipSource};

        let member_id = member_id.trim();
        anyhow::ensure!(!member_id.is_empty(), "member id must not be empty");
        let units = self.resolve_org_unit_refs(unit_refs, tenant).await?;
        let member = tandem_types::PrincipalRef::human_user(member_id);
        let now_ms = crate::now_ms();

        let mut effective_ids = Vec::new();
        {
            let mut registry = self.enterprise.org_unit_memberships.write().await;
            for unit in units {
                let unit_principal = unit.principal_ref();
                if let Some(existing) = registry.values().find(|membership| {
                    membership.state.is_active()
                        && membership.unit.id == unit_principal.id
                        && membership.member.id == member.id
                        && membership.tenant_context.org_id == unit.tenant_context.org_id
                        && membership.tenant_context.workspace_id
                            == unit.tenant_context.workspace_id
                }) {
                    effective_ids.push(existing.membership_id.clone());
                    continue;
                }
                let membership_id = format!(
                    "enrollment-{}-{}",
                    unit.unit_id,
                    Uuid::new_v4()
                        .simple()
                        .to_string()
                        .chars()
                        .take(8)
                        .collect::<String>()
                );
                let membership = OrganizationUnitMembership::active(
                    membership_id.clone(),
                    unit.tenant_context.clone(),
                    unit_principal,
                    member.clone(),
                    OrganizationUnitMembershipSource::Direct,
                    now_ms,
                );
                registry.insert(membership.membership_id.clone(), membership);
                effective_ids.push(membership_id);
            }
            self.persist_enterprise_org_unit_memberships_locked(&registry)
                .await?;
        }
        Ok(effective_ids)
    }

    /// Persist the org-unit membership registry through the governance store
    /// (encryption-aware), mirroring how the store is loaded at startup.
    async fn persist_enterprise_org_unit_memberships_locked(
        &self,
        registry: &HashMap<String, tandem_enterprise_contract::OrganizationUnitMembership>,
    ) -> anyhow::Result<()> {
        let records = registry
            .iter()
            .map(|(key, row)| {
                crate::governance_store::GovernanceStoreFile::OrgUnitMemberships.json_record(
                    key,
                    row,
                    &row.tenant_context,
                    Some(&row.unit.id),
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        crate::governance_store::for_state(self)
            .write_json_records(
                crate::governance_store::GovernanceStoreFile::OrgUnitMemberships,
                &records,
            )
            .await
    }

    pub async fn load_channel_user_capabilities(&self) -> anyhow::Result<()> {
        if !self.channel_user_capabilities_path.exists() {
            return Ok(());
        }
        let raw = tokio::fs::read_to_string(&self.channel_user_capabilities_path).await?;
        let parsed = serde_json::from_str::<HashMap<String, ChannelUserCapabilityRecord>>(&raw)
            .unwrap_or_default();
        *self.channel_user_capabilities.write().await = parsed;
        Ok(())
    }

    pub async fn persist_channel_user_capabilities(&self) -> anyhow::Result<()> {
        let payload = {
            let guard = self.channel_user_capabilities.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.channel_user_capabilities_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&self.channel_user_capabilities_path, payload).await?;
        Ok(())
    }

    pub async fn upsert_channel_user_capability(
        &self,
        record: ChannelUserCapabilityRecord,
    ) -> anyhow::Result<()> {
        let key = channel_user_capability_key(&record.channel, &record.user_id);
        self.channel_user_capabilities
            .write()
            .await
            .insert(key, record.clone());
        if let Some(runtime) = self.runtime.get() {
            runtime.event_bus.publish(EngineEvent::new(
                "channel.capability.changed",
                json!({
                    "channel": record.channel,
                    "user_id": record.user_id,
                    "max_tier": record.max_tier,
                    "actor_id": record.enrolled_by,
                    "executed_as": "channel_enrollment",
                    "workspace": record.pinned_workspace_id,
                }),
            ));
        }
        self.persist_channel_user_capabilities().await
    }

    pub async fn channel_user_capability_tier(
        &self,
        channel: &str,
        user_id: &str,
        fallback_profile: ChannelSecurityProfile,
    ) -> CommandTier {
        let key = channel_user_capability_key(channel, user_id);
        self.channel_user_capabilities
            .read()
            .await
            .get(&key)
            .map(|record| CommandTier::from(record.max_tier))
            .unwrap_or_else(|| command_tier_for_profile(fallback_profile))
    }

    pub async fn channel_user_can_approve(
        &self,
        channel: &str,
        user_id: &str,
        fallback_profile: ChannelSecurityProfile,
        is_open_channel: bool,
    ) -> bool {
        // An explicit per-identity capability grant is authoritative — including a
        // deliberate downgrade below `Approve`.
        let key = channel_user_capability_key(channel, user_id);
        if let Some(record) = self.channel_user_capabilities.read().await.get(&key) {
            return CommandTier::from(record.max_tier) >= CommandTier::Approve;
        }
        // GOV-B5a: with no explicit grant, fall back to the channel security profile
        // ONLY on a non-open channel, where the hand-picked `allowed_users` list is a
        // deliberate identity-trust decision by the operator. On a wildcard-open (`*`)
        // channel, "allowed to talk" must not imply "allowed to approve" — approval
        // there requires an explicit per-identity grant. This closes the
        // approve-by-default hole without disturbing solo/hand-picked-allowlist setups.
        if is_open_channel {
            return false;
        }
        command_tier_for_profile(fallback_profile) >= CommandTier::Approve
    }

    /// GOV-B5b: issue a per-identity step-up valid for `ttl_ms`, returning the
    /// expiry timestamp. Replaces any prior grant for the same channel+user.
    pub async fn grant_channel_step_up(&self, channel: &str, user_id: &str, ttl_ms: u64) -> u64 {
        let expires_at_ms = crate::now_ms().saturating_add(ttl_ms);
        self.channel_step_up_grants
            .write()
            .await
            .insert(channel_user_capability_key(channel, user_id), expires_at_ms);
        expires_at_ms
    }

    /// GOV-B5b: true if the identity currently holds an unexpired step-up grant.
    /// Expired grants are pruned on read. This is the per-user replacement for the
    /// legacy global `TANDEM_CHANNEL_STEP_UP_PIN`, and (unlike the slash-only PIN)
    /// is checkable on the button/interaction path.
    pub async fn channel_step_up_active(&self, channel: &str, user_id: &str) -> bool {
        let key = channel_user_capability_key(channel, user_id);
        let mut guard = self.channel_step_up_grants.write().await;
        match guard.get(&key).copied() {
            Some(expires_at_ms) if expires_at_ms > crate::now_ms() => true,
            Some(_) => {
                guard.remove(&key);
                false
            }
            None => false,
        }
    }
}

pub fn channel_security_profile_from_config(
    effective_config: &serde_json::Value,
    channel: &str,
) -> ChannelSecurityProfile {
    effective_config
        .pointer(&format!("/channels/{channel}/security_profile"))
        .cloned()
        .and_then(|value| serde_json::from_value::<ChannelSecurityProfile>(value).ok())
        .unwrap_or_default()
}

pub fn channel_user_capability_key(channel: &str, user_id: &str) -> String {
    format!(
        "{}:{}",
        channel.trim().to_ascii_lowercase(),
        user_id.trim().to_ascii_lowercase()
    )
}

/// GOV-B5b: whether a channel requires an active per-identity step-up before an
/// approval/interaction is honored. Opt-in per channel via
/// `/channels/{channel}/require_approval_step_up` (default `false`), so existing
/// flows are unchanged unless an operator deliberately raises the bar.
pub fn channel_requires_approval_step_up(
    effective_config: &serde_json::Value,
    channel: &str,
) -> bool {
    effective_config
        .pointer(&format!("/channels/{channel}/require_approval_step_up"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn normalize_enrollment_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

fn normalize_org_unit_refs(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for entry in raw {
        let normalized = entry.trim().to_string();
        if normalized.is_empty() || out.contains(&normalized) {
            continue;
        }
        out.push(normalized);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn persists_and_loads_channel_user_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new_starting("test".to_string(), true);
        state.channel_user_capabilities_path = dir.path().join("channel_user_capabilities.json");
        state
            .upsert_channel_user_capability(ChannelUserCapabilityRecord {
                channel: "slack".to_string(),
                user_id: "U123".to_string(),
                max_tier: StoredCommandTier::Approve,
                enrolled_at_ms: Some(7),
                enrolled_by: Some("admin".to_string()),
                pinned_workspace_id: None,
                org_units: Vec::new(),
            })
            .await
            .unwrap();

        let mut loaded = AppState::new_starting("test".to_string(), true);
        loaded.channel_user_capabilities_path = state.channel_user_capabilities_path.clone();
        loaded.load_channel_user_capabilities().await.unwrap();
        assert_eq!(
            loaded
                .channel_user_capability_tier("slack", "U123", ChannelSecurityProfile::PublicDemo)
                .await,
            CommandTier::Approve
        );
    }

    #[tokio::test]
    async fn missing_user_falls_back_to_channel_profile_tier() {
        let state = AppState::new_starting("test".to_string(), true);
        assert_eq!(
            state
                .channel_user_capability_tier(
                    "telegram",
                    "alice",
                    ChannelSecurityProfile::PublicDemo
                )
                .await,
            CommandTier::Read
        );
    }

    #[tokio::test]
    async fn enrollment_code_binds_fake_telegram_user_to_approve_tier() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new_starting("test".to_string(), true);
        state.channel_user_capabilities_path = dir.path().join("channel_user_capabilities.json");

        let issued = state
            .issue_channel_enrollment_code(
                "telegram",
                "fake-telegram-user",
                StoredCommandTier::Approve,
                None,
                Some("operator".to_string()),
                Some("/workspace/acme".to_string()),
                Vec::new(),
                None,
            )
            .await
            .expect("issue enrollment code");
        let record = state
            .confirm_channel_enrollment_code(&issued.code.to_ascii_lowercase(), None)
            .await
            .unwrap();

        assert_eq!(record.channel, "telegram");
        assert_eq!(record.user_id, "fake-telegram-user");
        assert_eq!(
            record.pinned_workspace_id.as_deref(),
            Some("/workspace/acme")
        );
        assert!(
            state
                .channel_user_can_approve(
                    "telegram",
                    "fake-telegram-user",
                    ChannelSecurityProfile::PublicDemo,
                    false,
                )
                .await
        );
    }

    #[tokio::test]
    async fn open_channel_denies_approval_without_explicit_grant() {
        // GOV-B5a: on a wildcard-open channel, the Operator profile must NOT confer
        // approval to an ungranted user — "allowed to talk" is not "allowed to approve".
        let state = AppState::new_starting("test".to_string(), true);
        assert!(
            !state
                .channel_user_can_approve("slack", "U-open", ChannelSecurityProfile::Operator, true)
                .await,
            "open channel must not auto-confer approval"
        );
        // On a hand-picked (non-open) channel, the deliberate Operator profile still
        // confers approval — preserving solo/trusted-allowlist setups.
        assert!(
            state
                .channel_user_can_approve(
                    "slack",
                    "U-open",
                    ChannelSecurityProfile::Operator,
                    false
                )
                .await,
            "non-open Operator channel preserves approval"
        );
    }

    #[tokio::test]
    async fn explicit_grant_approves_even_on_open_channel() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new_starting("test".to_string(), true);
        state.channel_user_capabilities_path = dir.path().join("caps.json");
        state
            .upsert_channel_user_capability(ChannelUserCapabilityRecord {
                channel: "slack".to_string(),
                user_id: "U-granted".to_string(),
                max_tier: StoredCommandTier::Approve,
                enrolled_at_ms: Some(1),
                enrolled_by: Some("admin".to_string()),
                pinned_workspace_id: None,
                org_units: Vec::new(),
            })
            .await
            .unwrap();
        // An explicit per-identity grant >= Approve wins even on an open channel.
        assert!(
            state
                .channel_user_can_approve(
                    "slack",
                    "U-granted",
                    ChannelSecurityProfile::PublicDemo,
                    true
                )
                .await
        );
        // A different, ungranted user on the same open channel still cannot approve.
        assert!(
            !state
                .channel_user_can_approve(
                    "slack",
                    "U-nogrant",
                    ChannelSecurityProfile::PublicDemo,
                    true
                )
                .await
        );
    }

    #[tokio::test]
    async fn step_up_grant_is_active_until_expiry() {
        // GOV-B5b: a per-identity step-up is active while unexpired and absent
        // otherwise (a zero-TTL grant is immediately expired).
        let state = AppState::new_starting("test".to_string(), true);
        assert!(!state.channel_step_up_active("slack", "U-step").await);
        state.grant_channel_step_up("slack", "U-step", 60_000).await;
        assert!(state.channel_step_up_active("slack", "U-step").await);
        state.grant_channel_step_up("slack", "U-step", 0).await;
        assert!(!state.channel_step_up_active("slack", "U-step").await);
    }

    #[test]
    fn require_approval_step_up_config_defaults_off() {
        // GOV-B5b: step-up is opt-in per channel; absent config means off.
        let cfg = serde_json::json!({
            "channels": { "slack": { "require_approval_step_up": true }, "discord": {} }
        });
        assert!(channel_requires_approval_step_up(&cfg, "slack"));
        assert!(!channel_requires_approval_step_up(&cfg, "discord"));
        assert!(!channel_requires_approval_step_up(&cfg, "telegram"));
    }
}
