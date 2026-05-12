use tandem_types::{ToolDomain, ToolEffect, ToolSchema};

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
}

pub fn canonical_tool_name(name: &str) -> String {
    let lowered = name.trim().to_ascii_lowercase().replace('-', "_");
    let canonical = if let Some(stripped) = strip_known_namespace(&lowered) {
        stripped
    } else {
        lowered
    };
    match canonical.as_str() {
        "shell" | "powershell" | "cmd" | "run_command" => "bash".to_string(),
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        other => other.to_string(),
    }
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
            "glob" | "search" | "grep" | "codesearch" | "ls" | "list"
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
    }
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
        ToolCapabilityProfile::EmailDelivery
        | ToolCapabilityProfile::EmailSend
        | ToolCapabilityProfile::EmailDraft => false,
    }
}

fn strip_known_namespace(name: &str) -> Option<String> {
    const PREFIXES: [&str; 8] = [
        "default_api:",
        "default_api.",
        "functions.",
        "function.",
        "tools.",
        "tool.",
        "builtin:",
        "builtin.",
    ];
    for prefix in PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn tool_name_looks_like_email_delivery(tool_name: &str) -> bool {
    if tool_name_is_mcp_connector_tool(tool_name) {
        return false;
    }
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
    let tokens = tool_name_tokens(tool_name);
    let compact = tool_name_compact(tool_name);
    tool_name_looks_like_email_delivery(tool_name)
        && (tool_name_tokens_contains(&tokens, "send")
            || tool_name_tokens_contains(&tokens, "deliver")
            || tool_name_tokens_contains(&tokens, "reply")
            || compact.contains("sendemail")
            || compact.contains("emailsend")
            || compact.contains("replyemail")
            || compact.contains("emailreply"))
}

fn tool_name_looks_like_email_draft(tool_name: &str) -> bool {
    let tokens = tool_name_tokens(tool_name);
    let compact = tool_name_compact(tool_name);
    tool_name_looks_like_email_delivery(tool_name)
        && (tool_name_tokens_contains(&tokens, "draft")
            || tool_name_tokens_contains(&tokens, "compose")
            || compact.contains("draftemail")
            || compact.contains("emaildraft")
            || compact.contains("composeemail")
            || compact.contains("emailcompose"))
}

fn tool_name_is_mcp_connector_tool(tool_name: &str) -> bool {
    tool_name.trim().to_ascii_lowercase().starts_with("mcp.")
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
    use tandem_types::{ToolCapabilities, ToolDomain, ToolEffect};

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
    fn mcp_action_name_does_not_infer_email_delivery() {
        assert!(!tool_name_matches_profile(
            "mcp.reddit_gmail.gmail_send_email",
            ToolCapabilityProfile::EmailSend
        ));
    }
}
