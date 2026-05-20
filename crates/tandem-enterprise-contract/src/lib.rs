pub mod governance;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterpriseMode {
    Disabled,
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAuthMode {
    #[default]
    LocalSingleTenant,
    HostedSingleTenant,
    EnterpriseRequired,
}

impl RuntimeAuthMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalSingleTenant => "local_single_tenant",
            Self::HostedSingleTenant => "hosted_single_tenant",
            Self::EnterpriseRequired => "enterprise_required",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ParseRuntimeAuthModeError> {
        value.parse()
    }
}

impl core::str::FromStr for RuntimeAuthMode {
    type Err = ParseRuntimeAuthModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            ""
            | "local"
            | "local_single_tenant"
            | "local-single-tenant"
            | "single_tenant"
            | "single-tenant" => Ok(Self::LocalSingleTenant),
            "hosted" | "hosted_single_tenant" | "hosted-single-tenant" => {
                Ok(Self::HostedSingleTenant)
            }
            "enterprise" | "enterprise_required" | "enterprise-required" | "required" => {
                Ok(Self::EnterpriseRequired)
            }
            _ => Err(ParseRuntimeAuthModeError),
        }
    }
}

impl core::fmt::Display for RuntimeAuthMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseRuntimeAuthModeError;

impl core::fmt::Display for ParseRuntimeAuthModeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("invalid runtime auth mode")
    }
}

impl std::error::Error for ParseRuntimeAuthModeError {}

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

    pub fn authenticated_user(actor_id: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            actor_id: Some(actor_id.into()),
            source: source.into(),
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

impl AuthorityChain {
    pub fn from_request(principal: RequestPrincipal) -> Self {
        Self {
            initiated_by: principal.clone(),
            owned_by: None,
            executed_as: ExecutionPrincipal::Request(principal),
            approved_by: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanActor {
    pub actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl HumanActor {
    pub fn tandem_user(actor_id: impl Into<String>) -> Self {
        Self {
            actor_id: actor_id.into(),
            provider: Some("tandem".to_string()),
            issuer: None,
            subject: None,
            email: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Organization,
    Workspace,
    Department,
    Group,
    Project,
    DataRoom,
    SharedDrive,
    DocumentCollection,
    DataStore,
    Dataset,
    Document,
    Repository,
    Directory,
    File,
    Artifact,
    MemorySpace,
    KnowledgeSpace,
    SecretProviderCredential,
    Automation,
    Run,
    Approval,
    AuditExport,
    McpServer,
    McpTool,
    ExternalIntegrationAccount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourcePathSegment {
    pub kind: ResourceKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ResourcePathSegment {
    pub fn new(kind: ResourceKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            name: None,
        }
    }

    pub fn named(kind: ResourceKind, id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            name: Some(name.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRef {
    pub organization_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub resource_kind: ResourceKind,
    pub resource_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent_path: Vec<ResourcePathSegment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
}

impl ResourceRef {
    pub fn new(
        organization_id: impl Into<String>,
        workspace_id: impl Into<String>,
        resource_kind: ResourceKind,
        resource_id: impl Into<String>,
    ) -> Self {
        Self {
            organization_id: organization_id.into(),
            workspace_id: workspace_id.into(),
            project_id: None,
            resource_kind,
            resource_id: resource_id.into(),
            parent_path: Vec::new(),
            branch_id: None,
            path_prefix: None,
        }
    }

    pub fn with_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }

    pub fn with_parent_path(mut self, parent_path: Vec<ResourcePathSegment>) -> Self {
        self.parent_path = parent_path;
        self
    }

    pub fn with_branch_id(mut self, branch_id: impl Into<String>) -> Self {
        self.branch_id = Some(branch_id.into());
        self
    }

    pub fn with_path_prefix(mut self, path_prefix: impl Into<String>) -> Self {
        self.path_prefix = Some(path_prefix.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceScope {
    pub root: ResourceRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_resources: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_resources: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
}

impl ResourceScope {
    pub fn root(root: ResourceRef) -> Self {
        Self {
            root,
            allowed_resources: Vec::new(),
            denied_resources: Vec::new(),
            max_depth: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessPermission {
    View,
    Read,
    Edit,
    Execute,
    Delegate,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    Public,
    Internal,
    Confidential,
    Restricted,
    Executive,
    Credential,
    Regulated,
    CustomerData,
    SourceCode,
    FinancialRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    HumanUser,
    Group,
    Department,
    AgentWorker,
    Automation,
    ServiceAccount,
    ExternalDelegate,
    SupportOperator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalRef {
    pub kind: PrincipalKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

impl PrincipalRef {
    pub fn new(kind: PrincipalKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            tenant_actor_id: None,
            issuer: None,
            subject: None,
        }
    }

    pub fn human_user(id: impl Into<String>) -> Self {
        Self::new(PrincipalKind::HumanUser, id)
    }

    pub fn agent_worker(id: impl Into<String>) -> Self {
        Self::new(PrincipalKind::AgentWorker, id)
    }

    pub fn with_tenant_actor_id(mut self, tenant_actor_id: impl Into<String>) -> Self {
        self.tenant_actor_id = Some(tenant_actor_id.into());
        self
    }

    pub fn with_issuer_subject(
        mut self,
        issuer: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        self.issuer = Some(issuer.into());
        self.subject = Some(subject.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantSource {
    Direct,
    GroupMembership,
    DepartmentMembership,
    Inherited,
    ExecutiveGlobal,
    Delegation,
    BreakGlass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedGrant {
    pub grant_id: String,
    pub principal: PrincipalRef,
    pub resource: ResourceRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<AccessPermission>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_patterns: Vec<String>,
    pub grant_source: GrantSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_principal: Option<PrincipalRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_id: Option<String>,
}

impl ScopedGrant {
    pub fn new(
        grant_id: impl Into<String>,
        principal: PrincipalRef,
        resource: ResourceRef,
        grant_source: GrantSource,
    ) -> Self {
        Self {
            grant_id: grant_id.into(),
            principal,
            resource,
            permissions: Vec::new(),
            data_classes: Vec::new(),
            tool_patterns: Vec::new(),
            grant_source,
            source_principal: None,
            expires_at_ms: None,
            delegation_id: None,
        }
    }

    pub fn with_permissions(mut self, permissions: Vec<AccessPermission>) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn with_data_classes(mut self, data_classes: Vec<DataClass>) -> Self {
        self.data_classes = data_classes;
        self
    }

    pub fn with_tool_patterns(mut self, tool_patterns: Vec<String>) -> Self {
        self.tool_patterns = tool_patterns;
        self
    }

    pub fn with_source_principal(mut self, source_principal: PrincipalRef) -> Self {
        self.source_principal = Some(source_principal);
        self
    }

    pub fn with_expires_at_ms(mut self, expires_at_ms: u64) -> Self {
        self.expires_at_ms = Some(expires_at_ms);
        self
    }

    pub fn with_delegation_id(mut self, delegation_id: impl Into<String>) -> Self {
        self.delegation_id = Some(delegation_id.into());
        self
    }

    pub fn has_permission(&self, permission: AccessPermission) -> bool {
        self.permissions.contains(&permission)
    }

    pub fn allows_data_class(&self, data_class: DataClass) -> bool {
        self.data_classes.contains(&data_class)
    }

    pub fn is_expired_at(&self, now_ms: u64) -> bool {
        self.expires_at_ms
            .map(|expires_at_ms| expires_at_ms <= now_ms)
            .unwrap_or(false)
    }
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
    pub deployment_id: Option<String>,
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
            deployment_id: None,
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
            deployment_id: None,
            actor_id,
            source: TenantSource::Explicit,
        }
    }

    pub fn explicit_user_workspace(
        org_id: impl Into<String>,
        workspace_id: impl Into<String>,
        deployment_id: Option<String>,
        actor_id: impl Into<String>,
    ) -> Self {
        Self {
            org_id: org_id.into(),
            workspace_id: workspace_id.into(),
            deployment_id,
            actor_id: Some(actor_id.into()),
            source: TenantSource::Explicit,
        }
    }

    pub fn is_local_implicit(&self) -> bool {
        self.source == TenantSource::LocalImplicit
            && self.org_id == LocalImplicitTenant::ORG_ID
            && self.workspace_id == LocalImplicitTenant::WORKSPACE_ID
            && self.deployment_id.is_none()
            && self.actor_id.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedTenantContext {
    pub tenant_context: TenantContext,
    pub human_actor: HumanActor,
    pub authority_chain: AuthorityChain,
    pub issuer: String,
    pub audience: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub assertion_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantContextAssertionHeader {
    pub alg: String,
    pub typ: String,
    pub kid: String,
}

impl TenantContextAssertionHeader {
    pub fn ed25519(key_id: impl Into<String>) -> Self {
        Self {
            alg: "EdDSA".to_string(),
            typ: "tandem-tenant-context+jws".to_string(),
            kid: key_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantContextAssertionClaims {
    pub version: String,
    pub issuer: String,
    pub audience: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub assertion_id: String,
    pub tenant_context: TenantContext,
    pub human_actor: HumanActor,
    pub authority_chain: AuthorityChain,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

impl TenantContextAssertionClaims {
    #[allow(clippy::too_many_arguments)]
    pub fn new_v1(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        issued_at_ms: u64,
        expires_at_ms: u64,
        assertion_id: impl Into<String>,
        tenant_context: TenantContext,
        human_actor: HumanActor,
        authority_chain: AuthorityChain,
        roles: Vec<String>,
    ) -> Self {
        Self {
            version: "v1".to_string(),
            issuer: issuer.into(),
            audience: audience.into(),
            issued_at_ms,
            expires_at_ms,
            assertion_id: assertion_id.into(),
            tenant_context,
            human_actor,
            authority_chain,
            roles,
        }
    }

    pub fn is_expired_at(&self, now_ms: u64) -> bool {
        self.expires_at_ms <= now_ms
    }
}

impl From<TenantContextAssertionClaims> for VerifiedTenantContext {
    fn from(claims: TenantContextAssertionClaims) -> Self {
        Self {
            tenant_context: claims.tenant_context,
            human_actor: claims.human_actor,
            authority_chain: claims.authority_chain,
            issuer: claims.issuer,
            audience: claims.audience,
            issued_at_ms: claims.issued_at_ms,
            expires_at_ms: claims.expires_at_ms,
            assertion_id: claims.assertion_id,
        }
    }
}

impl VerifiedTenantContext {
    pub fn is_expired_at(&self, now_ms: u64) -> bool {
        self.expires_at_ms <= now_ms
    }

    pub fn tenant_matches(&self, tenant: &TenantContext) -> bool {
        self.tenant_context.org_id == tenant.org_id
            && self.tenant_context.workspace_id == tenant.workspace_id
            && self.tenant_context.deployment_id == tenant.deployment_id
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
    fn explicit_user_workspace_preserves_actor_and_deployment() {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "user-a",
        );

        assert_eq!(tenant.org_id, "org-a");
        assert_eq!(tenant.workspace_id, "workspace-a");
        assert_eq!(tenant.deployment_id.as_deref(), Some("deployment-a"));
        assert_eq!(tenant.actor_id.as_deref(), Some("user-a"));
        assert_eq!(tenant.source, TenantSource::Explicit);
        assert!(!tenant.is_local_implicit());
    }

    #[test]
    fn authority_chain_from_request_executes_as_same_actor() {
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem_web");
        let chain = AuthorityChain::from_request(principal.clone());

        assert_eq!(chain.initiated_by, principal);
        assert!(chain.owned_by.is_none());
        assert!(chain.approved_by.is_none());
        assert_eq!(chain.executed_as, ExecutionPrincipal::Request(principal));
    }

    #[test]
    fn verified_tenant_context_checks_expiry_and_tenant_match() {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "user-a",
        );
        let actor = HumanActor::tandem_user("user-a");
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem_web");
        let verified = VerifiedTenantContext {
            tenant_context: tenant.clone(),
            human_actor: actor,
            authority_chain: AuthorityChain::from_request(principal),
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 100,
            expires_at_ms: 200,
            assertion_id: "assertion-1".to_string(),
        };

        assert!(!verified.is_expired_at(199));
        assert!(verified.is_expired_at(200));
        assert!(verified.tenant_matches(&tenant));
        assert!(!verified.tenant_matches(&TenantContext::explicit(
            "org-b",
            "workspace-a",
            Some("user-a".to_string()),
        )));
    }

    #[test]
    fn runtime_auth_mode_parses_operator_aliases() {
        assert_eq!(
            RuntimeAuthMode::parse("local"),
            Ok(RuntimeAuthMode::LocalSingleTenant)
        );
        assert_eq!(
            RuntimeAuthMode::parse("hosted-single-tenant"),
            Ok(RuntimeAuthMode::HostedSingleTenant)
        );
        assert_eq!(
            RuntimeAuthMode::parse("enterprise_required"),
            Ok(RuntimeAuthMode::EnterpriseRequired)
        );
        assert!(RuntimeAuthMode::parse("definitely-not-a-mode").is_err());
        assert_eq!(
            RuntimeAuthMode::EnterpriseRequired.to_string(),
            "enterprise_required"
        );
    }

    #[test]
    fn tenant_context_assertion_claims_convert_to_verified_context() {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "user-a",
        );
        let actor = HumanActor::tandem_user("user-a");
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem_web");
        let chain = AuthorityChain::from_request(principal);
        let claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            100,
            200,
            "assertion-1",
            tenant.clone(),
            actor.clone(),
            chain.clone(),
            vec!["operator".to_string(), "approver".to_string()],
        );

        assert_eq!(claims.version, "v1");
        assert!(!claims.is_expired_at(199));
        assert!(claims.is_expired_at(200));

        let verified = VerifiedTenantContext::from(claims);
        assert_eq!(verified.tenant_context, tenant);
        assert_eq!(verified.human_actor, actor);
        assert_eq!(verified.authority_chain, chain);
        assert_eq!(verified.issuer, "tandem-web");
        assert_eq!(verified.audience, "tandem-runtime");
        assert_eq!(verified.assertion_id, "assertion-1");
    }

    #[test]
    fn tenant_context_assertion_header_uses_eddsa_jws_typ() {
        let header = TenantContextAssertionHeader::ed25519("key-1");
        assert_eq!(header.alg, "EdDSA");
        assert_eq!(header.typ, "tandem-tenant-context+jws");
        assert_eq!(header.kid, "key-1");
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

    #[test]
    fn resource_ref_round_trips_finance_workspace_data_store() {
        let resource =
            ResourceRef::new("acme", "finance", ResourceKind::DataStore, "finance-ledger")
                .with_parent_path(vec![
                    ResourcePathSegment::named(ResourceKind::Department, "finance", "Finance"),
                    ResourcePathSegment::named(
                        ResourceKind::SharedDrive,
                        "finance-drive",
                        "Finance",
                    ),
                ]);

        let encoded = serde_json::to_string(&resource).expect("serialize resource ref");
        assert!(encoded.contains("\"resource_kind\":\"data_store\""));

        let decoded: ResourceRef =
            serde_json::from_str(&encoded).expect("deserialize resource ref");
        assert_eq!(decoded, resource);
        assert_eq!(decoded.organization_id, "acme");
        assert_eq!(decoded.workspace_id, "finance");
        assert_eq!(decoded.resource_kind, ResourceKind::DataStore);
    }

    #[test]
    fn resource_scope_models_engineering_repo_path_scope() {
        let repository =
            ResourceRef::new("acme", "engineering", ResourceKind::Repository, "tandem")
                .with_project_id("platform")
                .with_branch_id("main")
                .with_path_prefix("crates/tandem-enterprise-contract/");

        let scope = ResourceScope {
            root: ResourceRef::new("acme", "engineering", ResourceKind::Project, "platform"),
            allowed_resources: vec![repository.clone()],
            denied_resources: vec![ResourceRef::new(
                "acme",
                "engineering",
                ResourceKind::Directory,
                "secrets",
            )
            .with_project_id("platform")
            .with_path_prefix("crates/tandem-enterprise-contract/secrets/")],
            max_depth: Some(4),
        };

        let encoded = serde_json::to_value(&scope).expect("serialize resource scope");
        assert_eq!(
            encoded["allowed_resources"][0]["resource_kind"],
            "repository"
        );
        assert_eq!(
            encoded["allowed_resources"][0]["path_prefix"],
            "crates/tandem-enterprise-contract/"
        );

        let decoded: ResourceScope =
            serde_json::from_value(encoded).expect("deserialize resource scope");
        assert_eq!(decoded, scope);
        assert_eq!(decoded.allowed_resources, vec![repository]);
    }

    #[test]
    fn resource_scope_models_ceo_org_wide_executive_access() {
        let principal = PrincipalRef::human_user("ceo-user")
            .with_tenant_actor_id("user-ceo")
            .with_issuer_subject("https://idp.acme.example", "00uceo");
        let scope = ResourceScope::root(ResourceRef::new(
            "acme",
            "*",
            ResourceKind::Organization,
            "acme",
        ));

        assert_eq!(principal.kind, PrincipalKind::HumanUser);
        assert_eq!(principal.tenant_actor_id.as_deref(), Some("user-ceo"));
        assert_eq!(scope.root.resource_kind, ResourceKind::Organization);
        assert_eq!(scope.root.workspace_id, "*");
        assert!(scope.allowed_resources.is_empty());

        let encoded = serde_json::to_string(&DataClass::Executive).expect("serialize data class");
        assert_eq!(encoded, "\"executive\"");
    }

    #[test]
    fn mcp_tool_resource_target_and_permissions_are_transport_safe() {
        let tool = ResourceRef::new(
            "acme",
            "security",
            ResourceKind::McpTool,
            "mcp:google-drive:files.export",
        )
        .with_parent_path(vec![
            ResourcePathSegment::new(ResourceKind::McpServer, "google-drive"),
            ResourcePathSegment::new(ResourceKind::DataStore, "security-drive"),
        ]);
        let permissions = vec![AccessPermission::View, AccessPermission::Execute];
        let data_classes = vec![DataClass::Confidential, DataClass::Credential];
        let worker = PrincipalRef::agent_worker("agent-security-export");

        let payload = serde_json::json!({
            "principal": worker,
            "resource": tool,
            "permissions": permissions,
            "data_classes": data_classes,
        });

        assert_eq!(payload["principal"]["kind"], "agent_worker");
        assert_eq!(payload["resource"]["resource_kind"], "mcp_tool");
        assert_eq!(payload["permissions"][1], "execute");
        assert_eq!(payload["data_classes"][1], "credential");
    }

    #[test]
    fn scoped_grant_models_department_membership_data_access() {
        let finance_department = PrincipalRef::new(PrincipalKind::Department, "finance");
        let finance_user =
            PrincipalRef::human_user("user-finance").with_tenant_actor_id("actor-finance");
        let finance_store =
            ResourceRef::new("acme", "finance", ResourceKind::DataStore, "finance-ledger");
        let grant = ScopedGrant::new(
            "grant-finance-ledger-read",
            finance_user,
            finance_store,
            GrantSource::DepartmentMembership,
        )
        .with_source_principal(finance_department)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::FinancialRecord, DataClass::Confidential]);

        assert_eq!(grant.grant_source, GrantSource::DepartmentMembership);
        assert!(grant.has_permission(AccessPermission::Read));
        assert!(!grant.has_permission(AccessPermission::Edit));
        assert!(grant.allows_data_class(DataClass::FinancialRecord));
        assert!(!grant.allows_data_class(DataClass::Executive));

        let encoded = serde_json::to_value(&grant).expect("serialize department grant");
        assert_eq!(encoded["grant_source"], "department_membership");
        assert_eq!(encoded["source_principal"]["kind"], "department");
    }

    #[test]
    fn scoped_grant_models_cross_functional_group_access() {
        let launch_group = PrincipalRef::new(PrincipalKind::Group, "launch-team");
        let marketer = PrincipalRef::human_user("user-marketing");
        let launch_room = ResourceRef::new("acme", "gtm", ResourceKind::DataRoom, "q4-launch-room");
        let grant = ScopedGrant::new(
            "grant-launch-room-edit",
            marketer,
            launch_room,
            GrantSource::GroupMembership,
        )
        .with_source_principal(launch_group)
        .with_permissions(vec![
            AccessPermission::View,
            AccessPermission::Read,
            AccessPermission::Edit,
        ])
        .with_data_classes(vec![DataClass::Internal, DataClass::CustomerData]);

        let decoded: ScopedGrant =
            serde_json::from_value(serde_json::to_value(&grant).expect("serialize group grant"))
                .expect("deserialize group grant");
        assert_eq!(decoded, grant);
        assert_eq!(decoded.grant_source, GrantSource::GroupMembership);
        assert!(decoded.has_permission(AccessPermission::Edit));
        assert!(decoded.allows_data_class(DataClass::CustomerData));
    }

    #[test]
    fn scoped_grant_models_explicit_executive_global_access() {
        let ceo = PrincipalRef::human_user("ceo-user").with_tenant_actor_id("actor-ceo");
        let org = ResourceRef::new("acme", "*", ResourceKind::Organization, "acme");
        let grant = ScopedGrant::new("grant-ceo-global", ceo, org, GrantSource::ExecutiveGlobal)
            .with_permissions(vec![
                AccessPermission::View,
                AccessPermission::Read,
                AccessPermission::Admin,
            ])
            .with_data_classes(vec![
                DataClass::Internal,
                DataClass::Confidential,
                DataClass::Restricted,
                DataClass::Executive,
                DataClass::FinancialRecord,
            ]);

        assert_eq!(grant.grant_source, GrantSource::ExecutiveGlobal);
        assert_eq!(grant.resource.resource_kind, ResourceKind::Organization);
        assert_eq!(grant.resource.workspace_id, "*");
        assert!(grant.has_permission(AccessPermission::Admin));
        assert!(grant.allows_data_class(DataClass::Executive));
    }

    #[test]
    fn scoped_grant_models_down_scoped_delegation_with_expiry() {
        let delegate = PrincipalRef::new(PrincipalKind::ExternalDelegate, "vendor-agent")
            .with_issuer_subject("a2a://vendor.example", "vendor-agent-7");
        let delegator = PrincipalRef::human_user("user-legal");
        let contract_branch =
            ResourceRef::new("acme", "legal", ResourceKind::Document, "vendor-contract")
                .with_project_id("vendor-review")
                .with_path_prefix("/contracts/vendor-a/");
        let grant = ScopedGrant::new(
            "grant-vendor-contract-read",
            delegate,
            contract_branch,
            GrantSource::Delegation,
        )
        .with_source_principal(delegator)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::Confidential])
        .with_tool_patterns(vec!["mcp:google-drive:files.get".to_string()])
        .with_delegation_id("delegation-123")
        .with_expires_at_ms(2_000);

        assert_eq!(grant.grant_source, GrantSource::Delegation);
        assert_eq!(grant.delegation_id.as_deref(), Some("delegation-123"));
        assert!(!grant.is_expired_at(1_999));
        assert!(grant.is_expired_at(2_000));
        assert_eq!(grant.tool_patterns, vec!["mcp:google-drive:files.get"]);

        let encoded = serde_json::to_value(&grant).expect("serialize delegation grant");
        assert_eq!(encoded["principal"]["kind"], "external_delegate");
        assert_eq!(encoded["grant_source"], "delegation");
        assert_eq!(encoded["delegation_id"], "delegation-123");
    }
}
