use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AccessPermission, DataClass, ResourceRef, TenantContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterprisePolicyScopeLevel {
    Enterprise,
    Tenant,
    OrgUnit,
    Workspace,
    Resource,
    Workflow,
    Phase,
}

impl EnterprisePolicyScopeLevel {
    pub fn inheritance_rank(self) -> u8 {
        match self {
            Self::Enterprise => 0,
            Self::Tenant => 1,
            Self::OrgUnit => 2,
            Self::Workspace => 3,
            Self::Resource => 4,
            Self::Workflow => 5,
            Self::Phase => 6,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enterprise => "enterprise",
            Self::Tenant => "tenant",
            Self::OrgUnit => "org_unit",
            Self::Workspace => "workspace",
            Self::Resource => "resource",
            Self::Workflow => "workflow",
            Self::Phase => "phase",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterprisePolicyEffect {
    Allow,
    Deny,
    ApprovalRequired,
}

impl EnterprisePolicyEffect {
    fn same_level_priority(self) -> u8 {
        match self {
            Self::Allow => 0,
            Self::ApprovalRequired => 1,
            Self::Deny => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::ApprovalRequired => "approval_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnterprisePolicyRule {
    pub rule_id: String,
    pub policy_id: String,
    pub version: u64,
    pub scope_level: EnterprisePolicyScopeLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_context: Option<TenantContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<AccessPermission>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_patterns: Vec<String>,
    pub effect: EnterprisePolicyEffect,
    pub reason_code: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    pub updated_at_ms: u64,
}

impl EnterprisePolicyRule {
    pub fn new(
        rule_id: impl Into<String>,
        policy_id: impl Into<String>,
        scope_level: EnterprisePolicyScopeLevel,
        effect: EnterprisePolicyEffect,
    ) -> Self {
        let effect_label = effect.as_str();
        Self {
            rule_id: rule_id.into(),
            policy_id: policy_id.into(),
            version: 1,
            scope_level,
            tenant_context: None,
            org_unit_id: None,
            resource: None,
            workflow_id: None,
            workflow_phase: None,
            permissions: Vec::new(),
            data_classes: Vec::new(),
            tool_patterns: Vec::new(),
            effect,
            reason_code: format!("policy_{effect_label}"),
            reason: format!("policy resolved to {effect_label}"),
            approval_id: None,
            updated_at_ms: 0,
        }
    }

    pub fn with_version(mut self, version: u64) -> Self {
        self.version = version;
        self
    }

    pub fn with_tenant_context(mut self, tenant_context: TenantContext) -> Self {
        self.tenant_context = Some(tenant_context);
        self
    }

    pub fn with_org_unit_id(mut self, org_unit_id: impl Into<String>) -> Self {
        self.org_unit_id = Some(org_unit_id.into());
        self
    }

    pub fn with_resource(mut self, resource: ResourceRef) -> Self {
        self.resource = Some(resource);
        self
    }

    pub fn with_workflow_id(mut self, workflow_id: impl Into<String>) -> Self {
        self.workflow_id = Some(workflow_id.into());
        self
    }

    pub fn with_workflow_phase(mut self, workflow_phase: impl Into<String>) -> Self {
        self.workflow_phase = Some(workflow_phase.into());
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

    pub fn with_reason(
        mut self,
        reason_code: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        self.reason_code = reason_code.into();
        self.reason = reason.into();
        self
    }

    pub fn with_approval_id(mut self, approval_id: impl Into<String>) -> Self {
        self.approval_id = Some(approval_id.into());
        self
    }

    pub fn with_updated_at_ms(mut self, updated_at_ms: u64) -> Self {
        self.updated_at_ms = updated_at_ms;
        self
    }

    fn matches(&self, input: &EnterprisePolicyInput) -> bool {
        self.matches_tenant(&input.tenant_context)
            && self.matches_org_unit(input.org_unit_id.as_deref())
            && self.matches_resource(input.resource.as_ref())
            && self.matches_workflow(input.workflow_id.as_deref())
            && self.matches_phase(input.workflow_phase.as_deref())
            && self.matches_permission(input.permission)
            && self.matches_data_class(input.data_class)
            && self.matches_tool(input.tool.as_deref())
    }

    fn matches_tenant(&self, tenant_context: &TenantContext) -> bool {
        let Some(rule_tenant) = &self.tenant_context else {
            return true;
        };
        rule_tenant.org_id == tenant_context.org_id
            && rule_tenant.workspace_id == tenant_context.workspace_id
            && rule_tenant.deployment_id == tenant_context.deployment_id
    }

    fn matches_org_unit(&self, org_unit_id: Option<&str>) -> bool {
        self.org_unit_id
            .as_deref()
            .map(|expected| org_unit_id == Some(expected))
            .unwrap_or(true)
    }

    fn matches_resource(&self, resource: Option<&ResourceRef>) -> bool {
        match (&self.resource, resource) {
            (Some(rule_resource), Some(resource)) => rule_resource.applies_to(resource),
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn matches_workflow(&self, workflow_id: Option<&str>) -> bool {
        self.workflow_id
            .as_deref()
            .map(|expected| workflow_id == Some(expected))
            .unwrap_or(true)
    }

    fn matches_phase(&self, workflow_phase: Option<&str>) -> bool {
        self.workflow_phase
            .as_deref()
            .map(|expected| workflow_phase == Some(expected))
            .unwrap_or(true)
    }

    fn matches_permission(&self, permission: Option<AccessPermission>) -> bool {
        self.permissions.is_empty()
            || permission.is_some_and(|permission| self.permissions.contains(&permission))
    }

    fn matches_data_class(&self, data_class: Option<DataClass>) -> bool {
        self.data_classes.is_empty()
            || data_class.is_some_and(|data_class| self.data_classes.contains(&data_class))
    }

    fn matches_tool(&self, tool: Option<&str>) -> bool {
        self.tool_patterns.is_empty()
            || tool.is_some_and(|tool| {
                self.tool_patterns
                    .iter()
                    .any(|pattern| tool_pattern_matches(pattern, tool))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnterprisePolicyInput {
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<AccessPermission>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_class: Option<DataClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

impl EnterprisePolicyInput {
    pub fn new(tenant_context: TenantContext) -> Self {
        Self {
            tenant_context,
            org_unit_id: None,
            resource: None,
            workflow_id: None,
            workflow_phase: None,
            permission: None,
            data_class: None,
            tool: None,
        }
    }

    pub fn with_org_unit_id(mut self, org_unit_id: impl Into<String>) -> Self {
        self.org_unit_id = Some(org_unit_id.into());
        self
    }

    pub fn with_resource(mut self, resource: ResourceRef) -> Self {
        self.resource = Some(resource);
        self
    }

    pub fn with_workflow_id(mut self, workflow_id: impl Into<String>) -> Self {
        self.workflow_id = Some(workflow_id.into());
        self
    }

    pub fn with_workflow_phase(mut self, workflow_phase: impl Into<String>) -> Self {
        self.workflow_phase = Some(workflow_phase.into());
        self
    }

    pub fn with_permission(mut self, permission: AccessPermission) -> Self {
        self.permission = Some(permission);
        self
    }

    pub fn with_data_class(mut self, data_class: DataClass) -> Self {
        self.data_class = Some(data_class);
        self
    }

    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectivePolicySource {
    pub rule_id: String,
    pub policy_id: String,
    pub version: u64,
    pub scope_level: EnterprisePolicyScopeLevel,
    pub effect: EnterprisePolicyEffect,
    pub reason_code: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
}

impl From<&EnterprisePolicyRule> for EffectivePolicySource {
    fn from(rule: &EnterprisePolicyRule) -> Self {
        Self {
            rule_id: rule.rule_id.clone(),
            policy_id: rule.policy_id.clone(),
            version: rule.version,
            scope_level: rule.scope_level,
            effect: rule.effect,
            reason_code: rule.reason_code.clone(),
            reason: rule.reason.clone(),
            approval_id: rule.approval_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectivePolicySnapshot {
    pub snapshot_id: String,
    pub policy_version_id: String,
    pub resolved_at_ms: u64,
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_phase: Option<String>,
    pub effect: EnterprisePolicyEffect,
    pub reason_code: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_source: Option<EffectivePolicySource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherited_sources: Vec<EffectivePolicySource>,
}

impl EffectivePolicySnapshot {
    pub fn single_source(
        tenant_context: TenantContext,
        resolved_at_ms: u64,
        source: EffectivePolicySource,
    ) -> Self {
        let effect = source.effect;
        let reason_code = source.reason_code.clone();
        let reason = source.reason.clone();
        let approval_id = source.approval_id.clone();
        Self::from_parts(
            tenant_context,
            resolved_at_ms,
            Some(source.clone()),
            vec![source],
            effect,
            reason_code,
            reason,
            approval_id,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        tenant_context: TenantContext,
        resolved_at_ms: u64,
        decision_source: Option<EffectivePolicySource>,
        inherited_sources: Vec<EffectivePolicySource>,
        effect: EnterprisePolicyEffect,
        reason_code: String,
        reason: String,
        approval_id: Option<String>,
        org_unit_id: Option<String>,
        workflow_id: Option<String>,
        workflow_phase: Option<String>,
    ) -> Self {
        let policy_version_id = policy_version_id_for_sources(&inherited_sources);
        let snapshot_id = format!(
            "effective_policy_{}",
            digest_hex(format!(
                "{}:{}:{}:{}",
                policy_version_id,
                tenant_context.org_id,
                tenant_context.workspace_id,
                resolved_at_ms
            ))
        );
        Self {
            snapshot_id,
            policy_version_id,
            resolved_at_ms,
            tenant_context,
            org_unit_id,
            workflow_id,
            workflow_phase,
            effect,
            reason_code,
            reason,
            approval_id,
            decision_source,
            inherited_sources,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnterprisePolicyResolver {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<EnterprisePolicyRule>,
}

impl EnterprisePolicyResolver {
    pub fn new(rules: Vec<EnterprisePolicyRule>) -> Self {
        Self { rules }
    }

    pub fn resolve(&self, input: &EnterprisePolicyInput, now_ms: u64) -> EffectivePolicySnapshot {
        let mut matching = self
            .rules
            .iter()
            .filter(|rule| rule.matches(input))
            .collect::<Vec<_>>();
        matching.sort_by_key(|rule| {
            (
                rule.scope_level.inheritance_rank(),
                rule.effect.same_level_priority(),
                rule.version,
                rule.updated_at_ms,
                rule.rule_id.clone(),
            )
        });
        let inherited_sources = matching
            .iter()
            .map(|rule| EffectivePolicySource::from(*rule))
            .collect::<Vec<_>>();
        let decision_source = matching
            .last()
            .map(|rule| EffectivePolicySource::from(*rule));

        let Some(source) = decision_source.clone() else {
            return EffectivePolicySnapshot::from_parts(
                input.tenant_context.clone(),
                now_ms,
                None,
                Vec::new(),
                EnterprisePolicyEffect::Deny,
                "enterprise_policy_no_matching_rule".to_string(),
                "no matching enterprise policy rule was found (fail closed)".to_string(),
                None,
                input.org_unit_id.clone(),
                input.workflow_id.clone(),
                input.workflow_phase.clone(),
            );
        };

        EffectivePolicySnapshot::from_parts(
            input.tenant_context.clone(),
            now_ms,
            Some(source.clone()),
            inherited_sources,
            source.effect,
            source.reason_code,
            source.reason,
            source.approval_id,
            input.org_unit_id.clone(),
            input.workflow_id.clone(),
            input.workflow_phase.clone(),
        )
    }
}

fn tool_pattern_matches(pattern: &str, tool: &str) -> bool {
    let pattern = pattern.trim();
    pattern == "*"
        || pattern == tool
        || pattern
            .strip_suffix(".*")
            .is_some_and(|prefix| tool.starts_with(&format!("{prefix}.")))
}

fn policy_version_id_for_sources(sources: &[EffectivePolicySource]) -> String {
    if sources.is_empty() {
        return "enterprise-policy-empty".to_string();
    }
    let encoded = serde_json::to_string(sources).unwrap_or_default();
    format!("enterprise-policy-{}", digest_hex(encoded))
}

fn digest_hex(input: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(input.as_ref());
    format!("{digest:x}")[..24].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResourceKind;

    fn tenant() -> TenantContext {
        TenantContext::explicit_user_workspace("acme", "finance", None, "user-finance")
    }

    fn ledger_resource() -> ResourceRef {
        ResourceRef::new("acme", "finance", ResourceKind::DataRoom, "ledger")
    }

    #[test]
    fn workspace_rule_overrides_enterprise_default_in_inheritance_order() {
        let input = EnterprisePolicyInput::new(tenant())
            .with_resource(ledger_resource())
            .with_permission(AccessPermission::Read)
            .with_data_class(DataClass::FinancialRecord);
        let resolver = EnterprisePolicyResolver::new(vec![
            EnterprisePolicyRule::new(
                "enterprise-default",
                "finance-policy",
                EnterprisePolicyScopeLevel::Enterprise,
                EnterprisePolicyEffect::Deny,
            )
            .with_permissions(vec![AccessPermission::Read])
            .with_reason(
                "enterprise_default_deny",
                "enterprise default denies finance reads",
            ),
            EnterprisePolicyRule::new(
                "workspace-finance",
                "finance-policy",
                EnterprisePolicyScopeLevel::Workspace,
                EnterprisePolicyEffect::Allow,
            )
            .with_tenant_context(tenant())
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::FinancialRecord])
            .with_reason(
                "workspace_finance_allow",
                "finance workspace can read ledger data",
            ),
        ]);

        let snapshot = resolver.resolve(&input, 1_000);

        assert_eq!(snapshot.effect, EnterprisePolicyEffect::Allow);
        assert_eq!(snapshot.reason_code, "workspace_finance_allow");
        assert_eq!(snapshot.inherited_sources.len(), 2);
        assert_eq!(
            snapshot
                .inherited_sources
                .iter()
                .map(|source| source.scope_level)
                .collect::<Vec<_>>(),
            vec![
                EnterprisePolicyScopeLevel::Enterprise,
                EnterprisePolicyScopeLevel::Workspace,
            ]
        );
        assert_eq!(
            snapshot
                .decision_source
                .as_ref()
                .map(|source| &source.rule_id),
            Some(&"workspace-finance".to_string())
        );
    }

    #[test]
    fn phase_deny_overrides_workflow_allow() {
        let input = EnterprisePolicyInput::new(tenant())
            .with_workflow_id("close-books")
            .with_workflow_phase("draft")
            .with_tool("mcp.erp.post_journal");
        let resolver = EnterprisePolicyResolver::new(vec![
            EnterprisePolicyRule::new(
                "workflow-close-books",
                "close-books-policy",
                EnterprisePolicyScopeLevel::Workflow,
                EnterprisePolicyEffect::Allow,
            )
            .with_workflow_id("close-books")
            .with_tool_patterns(vec!["mcp.erp.*".to_string()])
            .with_reason("workflow_tools_allowed", "workflow allows ERP tools"),
            EnterprisePolicyRule::new(
                "draft-posting-deny",
                "close-books-policy",
                EnterprisePolicyScopeLevel::Phase,
                EnterprisePolicyEffect::Deny,
            )
            .with_workflow_id("close-books")
            .with_workflow_phase("draft")
            .with_tool_patterns(vec!["mcp.erp.post_journal".to_string()])
            .with_reason(
                "draft_cannot_post_journals",
                "draft phase cannot post journals",
            ),
        ]);

        let snapshot = resolver.resolve(&input, 1_000);

        assert_eq!(snapshot.effect, EnterprisePolicyEffect::Deny);
        assert_eq!(snapshot.reason_code, "draft_cannot_post_journals");
        assert_eq!(
            snapshot
                .decision_source
                .as_ref()
                .map(|source| &source.rule_id),
            Some(&"draft-posting-deny".to_string())
        );
    }

    #[test]
    fn resource_approval_override_beats_tenant_allow() {
        let input = EnterprisePolicyInput::new(tenant())
            .with_resource(ledger_resource())
            .with_permission(AccessPermission::Execute)
            .with_data_class(DataClass::FinancialRecord);
        let resolver = EnterprisePolicyResolver::new(vec![
            EnterprisePolicyRule::new(
                "tenant-execute",
                "finance-policy",
                EnterprisePolicyScopeLevel::Tenant,
                EnterprisePolicyEffect::Allow,
            )
            .with_tenant_context(tenant())
            .with_permissions(vec![AccessPermission::Execute]),
            EnterprisePolicyRule::new(
                "ledger-approval",
                "finance-policy",
                EnterprisePolicyScopeLevel::Resource,
                EnterprisePolicyEffect::ApprovalRequired,
            )
            .with_resource(ledger_resource())
            .with_permissions(vec![AccessPermission::Execute])
            .with_data_classes(vec![DataClass::FinancialRecord])
            .with_approval_id("approval-ledger-execute")
            .with_reason(
                "ledger_execution_requires_approval",
                "ledger execution requires finance approval",
            ),
        ]);

        let snapshot = resolver.resolve(&input, 1_000);

        assert_eq!(snapshot.effect, EnterprisePolicyEffect::ApprovalRequired);
        assert_eq!(
            snapshot.approval_id.as_deref(),
            Some("approval-ledger-execute")
        );
        assert_eq!(snapshot.reason_code, "ledger_execution_requires_approval");
    }

    #[test]
    fn serialized_snapshot_preserves_replay_policy_after_rules_change() {
        let input = EnterprisePolicyInput::new(tenant()).with_tool("mcp.docs.search");
        let allow = EnterprisePolicyRule::new(
            "phase-docs-allow",
            "docs-policy",
            EnterprisePolicyScopeLevel::Phase,
            EnterprisePolicyEffect::Allow,
        )
        .with_workflow_phase("research")
        .with_tool_patterns(vec!["mcp.docs.*".to_string()])
        .with_reason("docs_allowed", "docs tools allowed during research");
        let denied = EnterprisePolicyRule::new(
            "phase-docs-deny-v2",
            "docs-policy",
            EnterprisePolicyScopeLevel::Phase,
            EnterprisePolicyEffect::Deny,
        )
        .with_workflow_phase("research")
        .with_tool_patterns(vec!["mcp.docs.*".to_string()])
        .with_version(2)
        .with_reason("docs_revoked", "docs tools were revoked");

        let old_snapshot = EnterprisePolicyResolver::new(vec![allow])
            .resolve(&input.clone().with_workflow_phase("research"), 1_000);
        let encoded = serde_json::to_value(&old_snapshot).expect("snapshot serializes");
        let replay_snapshot: EffectivePolicySnapshot =
            serde_json::from_value(encoded).expect("snapshot deserializes");
        let new_snapshot = EnterprisePolicyResolver::new(vec![denied])
            .resolve(&input.with_workflow_phase("research"), 2_000);

        assert_eq!(replay_snapshot.effect, EnterprisePolicyEffect::Allow);
        assert_eq!(replay_snapshot.reason_code, "docs_allowed");
        assert_eq!(new_snapshot.effect, EnterprisePolicyEffect::Deny);
        assert_ne!(
            replay_snapshot.policy_version_id,
            new_snapshot.policy_version_id
        );
    }
}
