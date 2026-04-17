use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterpriseMode {
    Disabled,
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterpriseBridgeState {
    Absent,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterpriseCapability {
    Status,
    TenantContext,
    NoopBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TenantSource {
    #[default]
    LocalImplicit,
    Explicit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestPrincipal {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default)]
    pub source: String,
}

impl RequestPrincipal {
    pub fn anonymous() -> Self {
        Self {
            actor_id: None,
            source: "anonymous".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationPrincipal {
    pub automation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionPrincipal {
    Request(RequestPrincipal),
    Automation(AutomationPrincipal),
    ServiceAccount {
        service_account_id: String,
    },
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityChain {
    pub initiated_by: RequestPrincipal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<AutomationPrincipal>,
    pub executed_as: ExecutionPrincipal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<RequestPrincipal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LocalImplicitTenant;

impl LocalImplicitTenant {
    pub const ORG_ID: &'static str = "local";
    pub const WORKSPACE_ID: &'static str = "local";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantContext {
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default)]
    pub source: TenantSource,
}

impl Default for TenantContext {
    fn default() -> Self {
        Self::local_implicit()
    }
}

impl TenantContext {
    pub fn local_implicit() -> Self {
        Self {
            org_id: LocalImplicitTenant::ORG_ID.to_string(),
            workspace_id: LocalImplicitTenant::WORKSPACE_ID.to_string(),
            actor_id: None,
            source: TenantSource::LocalImplicit,
        }
    }

    pub fn explicit(
        org_id: impl Into<String>,
        workspace_id: impl Into<String>,
        actor_id: Option<String>,
    ) -> Self {
        Self {
            org_id: org_id.into(),
            workspace_id: workspace_id.into(),
            actor_id,
            source: TenantSource::Explicit,
        }
    }

    pub fn is_local_implicit(&self) -> bool {
        self.source == TenantSource::LocalImplicit
            && self.org_id == LocalImplicitTenant::ORG_ID
            && self.workspace_id == LocalImplicitTenant::WORKSPACE_ID
            && self.actor_id.is_none()
    }
}

impl From<LocalImplicitTenant> for TenantContext {
    fn from(_: LocalImplicitTenant) -> Self {
        Self::local_implicit()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRef {
    pub org_id: String,
    pub workspace_id: String,
    pub provider: String,
    pub secret_id: String,
    pub name: String,
}

impl SecretRef {
    pub fn validate_for_tenant(&self, ctx: &TenantContext) -> Result<(), SecretRefError> {
        if self.org_id != ctx.org_id {
            return Err(SecretRefError::OrgMismatch);
        }
        if self.workspace_id != ctx.workspace_id {
            return Err(SecretRefError::WorkspaceMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretRefError {
    OrgMismatch,
    WorkspaceMismatch,
    NotFound,
}

impl core::fmt::Display for SecretRefError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OrgMismatch => write!(f, "secret org does not match request context"),
            Self::WorkspaceMismatch => write!(f, "secret workspace does not match request context"),
            Self::NotFound => write!(f, "secret not found"),
        }
    }
}

impl std::error::Error for SecretRefError {}

pub trait TenantContextResolver: Send + Sync {
    fn resolve_tenant_context(
        &self,
        org_id: Option<&str>,
        workspace_id: Option<&str>,
        actor_id: Option<&str>,
    ) -> TenantContext;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HeaderTenantContextResolver;

impl TenantContextResolver for HeaderTenantContextResolver {
    fn resolve_tenant_context(
        &self,
        org_id: Option<&str>,
        workspace_id: Option<&str>,
        actor_id: Option<&str>,
    ) -> TenantContext {
        let org_id = org_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(LocalImplicitTenant::ORG_ID);
        let workspace_id = workspace_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(LocalImplicitTenant::WORKSPACE_ID);
        let actor_id = actor_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);

        if org_id == LocalImplicitTenant::ORG_ID
            && workspace_id == LocalImplicitTenant::WORKSPACE_ID
            && actor_id.is_none()
        {
            TenantContext::local_implicit()
        } else {
            TenantContext::explicit(org_id.to_string(), workspace_id.to_string(), actor_id)
        }
    }
}

pub trait RequestAuthorizationHook: Send + Sync {
    fn authorize(&self, principal: &RequestPrincipal, tenant: &TenantContext) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopRequestAuthorizationHook;

impl RequestAuthorizationHook for NoopRequestAuthorizationHook {
    fn authorize(&self, _principal: &RequestPrincipal, _tenant: &TenantContext) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnterpriseStatus {
    pub mode: EnterpriseMode,
    pub bridge_state: EnterpriseBridgeState,
    #[serde(default)]
    pub capabilities: Vec<EnterpriseCapability>,
    pub tenant_context: TenantContext,
    pub public_build: bool,
    pub contract_version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl EnterpriseStatus {
    pub fn public_oss() -> Self {
        Self {
            mode: EnterpriseMode::Disabled,
            bridge_state: EnterpriseBridgeState::Absent,
            capabilities: vec![
                EnterpriseCapability::Status,
                EnterpriseCapability::TenantContext,
            ],
            tenant_context: TenantContext::local_implicit(),
            public_build: true,
            contract_version: "v1".to_string(),
            notes: vec![
                "enterprise bridge is not configured".to_string(),
                "OSS mode uses a local implicit tenant until enterprise mode is enabled"
                    .to_string(),
            ],
        }
    }
}

pub trait EnterpriseBridge: Send + Sync {
    fn status(&self) -> EnterpriseStatus;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopEnterpriseBridge;

impl EnterpriseBridge for NoopEnterpriseBridge {
    fn status(&self) -> EnterpriseStatus {
        EnterpriseStatus::public_oss()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_ref_validation_rejects_cross_tenant_access() {
        let secret_ref = SecretRef {
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            provider: "mcp_header".to_string(),
            secret_id: "secret-a".to_string(),
            name: "authorization".to_string(),
        };
        let tenant = TenantContext::explicit("org-a", "workspace-a", None);
        assert!(secret_ref.validate_for_tenant(&tenant).is_ok());

        let wrong_workspace = TenantContext::explicit("org-a", "workspace-b", None);
        assert!(matches!(
            secret_ref.validate_for_tenant(&wrong_workspace),
            Err(SecretRefError::WorkspaceMismatch)
        ));
    }

    #[test]
    fn header_resolver_defaults_to_local_tenant() {
        let resolver = HeaderTenantContextResolver;
        let tenant = resolver.resolve_tenant_context(None, None, None);
        assert!(tenant.is_local_implicit());
    }

    #[test]
    fn request_authorization_hook_is_noop_by_default() {
        let hook = NoopRequestAuthorizationHook;
        let principal = RequestPrincipal::anonymous();
        let tenant = TenantContext::local_implicit();
        assert!(hook.authorize(&principal, &tenant));
    }
}
