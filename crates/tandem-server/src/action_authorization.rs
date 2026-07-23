// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Central authorization contract for effects that cross the engine/host boundary.
//!
//! HTTP handlers, background workers, and state managers must obtain an
//! AuthorizedHostEffect before invoking a host effect. The grant binds the
//! verified principal, tenant, capability, canonical resource, and an immutable
//! digest of the exact effect arguments. Private fields prevent callers from
//! manufacturing authority directly.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_types::{TenantContext, VerifiedTenantContext};

use crate::AppState;

const HOST_EFFECT_GRANT_TTL_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAction {
    FileSearch,
    FileRead,
    CommandExecute,
    PtyManage,
    GlobalDispose,
    StorageRepair,
    BrowserInstall,
    BrowserSmokeTest,
    WorktreeList,
    WorktreeCreate,
    WorktreeDelete,
    WorktreeReset,
    WorktreeCleanup,
    PackRead,
    PackInstall,
    PackUninstall,
    PackExport,
    PackDetect,
    ProjectConfigUpdate,
    ProviderCredentialUpdate,
    GlobalConfigUpdate,
    ApiTokenManage,
    ChannelRead,
    ChannelVerify,
    ChannelConfigUpdate,
    ChannelConfigDelete,
    ChannelReload,
    McpServerManage,
}

impl HostAction {
    pub fn capability(self) -> &'static str {
        match self {
            Self::FileSearch | Self::FileRead => "host.files.read",
            Self::CommandExecute => "host.command.execute",
            Self::PtyManage => "host.pty.manage",
            Self::GlobalDispose => "deployment.dispose",
            Self::StorageRepair => "deployment.storage.repair",
            Self::BrowserInstall => "deployment.browser.install",
            Self::BrowserSmokeTest => "deployment.browser.smoke_test",
            Self::WorktreeList => "host.worktree.read",
            Self::WorktreeCreate => "host.worktree.create",
            Self::WorktreeDelete => "host.worktree.delete",
            Self::WorktreeReset => "host.worktree.reset",
            Self::WorktreeCleanup => "host.worktree.cleanup",
            Self::PackRead => "packs.read",
            Self::PackInstall => "packs.install",
            Self::PackUninstall => "packs.uninstall",
            Self::PackExport => "packs.export",
            Self::PackDetect => "packs.detect",
            Self::ProjectConfigUpdate => "providers.config.manage",
            Self::ProviderCredentialUpdate => "providers.credentials.manage",
            Self::GlobalConfigUpdate => "deployment.config.manage",
            Self::ApiTokenManage => "deployment.api_token.manage",
            Self::ChannelRead => "deployment.channels.read",
            Self::ChannelVerify => "deployment.channels.verify",
            Self::ChannelConfigUpdate => "deployment.channels.manage",
            Self::ChannelConfigDelete => "deployment.channels.manage",
            Self::ChannelReload => "deployment.channels.reload",
            Self::McpServerManage => "deployment.mcp.manage",
        }
    }

    pub fn requires_deployment_admin(self) -> bool {
        matches!(
            self,
            Self::GlobalDispose
                | Self::StorageRepair
                | Self::BrowserInstall
                | Self::BrowserSmokeTest
                | Self::WorktreeDelete
                | Self::WorktreeReset
                | Self::WorktreeCleanup
                | Self::PackInstall
                | Self::PackUninstall
                | Self::PackExport
                | Self::ProjectConfigUpdate
                | Self::GlobalConfigUpdate
                | Self::ApiTokenManage
                | Self::ChannelRead
                | Self::ChannelVerify
                | Self::ChannelConfigUpdate
                | Self::ChannelConfigDelete
                | Self::ChannelReload
                | Self::McpServerManage
        )
    }

    /// Git consumes repository-local configuration and attributes that can be
    /// changed concurrently by another host principal. Until managed Git runs
    /// against an immutable metadata snapshot, these effects are deliberately
    /// restricted to the standalone loopback owner boundary.
    fn requires_standalone_git_boundary(self) -> bool {
        matches!(
            self,
            Self::CommandExecute
                | Self::WorktreeList
                | Self::WorktreeCreate
                | Self::WorktreeDelete
                | Self::WorktreeReset
                | Self::WorktreeCleanup
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalHostResource {
    pub kind: String,
    pub id: String,
    pub tenant_context: TenantContext,
}

impl CanonicalHostResource {
    pub fn new(
        kind: impl Into<String>,
        id: impl Into<String>,
        tenant_context: TenantContext,
    ) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
            tenant_context,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostEffectRequest {
    pub action: HostAction,
    pub resource: CanonicalHostResource,
    #[serde(default)]
    pub arguments: Value,
}

impl HostEffectRequest {
    pub fn new(action: HostAction, resource: CanonicalHostResource, arguments: Value) -> Self {
        Self {
            action,
            resource,
            arguments,
        }
    }

    fn digest(&self) -> Result<String, HostAuthorizationError> {
        let payload =
            serde_json::to_vec(self).map_err(|_| HostAuthorizationError::InvalidEffectArguments)?;
        Ok(format!("{:x}", Sha256::digest(payload)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AuthoritySource {
    VerifiedCapability,
    LoopbackLocalOwner,
    InternalRuntime,
}

#[derive(Debug, Clone)]
pub struct AuthorizedHostEffect {
    request_digest: String,
    tenant_context: TenantContext,
    capability: &'static str,
    source: AuthoritySource,
    expires_at_ms: u64,
}

impl AuthorizedHostEffect {
    pub fn revalidate(
        &self,
        state: &AppState,
        request: &HostEffectRequest,
    ) -> Result<(), HostAuthorizationError> {
        if crate::now_ms() > self.expires_at_ms {
            return Err(HostAuthorizationError::GrantExpired);
        }
        if request.resource.tenant_context.org_id != self.tenant_context.org_id
            || request.resource.tenant_context.workspace_id != self.tenant_context.workspace_id
            || request.resource.tenant_context.deployment_id != self.tenant_context.deployment_id
            || request.resource.tenant_context.actor_id != self.tenant_context.actor_id
            || request.action.capability() != self.capability
            || request.digest()? != self.request_digest
        {
            return Err(HostAuthorizationError::GrantMismatch);
        }
        if self.source == AuthoritySource::LoopbackLocalOwner
            && (!state.host_operations_loopback_only()
                || !base_url_is_loopback(&state.server_base_url())
                || !self.tenant_context.is_local_implicit())
        {
            return Err(HostAuthorizationError::LocalOwnerPostureUnavailable);
        }
        if self.source == AuthoritySource::InternalRuntime
            && standalone_git_capability(self.capability)
            && (!state.host_operations_loopback_only()
                || !base_url_is_loopback(&state.server_base_url())
                || !self.tenant_context.is_local_implicit())
        {
            return Err(HostAuthorizationError::HostedGitBoundaryUnavailable);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAuthorizationError {
    CrossTenantResource,
    MissingVerifiedContext,
    ContextMismatch,
    AssertionExpired,
    MissingCapability,
    LocalOwnerPostureUnavailable,
    HostedGitBoundaryUnavailable,
    InvalidEffectArguments,
    AuditPersistenceFailed,
    GrantExpired,
    GrantMismatch,
}

impl HostAuthorizationError {
    pub fn code(self) -> &'static str {
        match self {
            Self::CrossTenantResource => "cross_tenant_resource",
            Self::MissingVerifiedContext => "missing_verified_context",
            Self::ContextMismatch => "verified_context_mismatch",
            Self::AssertionExpired => "verified_context_expired",
            Self::MissingCapability => "missing_capability",
            Self::LocalOwnerPostureUnavailable => "local_owner_posture_unavailable",
            Self::HostedGitBoundaryUnavailable => "hosted_git_boundary_unavailable",
            Self::InvalidEffectArguments => "invalid_effect_arguments",
            Self::AuditPersistenceFailed => "audit_persistence_failed",
            Self::GrantExpired => "authorization_grant_expired",
            Self::GrantMismatch => "authorization_grant_mismatch",
        }
    }
}

pub async fn authorize_host_effect(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    direct_loopback_request: bool,
    request: &HostEffectRequest,
) -> Result<AuthorizedHostEffect, HostAuthorizationError> {
    if !same_tenant(&request.resource.tenant_context, tenant) {
        audit_denial(
            state,
            tenant,
            verified,
            request,
            HostAuthorizationError::CrossTenantResource,
        )
        .await;
        return Err(HostAuthorizationError::CrossTenantResource);
    }

    let now = crate::now_ms();
    let capability = request.action.capability();
    let (source, actor_id, assertion_id, expires_at_ms) = if let Some(verified) = verified {
        if !same_tenant(&verified.tenant_context, tenant)
            || verified.tenant_context.actor_id != tenant.actor_id
            || Some(verified.human_actor.actor_id.as_str()) != tenant.actor_id.as_deref()
        {
            audit_denial(
                state,
                tenant,
                Some(verified),
                request,
                HostAuthorizationError::ContextMismatch,
            )
            .await;
            return Err(HostAuthorizationError::ContextMismatch);
        }
        if verified.expires_at_ms <= now {
            audit_denial(
                state,
                tenant,
                Some(verified),
                request,
                HostAuthorizationError::AssertionExpired,
            )
            .await;
            return Err(HostAuthorizationError::AssertionExpired);
        }
        if request.action.requires_standalone_git_boundary() {
            audit_denial(
                state,
                tenant,
                Some(verified),
                request,
                HostAuthorizationError::HostedGitBoundaryUnavailable,
            )
            .await;
            return Err(HostAuthorizationError::HostedGitBoundaryUnavailable);
        }
        let exact = verified
            .capabilities
            .iter()
            .any(|candidate| candidate == capability);
        let deployment_admin = verified
            .capabilities
            .iter()
            .any(|candidate| candidate == "deployment.admin");
        let permitted = if request.action.requires_deployment_admin() {
            deployment_admin
        } else {
            exact || deployment_admin
        };
        if !permitted {
            audit_denial(
                state,
                tenant,
                Some(verified),
                request,
                HostAuthorizationError::MissingCapability,
            )
            .await;
            return Err(HostAuthorizationError::MissingCapability);
        }
        (
            AuthoritySource::VerifiedCapability,
            Some(verified.human_actor.actor_id.clone()),
            Some(verified.assertion_id.clone()),
            verified
                .expires_at_ms
                .min(now.saturating_add(HOST_EFFECT_GRANT_TTL_MS)),
        )
    } else if direct_loopback_request
        && state.host_operations_loopback_only()
        && base_url_is_loopback(&state.server_base_url())
        && tenant.is_local_implicit()
    {
        (
            AuthoritySource::LoopbackLocalOwner,
            None,
            None,
            now.saturating_add(HOST_EFFECT_GRANT_TTL_MS),
        )
    } else {
        audit_denial(
            state,
            tenant,
            None,
            request,
            HostAuthorizationError::MissingVerifiedContext,
        )
        .await;
        return Err(HostAuthorizationError::MissingVerifiedContext);
    };

    let request_digest = request.digest()?;
    crate::audit::append_protected_audit_event(
        state,
        "authority.host_effect.granted",
        tenant,
        actor_id,
        json!({
            "action": request.action,
            "capability": capability,
            "resource": request.resource,
            "effect_digest": request_digest,
            "authority_source": source,
            "assertion_id": assertion_id,
            "grant_expires_at_ms": expires_at_ms,
        }),
    )
    .await
    .map_err(|_| HostAuthorizationError::AuditPersistenceFailed)?;

    Ok(AuthorizedHostEffect {
        request_digest,
        tenant_context: tenant.clone(),
        capability,
        source,
        expires_at_ms,
    })
}

/// Authorize a host effect initiated by a trusted in-process worker.
///
/// The caller must resolve the resource and tenant from stored state before
/// calling this function. The returned grant is still exact-argument-bound,
/// short-lived, protected-audited, and must be revalidated at the effect.
pub async fn authorize_internal_host_effect(
    state: &AppState,
    caller: &'static str,
    request: &HostEffectRequest,
) -> Result<AuthorizedHostEffect, HostAuthorizationError> {
    if request.action.requires_standalone_git_boundary()
        && (!state.host_operations_loopback_only()
            || !base_url_is_loopback(&state.server_base_url())
            || !request.resource.tenant_context.is_local_implicit())
    {
        audit_denial(
            state,
            &request.resource.tenant_context,
            None,
            request,
            HostAuthorizationError::HostedGitBoundaryUnavailable,
        )
        .await;
        return Err(HostAuthorizationError::HostedGitBoundaryUnavailable);
    }
    let now = crate::now_ms();
    let expires_at_ms = now.saturating_add(HOST_EFFECT_GRANT_TTL_MS);
    let request_digest = request.digest()?;
    crate::audit::append_protected_audit_event(
        state,
        "authority.internal_host_effect.granted",
        &request.resource.tenant_context,
        None,
        json!({
            "caller": caller,
            "action": request.action,
            "capability": request.action.capability(),
            "resource": request.resource,
            "effect_digest": request_digest,
            "authority_source": AuthoritySource::InternalRuntime,
            "grant_expires_at_ms": expires_at_ms,
        }),
    )
    .await
    .map_err(|_| HostAuthorizationError::AuditPersistenceFailed)?;

    Ok(AuthorizedHostEffect {
        request_digest,
        tenant_context: request.resource.tenant_context.clone(),
        capability: request.action.capability(),
        source: AuthoritySource::InternalRuntime,
        expires_at_ms,
    })
}

async fn audit_denial(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    request: &HostEffectRequest,
    error: HostAuthorizationError,
) {
    crate::audit::append_protected_audit_event_best_effort(
        state,
        "authority.host_effect.denied",
        tenant,
        verified.map(|context| context.human_actor.actor_id.clone()),
        json!({
            "reason": error.code(),
            "action": request.action,
            "capability": request.action.capability(),
            "resource": request.resource,
            "assertion_id": verified.map(|context| context.assertion_id.as_str()),
        }),
    )
    .await;
}

fn same_tenant(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
        && left.actor_id == right.actor_id
}

fn standalone_git_capability(capability: &str) -> bool {
    capability == HostAction::CommandExecute.capability()
        || capability.starts_with("host.worktree.")
}

fn base_url_is_loopback(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_types::{AuthorityChain, HumanActor, RequestPrincipal};

    fn tenant(actor: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            actor,
        )
    }

    fn verified(
        tenant_context: TenantContext,
        capabilities: &[&str],
        roles: &[&str],
    ) -> VerifiedTenantContext {
        let actor_id = tenant_context
            .actor_id
            .clone()
            .expect("verified test tenant has actor");
        let principal = RequestPrincipal::authenticated_user(&actor_id, "test");
        let now = crate::now_ms();
        VerifiedTenantContext {
            tenant_context,
            human_actor: HumanActor::tandem_user(actor_id),
            authority_chain: AuthorityChain::from_request(principal),
            roles: roles.iter().map(|value| (*value).to_string()).collect(),
            org_units: Vec::new(),
            capabilities: capabilities
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            policy_version: Some(1),
            strict_projection: None,
            issuer: "test".to_string(),
            audience: "tandem-server".to_string(),
            issued_at_ms: now,
            expires_at_ms: now.saturating_add(60_000),
            assertion_id: format!("assertion-{now}"),
            assertion_key_id: Some("test-key".to_string()),
        }
    }

    fn file_request(tenant_context: TenantContext, path: &str) -> HostEffectRequest {
        HostEffectRequest::new(
            HostAction::FileRead,
            CanonicalHostResource::new("session_workspace", "session-a", tenant_context),
            json!({"path": path, "byte_limit": 1024}),
        )
    }

    #[tokio::test]
    async fn broad_role_does_not_authorize_host_effect() {
        let state = crate::test_support::test_state().await;
        let tenant = tenant("actor-a");
        let verified = verified(tenant.clone(), &[], &["enterprise:admin"]);
        let request = file_request(tenant.clone(), "README.md");

        let error = authorize_host_effect(&state, &tenant, Some(&verified), false, &request)
            .await
            .expect_err("a broad role must not imply a host capability");

        assert_eq!(error, HostAuthorizationError::MissingCapability);
    }

    #[tokio::test]
    async fn admin_only_action_rejects_action_specific_capability() {
        let state = crate::test_support::test_state().await;
        let tenant = tenant("actor-a");
        let verified = verified(
            tenant.clone(),
            &[HostAction::PackUninstall.capability()],
            &[],
        );
        let request = HostEffectRequest::new(
            HostAction::PackUninstall,
            CanonicalHostResource::new("local_pack_store", "local-pack-store", tenant.clone()),
            json!({"pack_id": "pack-a"}),
        );

        let error = authorize_host_effect(&state, &tenant, Some(&verified), false, &request)
            .await
            .expect_err("destructive host actions require deployment.admin");

        assert_eq!(error, HostAuthorizationError::MissingCapability);
    }

    #[tokio::test]
    async fn hosted_git_effects_fail_closed_even_for_deployment_admin() {
        let state = crate::test_support::test_state().await;
        let tenant = tenant("actor-a");
        let verified = verified(tenant.clone(), &["deployment.admin"], &[]);
        for action in [
            HostAction::CommandExecute,
            HostAction::WorktreeList,
            HostAction::WorktreeCreate,
            HostAction::WorktreeDelete,
            HostAction::WorktreeReset,
            HostAction::WorktreeCleanup,
        ] {
            let request = HostEffectRequest::new(
                action,
                CanonicalHostResource::new("repository", "repository-a", tenant.clone()),
                json!({"operation": "test"}),
            );
            let error = authorize_host_effect(&state, &tenant, Some(&verified), false, &request)
                .await
                .expect_err(
                    "mutable local Git metadata must not be reachable from hosted authority",
                );
            assert_eq!(error, HostAuthorizationError::HostedGitBoundaryUnavailable);
        }
    }

    #[tokio::test]
    async fn internal_hosted_worktree_effect_fails_closed() {
        let state = crate::test_support::test_state().await;
        let tenant = tenant("actor-a");
        let request = HostEffectRequest::new(
            HostAction::WorktreeCreate,
            CanonicalHostResource::new("repository", "repository-a", tenant),
            json!({"operation": "test"}),
        );

        let error = authorize_internal_host_effect(&state, "test", &request)
            .await
            .expect_err("internal callers must not bypass the standalone Git boundary");
        assert_eq!(error, HostAuthorizationError::HostedGitBoundaryUnavailable);
    }

    #[tokio::test]
    async fn known_cross_tenant_resource_is_rejected() {
        let state = crate::test_support::test_state().await;
        let tenant_a = tenant("actor-a");
        let mut tenant_b = tenant("actor-b");
        tenant_b.org_id = "org-b".to_string();
        let verified = verified(tenant_a.clone(), &["host.files.read"], &[]);
        let request = file_request(tenant_b, "README.md");

        let error = authorize_host_effect(&state, &tenant_a, Some(&verified), false, &request)
            .await
            .expect_err("known IDs must not cross tenant boundaries");

        assert_eq!(error, HostAuthorizationError::CrossTenantResource);
    }

    #[tokio::test]
    async fn grant_revalidation_rejects_argument_substitution() {
        let state = crate::test_support::test_state().await;
        let tenant = tenant("actor-a");
        let verified = verified(tenant.clone(), &["host.files.read"], &[]);
        let request = file_request(tenant.clone(), "README.md");
        let grant = authorize_host_effect(&state, &tenant, Some(&verified), false, &request)
            .await
            .expect("exact capability and resource authorize");

        grant
            .revalidate(&state, &request)
            .expect("unchanged exact effect revalidates");
        let substituted = file_request(tenant, "../secret");
        assert_eq!(
            grant.revalidate(&state, &substituted),
            Err(HostAuthorizationError::GrantMismatch)
        );
    }

    #[tokio::test]
    async fn audit_persistence_failure_prevents_grant_issuance() {
        let mut state = crate::test_support::test_state().await;
        let blocked_parent = state.protected_audit_path.with_extension("blocked");
        if let Some(parent) = blocked_parent.parent() {
            std::fs::create_dir_all(parent).expect("create audit test directory");
        }
        std::fs::write(&blocked_parent, b"not a directory").expect("create blocked parent");
        state.protected_audit_path = blocked_parent.join("audit.jsonl");
        let tenant = tenant("actor-a");
        let verified = verified(tenant.clone(), &["host.files.read"], &[]);
        let request = file_request(tenant.clone(), "README.md");

        let error = authorize_host_effect(&state, &tenant, Some(&verified), false, &request)
            .await
            .expect_err("audit persistence must commit before grant issuance");

        assert_eq!(error, HostAuthorizationError::AuditPersistenceFailed);
    }
}
