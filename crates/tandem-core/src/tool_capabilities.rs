use tandem_types::{
    AccessDecision, AccessPermission, DataClass, ResourceKind, ResourceRef, StrictTenantContext,
    ToolCapabilities, ToolDefaultVisibility, ToolDomain, ToolEffect, ToolRiskTier, ToolSchema,
    ToolSecurityDescriptor,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCapabilityProfile {
    WorkspaceRead,
    WorkspaceDiscover,
    ArtifactWrite,
    WebResearch,
    VerifyCommand,
    ShellExecution,
    MemoryOperation,
    EmailDelivery,
    EmailSend,
    EmailDraft,
    ExternalMutation,
}

pub fn tool_schema_security_descriptor(schema: &ToolSchema) -> ToolSecurityDescriptor {
    if !schema.security.is_empty() {
        return schema.security.clone();
    }
    tool_security_descriptor_from_name_and_capabilities(&schema.name, &schema.capabilities)
}

pub fn tool_name_security_descriptor(tool_name: &str) -> ToolSecurityDescriptor {
    tool_security_descriptor_from_name_and_capabilities(tool_name, &ToolCapabilities::default())
}

pub fn tool_schema_risk_tier(schema: &ToolSchema) -> ToolRiskTier {
    let descriptor = tool_schema_security_descriptor(schema);
    tool_risk_tier_from_name_and_descriptor(&schema.name, &descriptor)
}

pub fn tool_name_risk_tier(tool_name: &str) -> ToolRiskTier {
    let descriptor = tool_name_security_descriptor(tool_name);
    tool_risk_tier_from_name_and_descriptor(tool_name, &descriptor)
}

pub fn tool_risk_tier_from_name_and_descriptor(
    tool_name: &str,
    descriptor: &ToolSecurityDescriptor,
) -> ToolRiskTier {
    if let Some(risk_tier) = descriptor.risk_tier {
        return risk_tier;
    }
    if descriptor.admin_surface
        || descriptor.credential_access
        || descriptor
            .required_permissions
            .contains(&AccessPermission::Admin)
        || descriptor.data_classes.contains(&DataClass::Credential)
    {
        return ToolRiskTier::CredentialAdmin;
    }
    if tool_name_looks_like_money_movement_or_contract(tool_name) {
        return ToolRiskTier::MoneyMovementContract;
    }
    if tool_name_looks_like_destructive_action(tool_name) {
        return ToolRiskTier::DestructiveDelete;
    }
    if descriptor
        .data_classes
        .contains(&DataClass::FinancialRecord)
        || descriptor.data_classes.contains(&DataClass::Regulated)
    {
        return ToolRiskTier::FinancialRecordAccess;
    }
    if descriptor.data_classes.contains(&DataClass::CustomerData) {
        return ToolRiskTier::CustomerDataAccess;
    }
    if descriptor.data_classes.contains(&DataClass::SourceCode)
        && descriptor.required_permissions.iter().any(|permission| {
            matches!(
                permission,
                AccessPermission::Edit | AccessPermission::Execute | AccessPermission::Delegate
            )
        })
    {
        return ToolRiskTier::SourceCodeMutation;
    }
    if tool_name_matches_profile(tool_name, ToolCapabilityProfile::EmailSend)
        || tool_name_looks_like_external_send(tool_name)
    {
        return ToolRiskTier::ExternalSend;
    }
    if tool_name_matches_profile(tool_name, ToolCapabilityProfile::EmailDraft)
        || tool_name_looks_like_external_draft(tool_name)
    {
        return ToolRiskTier::ExternalDraft;
    }
    if descriptor.external_side_effect
        || descriptor.required_permissions.iter().any(|permission| {
            matches!(
                permission,
                AccessPermission::Edit | AccessPermission::Execute | AccessPermission::Delegate
            )
        })
    {
        return ToolRiskTier::InternalWrite;
    }
    ToolRiskTier::ReadDiscover
}

pub fn tool_schema_visible_to_strict_context(
    schema: &ToolSchema,
    strict_context: &StrictTenantContext,
    now_ms: u64,
) -> bool {
    let descriptor = tool_schema_security_descriptor(schema);
    tool_security_descriptor_visible_to_strict_context(
        &schema.name,
        &descriptor,
        strict_context,
        now_ms,
    )
}

pub fn tool_security_descriptor_visible_to_strict_context(
    tool_name: &str,
    descriptor: &ToolSecurityDescriptor,
    strict_context: &StrictTenantContext,
    now_ms: u64,
) -> bool {
    if descriptor.is_empty() {
        return true;
    }
    if strict_context.is_expired_at(now_ms) {
        return false;
    }

    let required_permissions = if descriptor.required_permissions.is_empty() {
        vec![AccessPermission::View]
    } else {
        descriptor.required_permissions.clone()
    };
    let data_classes = if descriptor.data_classes.is_empty() {
        vec![DataClass::Internal]
    } else {
        descriptor.data_classes.clone()
    };
    let resource_kinds = if descriptor.resource_kinds.is_empty() {
        vec![ResourceKind::McpTool]
    } else {
        descriptor.resource_kinds.clone()
    };

    let all_permissions_allowed = required_permissions.iter().all(|permission| {
        resource_kinds.iter().any(|resource_kind| {
            let resource = tool_resource_ref(strict_context, *resource_kind, tool_name);
            data_classes.iter().any(|data_class| {
                matches!(
                    strict_context
                        .evaluate_access(&resource, *permission, *data_class, now_ms)
                        .decision,
                    AccessDecision::Allow
                )
            })
        })
    });
    if all_permissions_allowed {
        return true;
    }

    let hidden_by_default = matches!(descriptor.default_visibility, ToolDefaultVisibility::Hidden)
        || descriptor.admin_surface
        || descriptor.credential_access;
    !hidden_by_default
        && required_permissions
            .iter()
            .all(|permission| matches!(permission, AccessPermission::View | AccessPermission::Read))
        && resource_kinds.iter().any(|resource_kind| {
            let resource = tool_resource_ref(strict_context, *resource_kind, tool_name);
            data_classes.iter().any(|data_class| {
                matches!(
                    strict_context
                        .evaluate_access(&resource, AccessPermission::Read, *data_class, now_ms)
                        .decision,
                    AccessDecision::Allow
                ) || matches!(
                    strict_context
                        .evaluate_access(&resource, AccessPermission::View, *data_class, now_ms)
                        .decision,
                    AccessDecision::Allow
                )
            })
        })
}

fn tool_resource_ref(
    strict_context: &StrictTenantContext,
    resource_kind: ResourceKind,
    tool_name: &str,
) -> ResourceRef {
    let resource_id = match resource_kind {
        ResourceKind::McpServer => mcp_server_segment_from_tool_name(tool_name),
        _ => tool_name.to_string(),
    };
    ResourceRef::new(
        strict_context.tenant_context.org_id.clone(),
        strict_context.tenant_context.workspace_id.clone(),
        resource_kind,
        resource_id,
    )
}

fn mcp_server_segment_from_tool_name(tool_name: &str) -> String {
    let mut parts = tool_name.split('.');
    match (parts.next(), parts.next()) {
        (Some("mcp"), Some(server)) if !server.trim().is_empty() => server.to_string(),
        _ => "mcp".to_string(),
    }
}

pub fn canonical_tool_name(name: &str) -> String {
    tandem_types::canonical_tool_name(name)
}

fn tool_security_descriptor_from_name_and_capabilities(
    tool_name: &str,
    capabilities: &ToolCapabilities,
) -> ToolSecurityDescriptor {
    if tool_name_looks_like_admin_or_credential_surface(tool_name) {
        return ToolSecurityDescriptor::new()
            .permission(AccessPermission::Admin)
            .permission(AccessPermission::Execute)
            .resource_kind(ResourceKind::McpServer)
            .resource_kind(ResourceKind::McpTool)
            .resource_kind(ResourceKind::SecretProviderCredential)
            .data_class(DataClass::Credential)
            .admin_surface()
            .credential_access()
            .external_side_effect()
            .hidden_by_default();
    }

    let schema = ToolSchema::new(tool_name, "", serde_json::json!({}))
        .with_capabilities(capabilities.clone());

    if tool_schema_matches_profile(&schema, ToolCapabilityProfile::ShellExecution) {
        return ToolSecurityDescriptor::new()
            .permission(AccessPermission::Execute)
            .resource_kind(ResourceKind::Directory)
            .resource_kind(ResourceKind::File)
            .resource_kind(ResourceKind::Repository)
            .data_class(DataClass::Internal)
            .data_class(DataClass::SourceCode)
            .external_side_effect();
    }

    if tool_schema_matches_profile(&schema, ToolCapabilityProfile::ArtifactWrite) {
        return ToolSecurityDescriptor::new()
            .permission(AccessPermission::Edit)
            .resource_kind(ResourceKind::Artifact)
            .resource_kind(ResourceKind::Directory)
            .resource_kind(ResourceKind::File)
            .resource_kind(ResourceKind::Repository)
            .data_class(DataClass::Internal)
            .data_class(DataClass::SourceCode);
    }

    if tool_schema_matches_profile(&schema, ToolCapabilityProfile::ExternalMutation)
        || tool_schema_matches_profile(&schema, ToolCapabilityProfile::EmailSend)
        || tool_schema_matches_profile(&schema, ToolCapabilityProfile::EmailDraft)
    {
        return ToolSecurityDescriptor::new()
            .permission(AccessPermission::Execute)
            .resource_kind(ResourceKind::ExternalIntegrationAccount)
            .resource_kind(ResourceKind::McpTool)
            .data_class(DataClass::Internal)
            .external_side_effect();
    }

    if tool_schema_matches_profile(&schema, ToolCapabilityProfile::WorkspaceRead)
        || tool_schema_matches_profile(&schema, ToolCapabilityProfile::WorkspaceDiscover)
    {
        return ToolSecurityDescriptor::new()
            .permission(AccessPermission::Read)
            .resource_kind(ResourceKind::Directory)
            .resource_kind(ResourceKind::File)
            .resource_kind(ResourceKind::Repository)
            .data_class(DataClass::Internal)
            .data_class(DataClass::SourceCode);
    }

    if tool_schema_matches_profile(&schema, ToolCapabilityProfile::MemoryOperation) {
        let permission = if tool_name_looks_like_external_mutation(tool_name) {
            AccessPermission::Edit
        } else {
            AccessPermission::Read
        };
        return ToolSecurityDescriptor::new()
            .permission(permission)
            .resource_kind(ResourceKind::MemorySpace)
            .resource_kind(ResourceKind::KnowledgeSpace)
            .data_class(DataClass::Internal)
            .data_class(DataClass::Confidential);
    }

    if tool_schema_matches_profile(&schema, ToolCapabilityProfile::WebResearch) {
        return ToolSecurityDescriptor::new()
            .permission(AccessPermission::View)
            .data_class(DataClass::Public);
    }

    ToolSecurityDescriptor::new()
}

pub fn tool_schema_matches_profile(schema: &ToolSchema, profile: ToolCapabilityProfile) -> bool {
    if tool_schema_matches_profile_from_metadata(schema, profile) {
        return true;
    }
    tool_name_matches_profile(&schema.name, profile)
}

pub fn tool_name_matches_profile(tool_name: &str, profile: ToolCapabilityProfile) -> bool {
    let normalized = canonical_tool_name(tool_name);
    match profile {
        ToolCapabilityProfile::WorkspaceRead => normalized == "read",
        ToolCapabilityProfile::WorkspaceDiscover => matches!(
            normalized.as_str(),
            "glob"
                | "search"
                | "grep"
                | "codesearch"
                | "ls"
                | "list"
                | "repo.context_bundle"
                | "repo.search"
                | "repo.symbol"
                | "repo.neighbors"
                | "repo.impact"
                | "repo.test_targets"
        ),
        ToolCapabilityProfile::ArtifactWrite => {
            matches!(normalized.as_str(), "write" | "edit" | "apply_patch")
        }
        ToolCapabilityProfile::WebResearch => {
            matches!(
                normalized.as_str(),
                "websearch" | "webfetch" | "webfetch_html"
            )
        }
        ToolCapabilityProfile::VerifyCommand | ToolCapabilityProfile::ShellExecution => {
            normalized == "bash"
        }
        ToolCapabilityProfile::MemoryOperation => matches!(
            normalized.as_str(),
            "memory_search" | "memory_store" | "memory_list" | "memory_delete"
        ),
        ToolCapabilityProfile::EmailDelivery => tool_name_looks_like_email_delivery(tool_name),
        ToolCapabilityProfile::EmailSend => tool_name_looks_like_email_send(tool_name),
        ToolCapabilityProfile::EmailDraft => tool_name_looks_like_email_draft(tool_name),
        ToolCapabilityProfile::ExternalMutation => {
            tool_name_looks_like_external_mutation(tool_name)
        }
    }
}

fn tool_name_looks_like_admin_or_credential_surface(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    let compact = tool_name_compact(tool_name);
    let action = mcp_tool_action_name(tool_name);

    let token_match = tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "admin"
                | "administrator"
                | "credential"
                | "credentials"
                | "secret"
                | "secrets"
                | "token"
                | "tokens"
                | "oauth"
                | "kms"
                | "key"
                | "keys"
        )
    });

    token_match
        || compact.contains("accesstoken")
        || compact.contains("refreshtoken")
        || compact.contains("secretref")
        || action.is_some_and(|action| tool_name_looks_like_admin_or_credential_surface(&action))
}

fn tool_schema_matches_profile_from_metadata(
    schema: &ToolSchema,
    profile: ToolCapabilityProfile,
) -> bool {
    let capabilities = &schema.capabilities;
    match profile {
        ToolCapabilityProfile::WorkspaceRead => {
            capabilities.reads_workspace
                && capabilities.domains.contains(&ToolDomain::Workspace)
                && capabilities.effects.contains(&ToolEffect::Read)
        }
        ToolCapabilityProfile::WorkspaceDiscover => {
            capabilities.reads_workspace
                && capabilities.preferred_for_discovery
                && capabilities.domains.contains(&ToolDomain::Workspace)
                && capabilities.effects.contains(&ToolEffect::Search)
        }
        ToolCapabilityProfile::ArtifactWrite => {
            capabilities.writes_workspace
                && capabilities.domains.contains(&ToolDomain::Workspace)
                && (capabilities.effects.contains(&ToolEffect::Write)
                    || capabilities.effects.contains(&ToolEffect::Patch)
                    || capabilities.effects.contains(&ToolEffect::Delete))
        }
        ToolCapabilityProfile::WebResearch => {
            capabilities.network_access
                && capabilities.domains.contains(&ToolDomain::Web)
                && (capabilities.effects.contains(&ToolEffect::Search)
                    || capabilities.effects.contains(&ToolEffect::Fetch))
        }
        ToolCapabilityProfile::VerifyCommand | ToolCapabilityProfile::ShellExecution => {
            capabilities.domains.contains(&ToolDomain::Shell)
                && capabilities.effects.contains(&ToolEffect::Execute)
        }
        ToolCapabilityProfile::MemoryOperation => {
            capabilities.domains.contains(&ToolDomain::Memory)
        }
        ToolCapabilityProfile::ExternalMutation => {
            capabilities.network_access
                && (capabilities.effects.contains(&ToolEffect::Write)
                    || capabilities.effects.contains(&ToolEffect::Patch)
                    || capabilities.effects.contains(&ToolEffect::Delete)
                    || capabilities.effects.contains(&ToolEffect::Execute))
        }
        ToolCapabilityProfile::EmailDelivery
        | ToolCapabilityProfile::EmailSend
        | ToolCapabilityProfile::EmailDraft => false,
    }
}

fn tool_name_looks_like_email_delivery(tool_name: &str) -> bool {
    if tool_name_is_mcp_connector_tool(tool_name) {
        return mcp_tool_action_name(tool_name).is_some_and(|action| {
            tool_name_looks_like_non_mcp_email_send(&action)
                || tool_name_looks_like_non_mcp_email_draft(&action)
        });
    }
    tool_name_looks_like_email_provider(tool_name)
}

fn tool_name_looks_like_email_provider(tool_name: &str) -> bool {
    tool_name_tokens(tool_name).iter().any(|token| {
        matches!(
            token.as_str(),
            "email"
                | "mail"
                | "gmail"
                | "outlook"
                | "smtp"
                | "imap"
                | "inbox"
                | "mailbox"
                | "mailer"
                | "exchange"
                | "sendgrid"
                | "mailgun"
                | "postmark"
                | "resend"
                | "ses"
        )
    })
}

fn tool_name_looks_like_email_send(tool_name: &str) -> bool {
    if tool_name_is_mcp_connector_tool(tool_name) {
        return mcp_tool_action_name(tool_name)
            .is_some_and(|action| tool_name_looks_like_non_mcp_email_send(&action));
    }
    tool_name_looks_like_non_mcp_email_send(tool_name)
}

fn tool_name_looks_like_non_mcp_email_send(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    let compact = tool_name_compact(tool_name);
    !tool_name_looks_like_email_read_or_settings(tool_name)
        && tool_name_looks_like_email_provider(tool_name)
        && (tool_name_tokens_contains(&tokens, "send")
            || tool_name_tokens_contains(&tokens, "deliver")
            || tool_name_tokens_contains(&tokens, "reply")
            || compact.contains("sendemail")
            || compact.contains("emailsend")
            || compact.contains("replyemail")
            || compact.contains("emailreply"))
}

fn tool_name_looks_like_email_draft(tool_name: &str) -> bool {
    if tool_name_is_mcp_connector_tool(tool_name) {
        return mcp_tool_action_name(tool_name)
            .is_some_and(|action| tool_name_looks_like_non_mcp_email_draft(&action));
    }
    tool_name_looks_like_non_mcp_email_draft(tool_name)
}

fn tool_name_looks_like_non_mcp_email_draft(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    let compact = tool_name_compact(tool_name);
    !tool_name_looks_like_email_read_or_settings(tool_name)
        && tool_name_looks_like_email_provider(tool_name)
        && !tool_name_looks_like_non_mcp_email_send(tool_name)
        && (tool_name_tokens_contains(&tokens, "draft")
            || tool_name_tokens_contains(&tokens, "compose")
            || compact.contains("draftemail")
            || compact.contains("emaildraft")
            || compact.contains("composeemail")
            || compact.contains("emailcompose"))
}

fn tool_name_looks_like_email_read_or_settings(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    ["settings", "imap", "pop", "fetch", "list", "get", "search"]
        .iter()
        .any(|needle| tool_name_tokens_contains(&tokens, needle))
}

fn tool_name_is_mcp_connector_tool(tool_name: &str) -> bool {
    tool_name.trim().to_ascii_lowercase().starts_with("mcp.")
}

fn tool_name_looks_like_external_mutation(tool_name: &str) -> bool {
    if tool_name_is_mcp_connector_tool(tool_name) {
        return mcp_tool_action_name(tool_name)
            .is_some_and(|action| tool_action_looks_like_external_mutation(&action));
    }
    tool_action_looks_like_external_mutation(tool_name)
}

fn tool_action_looks_like_external_mutation(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    let compact = tool_name_compact(tool_name);
    let has = |needle: &str| tool_name_tokens_contains(&tokens, needle);

    if [
        "get", "fetch", "list", "search", "retrieve", "read", "find", "query", "about", "top",
    ]
    .iter()
    .any(|needle| has(needle))
        && ![
            "create", "update", "delete", "send", "insert", "post", "publish", "write",
        ]
        .iter()
        .any(|needle| has(needle))
    {
        return false;
    }

    [
        "create",
        "update",
        "delete",
        "remove",
        "move",
        "duplicate",
        "send",
        "insert",
        "edit",
        "post",
        "publish",
        "comment",
        "upload",
        "append",
        "add",
        "patch",
        "write",
        "submit",
        "approve",
        "archive",
    ]
    .iter()
    .any(|needle| has(needle))
        || compact.contains("createpage")
        || compact.contains("createpages")
        || compact.contains("updatepage")
        || compact.contains("senddraft")
        || compact.contains("sendemail")
        || compact.contains("createdraft")
        || compact.contains("updatedraft")
}

fn tool_name_looks_like_external_send(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    ["send", "deliver", "reply", "post", "publish", "submit"]
        .iter()
        .any(|needle| tool_name_tokens_contains(&tokens, needle))
}

fn tool_name_looks_like_external_draft(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    ["draft", "compose", "prepare"]
        .iter()
        .any(|needle| tool_name_tokens_contains(&tokens, needle))
}

fn tool_name_looks_like_money_movement_or_contract(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    [
        "payment",
        "payout",
        "fund",
        "funds",
        "transfer",
        "wire",
        "ach",
        "transaction",
        "ledger",
        "refund",
        "reverse",
        "contract",
        "commitment",
        "invoice",
        "billing",
        "quote",
        "order",
    ]
    .iter()
    .any(|needle| tool_name_tokens_contains(&tokens, needle))
}

fn tool_name_looks_like_destructive_action(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    ["delete", "remove", "destroy", "wipe", "purge", "drop"]
        .iter()
        .any(|needle| tool_name_tokens_contains(&tokens, needle))
}

fn mcp_tool_action_name(tool_name: &str) -> Option<String> {
    let normalized = tool_name.trim().to_ascii_lowercase().replace('-', "_");
    normalized
        .strip_prefix("mcp.")
        .and_then(|rest| rest.rsplit('.').next())
        .map(str::trim)
        .filter(|action| !action.is_empty())
        .map(str::to_string)
}

fn tool_name_tokens(tool_name: &str) -> Vec<String> {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>()
}

fn tool_name_tokens_contains(tokens: &[String], needle: &str) -> bool {
    tokens.iter().any(|token| token == needle)
}

fn tool_name_compact(tool_name: &str) -> String {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tandem_types::{
        AccessPermission, AuthorityChain, DataBoundary, DataClass, GrantSource, PrincipalRef,
        RequestPrincipal, ResourceKind, ResourceRef, ResourceScope, ScopedGrant,
        StrictTenantContext, ToolCapabilities, ToolDomain, ToolEffect, ToolRiskTier,
        ToolSecurityDescriptor,
    };

    #[test]
    fn schema_metadata_overrides_unknown_name_for_workspace_read() {
        let schema = ToolSchema::new("workspace_inspector", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Read)
                .domain(ToolDomain::Workspace)
                .reads_workspace(),
        );

        assert!(tool_schema_matches_profile(
            &schema,
            ToolCapabilityProfile::WorkspaceRead
        ));
    }

    #[test]
    fn workspace_discover_metadata_requires_search_effect() {
        let schema = ToolSchema::new("custom_read", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Read)
                .domain(ToolDomain::Workspace)
                .reads_workspace()
                .preferred_for_discovery(),
        );

        assert!(!tool_schema_matches_profile(
            &schema,
            ToolCapabilityProfile::WorkspaceDiscover
        ));
    }

    #[test]
    fn schema_metadata_overrides_unknown_name_for_memory_operation() {
        // A memory tool annotated with the Memory domain is classified as a
        // memory operation from metadata even when its name does not match the
        // built-in `memory_*` name heuristics.
        let schema = ToolSchema::new("kb_recall", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Search)
                .domain(ToolDomain::Memory)
                .preferred_for_discovery(),
        );

        assert!(tool_schema_matches_profile(
            &schema,
            ToolCapabilityProfile::MemoryOperation
        ));
        // Name fallback alone would not classify this tool.
        assert!(!tool_name_matches_profile(
            "kb_recall",
            ToolCapabilityProfile::MemoryOperation
        ));

        let descriptor = tool_schema_security_descriptor(&schema);
        assert!(descriptor
            .resource_kinds
            .contains(&ResourceKind::MemorySpace));
        assert!(descriptor
            .required_permissions
            .contains(&AccessPermission::Read));
    }

    #[test]
    fn email_send_falls_back_to_name_heuristics() {
        assert!(tool_name_matches_profile(
            "gmail_send_email",
            ToolCapabilityProfile::EmailSend
        ));
    }

    #[test]
    fn mcp_server_name_does_not_make_reddit_tool_email_delivery() {
        assert!(!tool_name_matches_profile(
            "mcp.reddit_gmail.reddit_search_across_subreddits",
            ToolCapabilityProfile::EmailDelivery
        ));
    }

    #[test]
    fn mcp_action_name_can_satisfy_email_send_without_namespace_guessing() {
        assert!(tool_name_matches_profile(
            "mcp.reddit_gmail.gmail_send_email",
            ToolCapabilityProfile::EmailSend
        ));
        assert!(tool_name_matches_profile(
            "mcp.reddit_gmail.gmail_create_email_draft",
            ToolCapabilityProfile::EmailDraft
        ));
        assert!(tool_name_matches_profile(
            "mcp.poop.gmail_send_draft",
            ToolCapabilityProfile::EmailSend
        ));
        assert!(!tool_name_matches_profile(
            "mcp.poop.gmail_send_draft",
            ToolCapabilityProfile::EmailDraft
        ));
        assert!(tool_name_matches_profile(
            "mcp.reddit_gmail.gmail_send_email",
            ToolCapabilityProfile::EmailDelivery
        ));
        assert!(!tool_name_matches_profile(
            "mcp.reddit_gmail.gmail_settings_send_as_get",
            ToolCapabilityProfile::EmailSend
        ));
    }

    #[test]
    fn mcp_action_name_identifies_external_mutations_without_server_name() {
        assert!(tool_name_matches_profile(
            "mcp.poop.notion_create_pages",
            ToolCapabilityProfile::ExternalMutation
        ));
        assert!(tool_name_matches_profile(
            "mcp.anything.gmail_send_draft",
            ToolCapabilityProfile::ExternalMutation
        ));
        assert!(!tool_name_matches_profile(
            "mcp.anything.notion_fetch",
            ToolCapabilityProfile::ExternalMutation
        ));
        assert!(!tool_name_matches_profile(
            "mcp.anything.reddit_search_across_subreddits",
            ToolCapabilityProfile::ExternalMutation
        ));
    }

    #[test]
    fn tool_security_descriptor_prefers_explicit_schema_metadata() {
        let schema = ToolSchema::new("mcp.admin.rotate_credential", "", json!({})).with_security(
            ToolSecurityDescriptor::new()
                .permission(AccessPermission::Read)
                .resource_kind(ResourceKind::Document)
                .data_class(DataClass::Internal),
        );

        let descriptor = tool_schema_security_descriptor(&schema);

        assert_eq!(
            descriptor.required_permissions,
            vec![AccessPermission::Read]
        );
        assert!(!descriptor.admin_surface);
        assert!(!descriptor.credential_access);
    }

    #[test]
    fn tool_security_descriptor_marks_workspace_read_boundaries() {
        let schema = ToolSchema::new("workspace_inspector", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Read)
                .domain(ToolDomain::Workspace)
                .reads_workspace(),
        );

        let descriptor = tool_schema_security_descriptor(&schema);

        assert!(descriptor
            .required_permissions
            .contains(&AccessPermission::Read));
        assert!(descriptor.resource_kinds.contains(&ResourceKind::File));
        assert!(descriptor.data_classes.contains(&DataClass::SourceCode));
        assert!(!descriptor.external_side_effect);
    }

    #[test]
    fn tool_security_descriptor_marks_shell_as_execute_side_effect() {
        let descriptor = tool_name_security_descriptor("bash");

        assert!(descriptor
            .required_permissions
            .contains(&AccessPermission::Execute));
        assert!(descriptor
            .resource_kinds
            .contains(&ResourceKind::Repository));
        assert!(descriptor.external_side_effect);
        assert!(!descriptor.admin_surface);
    }

    #[test]
    fn tool_security_descriptor_hides_admin_credential_surfaces() {
        let descriptor = tool_name_security_descriptor("mcp.google_admin.rotate_credential");

        assert!(descriptor.admin_surface);
        assert!(descriptor.credential_access);
        assert!(descriptor
            .required_permissions
            .contains(&AccessPermission::Admin));
        assert!(descriptor.data_classes.contains(&DataClass::Credential));
        assert!(descriptor.resource_kinds.contains(&ResourceKind::McpTool));
        assert!(matches!(
            descriptor.default_visibility,
            tandem_types::ToolDefaultVisibility::Hidden
        ));
    }

    #[test]
    fn tool_risk_tier_maps_descriptors_to_canonical_taxonomy() {
        assert_eq!(
            tool_name_risk_tier("mcp.gmail.gmail_send_email"),
            ToolRiskTier::ExternalSend
        );
        assert_eq!(
            tool_name_risk_tier("mcp.gmail.gmail_create_email_draft"),
            ToolRiskTier::ExternalDraft
        );
        assert_eq!(
            tool_name_risk_tier("mcp.google_admin.rotate_credential"),
            ToolRiskTier::CredentialAdmin
        );
        assert_eq!(
            tool_name_risk_tier("mcp.billing.rotate_credential"),
            ToolRiskTier::CredentialAdmin
        );
        assert_eq!(
            tool_name_risk_tier("mcp.payment.rotate_key"),
            ToolRiskTier::CredentialAdmin
        );
        assert_eq!(
            tool_name_risk_tier("mcp.bank.release_funds"),
            ToolRiskTier::MoneyMovementContract
        );
        assert_eq!(tool_name_risk_tier("read"), ToolRiskTier::ReadDiscover);

        let descriptor = ToolSecurityDescriptor::new()
            .permission(AccessPermission::Read)
            .resource_kind(ResourceKind::McpTool)
            .data_class(DataClass::FinancialRecord);
        assert_eq!(
            tool_risk_tier_from_name_and_descriptor("mcp.accounting.list_records", &descriptor),
            ToolRiskTier::FinancialRecordAccess
        );
    }

    #[test]
    fn explicit_descriptor_risk_tier_overrides_inference() {
        let schema = ToolSchema::new("mcp.finance.prepare_customer_email", "", json!({}))
            .with_security(ToolSecurityDescriptor::new().risk_tier(ToolRiskTier::ExternalDraft));

        assert_eq!(tool_schema_risk_tier(&schema), ToolRiskTier::ExternalDraft);
    }

    #[test]
    fn strict_context_filters_provider_tool_schemas_by_descriptor() {
        let strict = strict_context_with_grant(
            vec![AccessPermission::View, AccessPermission::Read],
            vec![DataClass::Internal, DataClass::SourceCode],
        );
        let read_schema = ToolSchema::new("mcp.github.list_repository_issues", "", json!({}))
            .with_security(
                ToolSecurityDescriptor::new()
                    .permission(AccessPermission::Read)
                    .resource_kind(ResourceKind::McpTool)
                    .data_class(DataClass::SourceCode),
            );
        let execute_schema = ToolSchema::new("mcp.github.create_pull_request", "", json!({}))
            .with_security(
                ToolSecurityDescriptor::new()
                    .permission(AccessPermission::Execute)
                    .resource_kind(ResourceKind::McpTool)
                    .data_class(DataClass::SourceCode)
                    .external_side_effect(),
            );
        let hidden_schema = ToolSchema::new("mcp.admin.rotate_credential", "", json!({}))
            .with_security(
                ToolSecurityDescriptor::new()
                    .permission(AccessPermission::Admin)
                    .resource_kind(ResourceKind::SecretProviderCredential)
                    .data_class(DataClass::Credential)
                    .admin_surface()
                    .credential_access()
                    .hidden_by_default(),
            );

        assert!(tool_schema_visible_to_strict_context(
            &read_schema,
            &strict,
            2_000
        ));
        assert!(!tool_schema_visible_to_strict_context(
            &execute_schema,
            &strict,
            2_000
        ));
        assert!(!tool_schema_visible_to_strict_context(
            &hidden_schema,
            &strict,
            2_000
        ));
    }

    fn strict_context_with_grant(
        permissions: Vec<AccessPermission>,
        data_classes: Vec<DataClass>,
    ) -> StrictTenantContext {
        let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
            "acme",
            "dev",
            Some("deployment-test".to_string()),
            "user-1",
        );
        let principal = PrincipalRef::human_user("user-1");
        let root = ResourceRef::new("acme", "*", ResourceKind::Organization, "acme");
        let grant = ScopedGrant::new(
            "grant-tool-filter",
            principal.clone(),
            root.clone(),
            GrantSource::Direct,
        )
        .with_permissions(permissions)
        .with_data_classes(data_classes.clone());

        StrictTenantContext::new(
            tenant_context,
            principal.clone(),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                principal.id,
                "test",
            )),
            ResourceScope::root(root),
            tandem_types::AssertionMetadata::new(
                "test",
                "tandem-runtime",
                1_000,
                9_999_999_999_999,
                "assertion-tool-filter",
            ),
        )
        .with_grants(vec![grant])
        .with_data_boundary(DataBoundary::allow(data_classes))
    }
}
