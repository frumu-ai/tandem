pub mod approval_receipt;
pub mod authority;
pub mod authorization_hook;
pub mod cross_tenant;
mod delegation;
pub use delegation::*;
pub mod governance;

pub use approval_receipt::*;
pub use authorization_hook::*;
pub use cross_tenant::*;

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
    OrganizationUnit,
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
    ConnectorInstance,
    SourceBinding,
    SourceObject,
    IngestionJob,
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

    pub fn applies_to(&self, target: &ResourceRef) -> bool {
        if self.organization_id != target.organization_id {
            return false;
        }
        if self.workspace_id != "*" && self.workspace_id != target.workspace_id {
            return false;
        }

        match self.resource_kind {
            ResourceKind::Organization => self.resource_id == target.organization_id,
            ResourceKind::Workspace | ResourceKind::Department => {
                self.resource_id == target.workspace_id
                    || self.resource_id == "*"
                    || self.resource_id == target.resource_id
            }
            ResourceKind::Project => {
                target.project_id.as_deref() == Some(self.resource_id.as_str())
                    || target.resource_id == self.resource_id
            }
            _ => self.matches_resource_or_path(target),
        }
    }

    fn matches_resource_or_path(&self, target: &ResourceRef) -> bool {
        if self.project_id.is_some() && self.project_id != target.project_id {
            return false;
        }
        if self.resource_kind == target.resource_kind && self.resource_id == target.resource_id {
            return self.path_prefix_applies_to(target);
        }
        self.path_prefix
            .as_deref()
            .zip(target.path_prefix.as_deref())
            .map(|(prefix, target_prefix)| target_prefix.starts_with(prefix))
            .unwrap_or(false)
    }

    fn path_prefix_applies_to(&self, target: &ResourceRef) -> bool {
        match (self.path_prefix.as_deref(), target.path_prefix.as_deref()) {
            (Some(prefix), Some(target_prefix)) => target_prefix.starts_with(prefix),
            (Some(_), None) => false,
            (None, _) => true,
        }
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

    pub fn explicitly_denies(&self, resource: &ResourceRef) -> bool {
        self.denied_resources
            .iter()
            .any(|denied| denied.applies_to(resource))
    }

    pub fn contains(&self, resource: &ResourceRef) -> bool {
        !self.explicitly_denies(resource)
            && (self.root.applies_to(resource)
                || self
                    .allowed_resources
                    .iter()
                    .any(|allowed| allowed.applies_to(resource)))
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
    OrganizationUnit,
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

    pub fn organization_unit(id: impl Into<String>) -> Self {
        Self::new(PrincipalKind::OrganizationUnit, id)
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
    OrganizationUnitMembership,
    GroupMembership,
    DepartmentMembership,
    Inherited,
    ExecutiveGlobal,
    Delegation,
    CrossTenantGrant,
    BreakGlass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccessEffect {
    #[default]
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedGrant {
    pub grant_id: String,
    pub principal: PrincipalRef,
    pub resource: ResourceRef,
    #[serde(default)]
    pub effect: AccessEffect,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationUnitKind {
    Department,
    Team,
    RoleDomain,
    ContractorGroup,
    ExecutiveGroup,
    ClinicalGroup,
    OperationalGroup,
    Custom,
    #[default]
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationUnitState {
    #[default]
    Active,
    Disabled,
}

impl OrganizationUnitState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationUnit {
    pub unit_id: String,
    pub tenant_context: TenantContext,
    #[serde(default = "default_taxonomy_id")]
    pub taxonomy_id: String,
    pub display_name: String,
    #[serde(default)]
    pub kind: OrganizationUnitKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_unit_id: Option<String>,
    #[serde(default)]
    pub state: OrganizationUnitState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    pub created_by: PrincipalRef,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl OrganizationUnit {
    pub fn active(
        unit_id: impl Into<String>,
        tenant_context: TenantContext,
        display_name: impl Into<String>,
        kind: OrganizationUnitKind,
        created_by: PrincipalRef,
        now_ms: u64,
    ) -> Self {
        Self {
            unit_id: unit_id.into(),
            tenant_context,
            taxonomy_id: default_taxonomy_id(),
            display_name: display_name.into(),
            kind,
            parent_unit_id: None,
            state: OrganizationUnitState::Active,
            description: None,
            labels: Vec::new(),
            created_by,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    pub fn with_parent_unit_id(mut self, parent_unit_id: impl Into<String>) -> Self {
        self.parent_unit_id = Some(parent_unit_id.into());
        self
    }

    pub fn with_taxonomy_id(mut self, taxonomy_id: impl Into<String>) -> Self {
        self.taxonomy_id = taxonomy_id.into();
        self
    }

    pub fn with_state(mut self, state: OrganizationUnitState, updated_at_ms: u64) -> Self {
        self.state = state;
        self.updated_at_ms = updated_at_ms;
        self
    }

    pub fn principal_ref(&self) -> PrincipalRef {
        PrincipalRef::organization_unit(format!("{}/{}", self.taxonomy_id, self.unit_id))
    }

    pub fn resource_ref(&self) -> ResourceRef {
        ResourceRef::new(
            self.tenant_context.org_id.clone(),
            self.tenant_context.workspace_id.clone(),
            ResourceKind::OrganizationUnit,
            format!("{}/{}", self.taxonomy_id, self.unit_id),
        )
    }
}

fn default_taxonomy_id() -> String {
    "organization_unit".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationUnitMembershipSource {
    #[default]
    Direct,
    HostedControlPlane,
    Scim,
    GoogleWorkspace,
    Okta,
    ManualImport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationUnitMembership {
    pub membership_id: String,
    pub tenant_context: TenantContext,
    pub unit: PrincipalRef,
    pub member: PrincipalRef,
    #[serde(default)]
    pub source: OrganizationUnitMembershipSource,
    #[serde(default)]
    pub state: OrganizationUnitState,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

impl OrganizationUnitMembership {
    pub fn active(
        membership_id: impl Into<String>,
        tenant_context: TenantContext,
        unit: PrincipalRef,
        member: PrincipalRef,
        source: OrganizationUnitMembershipSource,
        created_at_ms: u64,
    ) -> Self {
        Self {
            membership_id: membership_id.into(),
            tenant_context,
            unit,
            member,
            source,
            state: OrganizationUnitState::Active,
            created_at_ms,
            expires_at_ms: None,
        }
    }

    pub fn with_expires_at_ms(mut self, expires_at_ms: u64) -> Self {
        self.expires_at_ms = Some(expires_at_ms);
        self
    }

    pub fn is_active_at(&self, now_ms: u64) -> bool {
        self.state.is_active()
            && self
                .expires_at_ms
                .map(|expires_at_ms| expires_at_ms > now_ms)
                .unwrap_or(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationUnitAccessGrant {
    pub grant_id: String,
    pub tenant_context: TenantContext,
    pub unit: PrincipalRef,
    pub resource: ResourceRef,
    #[serde(default)]
    pub effect: AccessEffect,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<AccessPermission>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_patterns: Vec<String>,
    #[serde(default)]
    pub state: OrganizationUnitState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

impl OrganizationUnitAccessGrant {
    pub fn active(
        grant_id: impl Into<String>,
        tenant_context: TenantContext,
        unit: PrincipalRef,
        resource: ResourceRef,
        created_at_ms: u64,
    ) -> Self {
        Self {
            grant_id: grant_id.into(),
            tenant_context,
            unit,
            resource,
            effect: AccessEffect::Allow,
            permissions: Vec::new(),
            data_classes: Vec::new(),
            tool_patterns: Vec::new(),
            state: OrganizationUnitState::Active,
            created_at_ms,
            updated_at_ms: created_at_ms,
            expires_at_ms: None,
        }
    }

    pub fn with_effect(mut self, effect: AccessEffect) -> Self {
        self.effect = effect;
        self
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

    pub fn with_expires_at_ms(mut self, expires_at_ms: u64) -> Self {
        self.expires_at_ms = Some(expires_at_ms);
        self
    }

    pub fn is_active_at(&self, now_ms: u64) -> bool {
        self.state.is_active()
            && self
                .expires_at_ms
                .map(|expires_at_ms| expires_at_ms > now_ms)
                .unwrap_or(true)
    }

    pub fn to_scoped_grant_for_membership(
        &self,
        membership: &OrganizationUnitMembership,
        now_ms: u64,
    ) -> Option<ScopedGrant> {
        if self.tenant_context.org_id != membership.tenant_context.org_id
            || self.tenant_context.workspace_id != membership.tenant_context.workspace_id
            || self.tenant_context.deployment_id != membership.tenant_context.deployment_id
            || self.unit != membership.unit
            || !self.is_active_at(now_ms)
            || !membership.is_active_at(now_ms)
        {
            return None;
        }

        let mut grant = ScopedGrant::new(
            format!("{}::{}", membership.membership_id, self.grant_id),
            membership.member.clone(),
            self.resource.clone(),
            GrantSource::OrganizationUnitMembership,
        )
        .with_effect(self.effect)
        .with_permissions(self.permissions.clone())
        .with_data_classes(self.data_classes.clone())
        .with_tool_patterns(self.tool_patterns.clone())
        .with_source_principal(self.unit.clone());

        grant.expires_at_ms = match (membership.expires_at_ms, self.expires_at_ms) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        };
        Some(grant)
    }
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
            effect: AccessEffect::Allow,
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

    pub fn with_effect(mut self, effect: AccessEffect) -> Self {
        self.effect = effect;
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

    pub fn applies_to(
        &self,
        resource: &ResourceRef,
        permission: AccessPermission,
        data_class: DataClass,
        now_ms: u64,
    ) -> bool {
        !self.is_expired_at(now_ms)
            && self.has_permission(permission)
            && self.allows_data_class(data_class)
            && self.resource.applies_to(resource)
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub org_units: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_projection: Option<StrictTenantContext>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub org_units: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<PrincipalRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_scope: Option<ResourceScope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<ScopedGrant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_boundary: Option<DataBoundary>,
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
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            principal: None,
            resource_scope: None,
            grants: Vec::new(),
            data_boundary: None,
        }
    }

    pub fn is_expired_at(&self, now_ms: u64) -> bool {
        self.expires_at_ms <= now_ms
    }

    pub fn with_strict_projection(
        mut self,
        principal: PrincipalRef,
        resource_scope: ResourceScope,
        grants: Vec<ScopedGrant>,
        data_boundary: DataBoundary,
    ) -> Self {
        self.principal = Some(principal);
        self.resource_scope = Some(resource_scope);
        self.grants = grants;
        self.data_boundary = Some(data_boundary);
        self
    }

    pub fn has_strict_projection(&self) -> bool {
        self.principal.is_some()
            || self.resource_scope.is_some()
            || !self.grants.is_empty()
            || self.data_boundary.is_some()
    }

    pub fn strict_projection(&self) -> Option<StrictTenantContext> {
        Some(
            StrictTenantContext::new(
                self.tenant_context.clone(),
                self.principal.clone()?,
                self.authority_chain.clone(),
                self.resource_scope.clone()?,
                AssertionMetadata::from(self),
            )
            .with_grants(self.grants.clone())
            .with_data_boundary(self.data_boundary.clone().unwrap_or_default()),
        )
    }
}

impl From<&TenantContextAssertionClaims> for AssertionMetadata {
    fn from(claims: &TenantContextAssertionClaims) -> Self {
        Self {
            issuer: claims.issuer.clone(),
            audience: claims.audience.clone(),
            issued_at_ms: claims.issued_at_ms,
            expires_at_ms: claims.expires_at_ms,
            assertion_id: claims.assertion_id.clone(),
            key_id: None,
            purpose: Some(SigningKeyPurpose::ContextAssertion),
        }
    }
}

impl From<TenantContextAssertionClaims> for VerifiedTenantContext {
    fn from(claims: TenantContextAssertionClaims) -> Self {
        let strict_projection = claims.strict_projection();
        Self {
            tenant_context: claims.tenant_context,
            human_actor: claims.human_actor,
            authority_chain: claims.authority_chain,
            roles: claims.roles,
            org_units: claims.org_units,
            capabilities: claims.capabilities,
            policy_version: claims.policy_version,
            strict_projection,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundary {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_data_classes: Vec<DataClass>,
}

impl DataBoundary {
    pub fn unrestricted() -> Self {
        Self {
            allowed_data_classes: Vec::new(),
            denied_data_classes: Vec::new(),
        }
    }

    pub fn allow(data_classes: Vec<DataClass>) -> Self {
        Self {
            allowed_data_classes: data_classes,
            denied_data_classes: Vec::new(),
        }
    }

    pub fn allows(&self, data_class: DataClass) -> bool {
        if self.denied_data_classes.contains(&data_class) {
            return false;
        }
        self.allowed_data_classes.is_empty() || self.allowed_data_classes.contains(&data_class)
    }
}

impl Default for DataBoundary {
    fn default() -> Self {
        Self::unrestricted()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningKeyPurpose {
    ContextAssertion,
    ApprovalReceipt,
    DelegationProjection,
    CrossTenantGrant,
    A2aPeerAssertion,
    BreakGlassAdminAssertion,
}

impl SigningKeyPurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ContextAssertion => "context_assertion",
            Self::ApprovalReceipt => "approval_receipt",
            Self::DelegationProjection => "delegation_projection",
            Self::CrossTenantGrant => "cross_tenant_grant",
            Self::A2aPeerAssertion => "a2a_peer_assertion",
            Self::BreakGlassAdminAssertion => "break_glass_admin_assertion",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ParseSigningKeyPurposeError> {
        value.parse()
    }
}

impl core::str::FromStr for SigningKeyPurpose {
    type Err = ParseSigningKeyPurposeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "context_assertion"
            | "context-assertion"
            | "tenant_context_assertion"
            | "tenant-context-assertion" => Ok(Self::ContextAssertion),
            "approval_receipt" | "approval-receipt" => Ok(Self::ApprovalReceipt),
            "delegation_projection" | "delegation-projection" => Ok(Self::DelegationProjection),
            "cross_tenant_grant" | "cross-tenant-grant" => Ok(Self::CrossTenantGrant),
            "a2a_peer_assertion"
            | "a2a-peer-assertion"
            | "agent2agent_peer_assertion"
            | "agent2agent-peer-assertion" => Ok(Self::A2aPeerAssertion),
            "break_glass_admin_assertion"
            | "break-glass-admin-assertion"
            | "break_glass_admin"
            | "break-glass-admin" => Ok(Self::BreakGlassAdminAssertion),
            _ => Err(ParseSigningKeyPurposeError),
        }
    }
}

impl core::fmt::Display for SigningKeyPurpose {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseSigningKeyPurposeError;

impl core::fmt::Display for ParseSigningKeyPurposeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("invalid signing key purpose")
    }
}

impl std::error::Error for ParseSigningKeyPurposeError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertionMetadata {
    pub issuer: String,
    pub audience: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub assertion_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<SigningKeyPurpose>,
}

impl AssertionMetadata {
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        issued_at_ms: u64,
        expires_at_ms: u64,
        assertion_id: impl Into<String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            audience: audience.into(),
            issued_at_ms,
            expires_at_ms,
            assertion_id: assertion_id.into(),
            key_id: None,
            purpose: None,
        }
    }

    pub fn with_key_id(mut self, key_id: impl Into<String>) -> Self {
        self.key_id = Some(key_id.into());
        self
    }

    pub fn with_purpose(mut self, purpose: SigningKeyPurpose) -> Self {
        self.purpose = Some(purpose);
        self
    }

    pub fn is_expired_at(&self, now_ms: u64) -> bool {
        self.expires_at_ms <= now_ms
    }
}

impl From<&VerifiedTenantContext> for AssertionMetadata {
    fn from(context: &VerifiedTenantContext) -> Self {
        Self {
            issuer: context.issuer.clone(),
            audience: context.audience.clone(),
            issued_at_ms: context.issued_at_ms,
            expires_at_ms: context.expires_at_ms,
            assertion_id: context.assertion_id.clone(),
            key_id: None,
            purpose: Some(SigningKeyPurpose::ContextAssertion),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrictTenantContext {
    pub tenant_context: TenantContext,
    pub principal: PrincipalRef,
    pub authority_chain: AuthorityChain,
    pub resource_scope: ResourceScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<ScopedGrant>,
    #[serde(default)]
    pub data_boundary: DataBoundary,
    pub assertion: AssertionMetadata,
    /// Remaining re-delegation hops when this context was produced by a
    /// delegation projection (EAA-08). `None` means the context is a root
    /// (non-delegated) context; `Some(0)` means it cannot delegate further.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_delegation_depth: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessDecision {
    Allow,
    Deny,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantEvaluation {
    pub decision: AccessDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    pub reason: String,
}

impl GrantEvaluation {
    pub fn allow(grant_id: impl Into<String>) -> Self {
        Self {
            decision: AccessDecision::Allow,
            grant_id: Some(grant_id.into()),
            reason: "matching_allow_grant".to_string(),
        }
    }

    pub fn deny(reason: impl Into<String>, grant_id: Option<String>) -> Self {
        Self {
            decision: AccessDecision::Deny,
            grant_id,
            reason: reason.into(),
        }
    }

    pub fn not_applicable(reason: impl Into<String>) -> Self {
        Self {
            decision: AccessDecision::NotApplicable,
            grant_id: None,
            reason: reason.into(),
        }
    }
}

impl StrictTenantContext {
    pub fn new(
        tenant_context: TenantContext,
        principal: PrincipalRef,
        authority_chain: AuthorityChain,
        resource_scope: ResourceScope,
        assertion: AssertionMetadata,
    ) -> Self {
        Self {
            tenant_context,
            principal,
            authority_chain,
            resource_scope,
            grants: Vec::new(),
            data_boundary: DataBoundary::default(),
            assertion,
            remaining_delegation_depth: None,
        }
    }

    pub fn with_remaining_delegation_depth(mut self, depth: u32) -> Self {
        self.remaining_delegation_depth = Some(depth);
        self
    }

    pub fn with_grants(mut self, grants: Vec<ScopedGrant>) -> Self {
        self.grants = grants;
        self
    }

    pub fn with_data_boundary(mut self, data_boundary: DataBoundary) -> Self {
        self.data_boundary = data_boundary;
        self
    }

    pub fn is_expired_at(&self, now_ms: u64) -> bool {
        self.assertion.is_expired_at(now_ms)
    }

    pub fn allows_data_class(&self, data_class: DataClass) -> bool {
        self.data_boundary.allows(data_class)
            && self
                .grants
                .iter()
                .any(|grant| grant.allows_data_class(data_class))
    }

    pub fn has_permission(&self, permission: AccessPermission) -> bool {
        self.grants
            .iter()
            .any(|grant| grant.has_permission(permission))
    }

    pub fn evaluate_access(
        &self,
        resource: &ResourceRef,
        permission: AccessPermission,
        data_class: DataClass,
        now_ms: u64,
    ) -> GrantEvaluation {
        if self.is_expired_at(now_ms) {
            return GrantEvaluation::deny("context_expired", None);
        }
        if !self.data_boundary.allows(data_class) {
            return GrantEvaluation::deny("data_class_denied_by_boundary", None);
        }
        if self.resource_scope.explicitly_denies(resource) {
            return GrantEvaluation::deny("resource_explicitly_denied_by_scope", None);
        }

        if let Some(grant) = self.grants.iter().find(|grant| {
            grant.effect == AccessEffect::Deny
                && grant.applies_to(resource, permission, data_class, now_ms)
        }) {
            return GrantEvaluation::deny("matching_deny_grant", Some(grant.grant_id.clone()));
        }

        if !self.resource_scope.contains(resource) {
            return GrantEvaluation::not_applicable("resource_outside_projected_scope");
        }

        if let Some(grant) = self.grants.iter().find(|grant| {
            grant.effect == AccessEffect::Allow
                && grant.applies_to(resource, permission, data_class, now_ms)
        }) {
            return GrantEvaluation::allow(grant.grant_id.clone());
        }

        GrantEvaluation::not_applicable("no_matching_allow_grant")
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorLifecycleState {
    #[default]
    Active,
    Paused,
    Revoked,
    Quarantined,
}

impl ConnectorLifecycleState {
    pub fn allows_ingestion(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCredentialClass {
    #[default]
    ReadOnly,
    ReadWrite,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorCredentialRef {
    pub org_id: String,
    pub workspace_id: String,
    pub connector_id: String,
    pub credential_id: String,
    #[serde(default)]
    pub credential_class: ConnectorCredentialClass,
    pub secret_ref: SecretRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_bound_resource: Option<ResourceRef>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotated_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

impl ConnectorCredentialRef {
    pub fn read_only(
        org_id: impl Into<String>,
        workspace_id: impl Into<String>,
        connector_id: impl Into<String>,
        credential_id: impl Into<String>,
        secret_ref: SecretRef,
        created_at_ms: u64,
    ) -> Self {
        Self {
            org_id: org_id.into(),
            workspace_id: workspace_id.into(),
            connector_id: connector_id.into(),
            credential_id: credential_id.into(),
            credential_class: ConnectorCredentialClass::ReadOnly,
            secret_ref,
            source_bound_resource: None,
            created_at_ms,
            rotated_at_ms: None,
            expires_at_ms: None,
        }
    }

    pub fn with_source_bound_resource(mut self, resource: ResourceRef) -> Self {
        self.source_bound_resource = Some(resource);
        self
    }

    pub fn validate_for_tenant(&self, ctx: &TenantContext) -> Result<(), SecretRefError> {
        if self.org_id != ctx.org_id {
            return Err(SecretRefError::OrgMismatch);
        }
        if self.workspace_id != ctx.workspace_id {
            return Err(SecretRefError::WorkspaceMismatch);
        }
        self.secret_ref.validate_for_tenant(ctx)
    }
}

include!("lib_sources.rs");

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

include!("lib_tests.rs");
