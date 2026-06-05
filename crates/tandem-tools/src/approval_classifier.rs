//! Approval-classification table for tools and capability identifiers.
//!
//! The compiler-side gate-injection pass calls into this module to decide
//! whether a workflow node that uses a given tool/capability should be wrapped
//! in a `HumanApprovalGate` by default. The decision is made *purely from the
//! tool/capability ID*: the classifier is intentionally pure and table-driven
//! so it can be unit-tested exhaustively and so callers can override per-tool
//! at scope-review time without round-tripping through the runtime.
//!
//! # Categories that gate by default
//!
//! These are the actions the founder pitch promises will never run silently:
//!
//! - **Outbound communications**: email send, SMS, public chat post.
//! - **CRM writes**: HubSpot/Salesforce contacts, deals, activities.
//! - **Payments**: Stripe charges, refunds, payouts.
//! - **File deletes outside scratch**: `rm` paths outside the workspace
//!   scratch directory.
//! - **Public posts**: LinkedIn, Twitter, blog publish.
//! - **Calendar invites** sent to external addresses.
//! - **System-of-record writes**: Notion page mutations, Linear/Jira issue
//!   transitions, GitHub merges.
//!
//! # Categories that never gate
//!
//! Read-only operations and search (web search, file read, MCP read-only
//! tools) are `NoApproval`. They never gate even when an `*` allowlist
//! includes them, so a research-only workflow stays fast.
//!
//! # Default for unknown tools
//!
//! Unknown tools — including unrecognized MCP server tools — return
//! `UserConfigurable`. The compiler treats `UserConfigurable` as
//! `RequiresApproval` *when* the surrounding node already has any other
//! gating tool (deny-by-default for the doubt case). When the only tools in
//! a node are read-only or already classified `NoApproval`, an unknown tool
//! does NOT add a gate. This balances safety against demo-runnability for
//! workflows that mix obviously safe tools with obscure custom MCP tools.

use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalClassification {
    /// Action mutates a system of record, sends outbound communication, or
    /// performs another high-stakes side effect. The compiler injects a
    /// `HumanApprovalGate` on any node whose allowlist includes one.
    RequiresApproval,
    /// Read-only or low-stakes; the compiler does not inject a gate purely
    /// because of this tool. Other tools in the same node may still gate.
    NoApproval,
    /// Unknown or operator-overridable. Treated as `RequiresApproval` when
    /// any sibling tool already gates; otherwise `NoApproval`. Operators can
    /// pin the result via scope-review override.
    UserConfigurable,
}

/// Classify a single tool/capability identifier.
///
/// Identifiers are matched exactly against the built-in allow/deny tables
/// first, then by namespace prefix (e.g. `mcp.stripe.*`, `mcp.salesforce.*`),
/// then by suffix heuristics for common verbs (`*.send`, `*.create`,
/// `*.publish`, `*.delete`).
pub fn classify(tool_id: &str) -> ApprovalClassification {
    let id = tool_id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return ApprovalClassification::UserConfigurable;
    }

    if always_gates_table().contains(id.as_str()) {
        return ApprovalClassification::RequiresApproval;
    }
    if never_gates_table().contains(id.as_str()) {
        return ApprovalClassification::NoApproval;
    }

    // Read-prefix exceptions must run before broad write-prefix gates like
    // `mcp.linear.` so issue-tracker read tools do not accidentally require
    // human approval.
    for prefix in NEVER_GATE_PREFIXES {
        if id.starts_with(prefix) {
            return ApprovalClassification::NoApproval;
        }
    }

    // Namespace prefix gates: every tool under these MCP-server prefixes
    // mutates a system of record by default unless a read-prefix exception
    // above matched first.
    for prefix in ALWAYS_GATE_PREFIXES {
        if id.starts_with(prefix) {
            return ApprovalClassification::RequiresApproval;
        }
    }

    // Suffix heuristics: most tools that end in send/publish/post/delete
    // exist precisely to perform an external mutation.
    for suffix in ALWAYS_GATE_SUFFIXES {
        if id.ends_with(suffix) {
            return ApprovalClassification::RequiresApproval;
        }
    }

    ApprovalClassification::UserConfigurable
}

/// Classify the *aggregate* of a node's tool allowlist, which is what the
/// compiler injection pass actually consults.
///
/// Returns `RequiresApproval` if any tool requires approval, OR if at least
/// one tool requires approval and any other is `UserConfigurable`. Returns
/// `NoApproval` only when every tool in the allowlist is `NoApproval`.
pub fn classify_node_allowlist<I>(allowlist: I) -> ApprovalClassification
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut has_unknown = false;
    let mut saw_any = false;

    for tool in allowlist {
        saw_any = true;
        match classify(tool.as_ref()) {
            ApprovalClassification::RequiresApproval => {
                return ApprovalClassification::RequiresApproval
            }
            ApprovalClassification::UserConfigurable => has_unknown = true,
            ApprovalClassification::NoApproval => {}
        }
    }

    if !saw_any {
        // An empty allowlist means "no tools claimed yet" — the compiler will
        // re-classify after binding. Treat as configurable for now.
        return ApprovalClassification::UserConfigurable;
    }

    // The early-return inside the loop already handled `RequiresApproval`,
    // so by this point only `NoApproval` and `UserConfigurable` remain.
    if has_unknown {
        // A wildcard or unknown tool alongside otherwise-safe tools is the
        // "I don't know enough to be safe" case — fail closed.
        ApprovalClassification::UserConfigurable
    } else {
        ApprovalClassification::NoApproval
    }
}

/// Wildcard allowlists (`*` or `mcp.*`) always require approval. The whole
/// point of an unconstrained allowlist is that we can't reason about it.
pub fn allowlist_is_wildcard<I>(allowlist: I) -> bool
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    allowlist
        .into_iter()
        .any(|item| matches!(item.as_ref().trim(), "*" | "**" | "mcp.*"))
}

// =============================================================================
// Built-in classification tables
// =============================================================================
//
// These are the canonical tools Tandem ships. New tools belong in one of these
// tables; mass-classifying via prefix is a fallback for unknown MCP servers.

/// Tools that are always safe to invoke without approval.
const NEVER_GATES_LIST: &[&str] = &[
    // Built-in read tools.
    "read",
    "glob",
    "grep",
    "list_directory",
    "list_files",
    // Search and retrieval.
    "websearch",
    "web_search",
    "fetch_url",
    "memory_search",
    "memory_get",
    // Status / introspection.
    "list_sessions",
    "describe_workflow",
    "describe_run",
    // Knowledge-base reads.
    "kb_search",
    "kb_get_document",
    // Plan compilation / preview.
    "workflow_plan_preview",
    "workflow_plan_validate",
];

/// Tools that always require approval before execution.
const ALWAYS_GATES_LIST: &[&str] = &[
    // Built-in destructive filesystem.
    "rm",
    "delete",
    "unlink",
    "rmdir",
    "write",
    "edit",
    "patch",
    // Shell / process execution outside scratch.
    "bash",
    "shell",
    "exec",
    // Git mutations that touch upstream.
    "git_push",
    "git_force_push",
    // Email built-ins.
    "send_email",
    "send_mail",
    // Generic outbound built-ins.
    "publish",
    "post",
];

fn never_gates_table() -> &'static HashSet<&'static str> {
    static TABLE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    TABLE.get_or_init(|| NEVER_GATES_LIST.iter().copied().collect())
}

fn always_gates_table() -> &'static HashSet<&'static str> {
    static TABLE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    TABLE.get_or_init(|| ALWAYS_GATES_LIST.iter().copied().collect())
}

const ALWAYS_GATE_PREFIXES: &[&str] = &[
    // CRM writes.
    "mcp.hubspot.",
    "mcp.salesforce.",
    "mcp.pipedrive.",
    // Payments.
    "mcp.stripe.",
    "mcp.paypal.",
    // Outbound email.
    "mcp.gmail.send",
    "mcp.outlook.send",
    "mcp.sendgrid.",
    "mcp.mailgun.",
    // Public-post platforms.
    "mcp.linkedin.",
    "mcp.twitter.",
    "mcp.x.",
    "mcp.threads.",
    "mcp.bluesky.",
    // Calendar (sends invites).
    "mcp.googlecalendar.",
    "mcp.calendly.",
    // Issue trackers (transitions / merges affect SOR). Read-only verbs are
    // carved out in NEVER_GATE_PREFIXES and are checked before this list.
    "mcp.linear.",
    "mcp.app_linear_linear.",
    "mcp.jira.",
    "mcp.shortcut.",
    // GitHub mutating verbs.
    "mcp.github.create_pull_request",
    "mcp.github.merge_pull_request",
    "mcp.github.create_issue",
    "mcp.github.close_issue",
    "mcp.github.update_issue",
    // Notion / Confluence writes.
    "mcp.notion.create",
    "mcp.notion.update",
    "mcp.notion.delete",
    "mcp.confluence.create",
    "mcp.confluence.update",
    // Slack outbound (we send to non-internal channels by default).
    "mcp.slack.post",
    "mcp.slack.send",
    // Telegram / Discord outbound.
    "mcp.telegram.send",
    "mcp.discord.send",
    // Internal Tandem coder execution that touches branches/PRs.
    "coder.merge",
    "coder.publish",
];

const NEVER_GATE_PREFIXES: &[&str] = &[
    // GitHub read verbs.
    "mcp.github.list_",
    "mcp.github.get_",
    "mcp.github.search_",
    // Notion / Confluence reads.
    "mcp.notion.search",
    "mcp.notion.get",
    "mcp.confluence.search",
    "mcp.confluence.get",
    // Calendar reads.
    "mcp.googlecalendar.list",
    "mcp.googlecalendar.get",
    // Linear / Jira reads.
    "mcp.linear.list",
    "mcp.linear.get",
    "mcp.linear.search",
    "mcp.linear.viewer",
    "mcp.app_linear_linear.list",
    "mcp.app_linear_linear.get",
    "mcp.app_linear_linear.search",
    "mcp.app_linear_linear.viewer",
    "mcp.jira.search",
    "mcp.jira.get",
    // KB MCP reads.
    "mcp.kb.",
    "mcp.knowledge.",
    // Search providers.
    "mcp.brave.",
    "mcp.exa.",
    "mcp.searxng.",
    "mcp.serper.",
];

const ALWAYS_GATE_SUFFIXES: &[&str] = &[
    ".send",
    ".send_message",
    ".send_email",
    ".publish",
    ".post",
    ".create",
    ".update",
    ".delete",
    ".remove",
    ".merge",
    ".pay",
    ".charge",
    ".refund",
    ".transfer",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_built_ins_never_gate() {
        for tool in ["read", "glob", "grep", "websearch", "kb_search"] {
            assert_eq!(classify(tool), ApprovalClassification::NoApproval, "{tool}");
        }
    }

    #[test]
    fn destructive_built_ins_always_gate() {
        for tool in ["rm", "delete", "write", "edit", "bash", "send_email"] {
            assert_eq!(
                classify(tool),
                ApprovalClassification::RequiresApproval,
                "{tool}"
            );
        }
    }

    #[test]
    fn case_insensitive_matching() {
        assert_eq!(classify("READ"), ApprovalClassification::NoApproval);
        assert_eq!(
            classify("Send_Email"),
            ApprovalClassification::RequiresApproval
        );
    }

    #[test]
    fn empty_id_is_user_configurable() {
        assert_eq!(classify(""), ApprovalClassification::UserConfigurable);
        assert_eq!(classify("   "), ApprovalClassification::UserConfigurable);
    }

    #[test]
    fn crm_mcp_prefix_always_gates() {
        for tool in [
            "mcp.hubspot.create_contact",
            "mcp.salesforce.update_account",
            "mcp.pipedrive.add_deal",
        ] {
            assert_eq!(
                classify(tool),
                ApprovalClassification::RequiresApproval,
                "{tool}"
            );
        }
    }

    #[test]
    fn payment_mcp_prefix_always_gates() {
        assert_eq!(
            classify("mcp.stripe.create_charge"),
            ApprovalClassification::RequiresApproval
        );
        assert_eq!(
            classify("mcp.paypal.send_payment"),
            ApprovalClassification::RequiresApproval
        );
    }

    #[test]
    fn github_read_verbs_never_gate_but_writes_do() {
        assert_eq!(
            classify("mcp.github.list_issues"),
            ApprovalClassification::NoApproval
        );
        assert_eq!(
            classify("mcp.github.get_pull_request"),
            ApprovalClassification::NoApproval
        );
        assert_eq!(
            classify("mcp.github.create_pull_request"),
            ApprovalClassification::RequiresApproval
        );
        assert_eq!(
            classify("mcp.github.merge_pull_request"),
            ApprovalClassification::RequiresApproval
        );
    }

    #[test]
    fn linear_read_verbs_do_not_gate_but_writes_do() {
        for tool in [
            "mcp.linear.list_issues",
            "mcp.linear.list_comments",
            "mcp.linear.get_issue",
            "mcp.linear.search_documentation",
            "mcp.linear.viewer",
            "mcp.app_linear_linear.list_issues",
            "mcp.app_linear_linear.list_comments",
            "mcp.app_linear_linear.get_issue",
            "mcp.app_linear_linear.search_documentation",
            "mcp.app_linear_linear.viewer",
        ] {
            assert_eq!(classify(tool), ApprovalClassification::NoApproval, "{tool}");
        }
        for tool in [
            "mcp.linear.create_issue",
            "mcp.linear.update_issue",
            "mcp.linear.save_issue",
            "mcp.linear.create_comment",
            "mcp.linear.update_comment",
            "mcp.linear.transition_issue",
            "mcp.app_linear_linear.create_issue",
            "mcp.app_linear_linear.update_issue",
            "mcp.app_linear_linear.save_issue",
            "mcp.app_linear_linear.create_comment",
            "mcp.app_linear_linear.update_comment",
            "mcp.app_linear_linear.transition_issue",
        ] {
            assert_eq!(
                classify(tool),
                ApprovalClassification::RequiresApproval,
                "{tool}"
            );
        }
    }

    #[test]
    fn linear_read_exception_order_survives_broad_prefix_gate() {
        let allowlist = vec![
            "mcp.linear.list_issues".to_string(),
            "mcp.linear.get_issue".to_string(),
            "mcp.app_linear_linear.list_issues".to_string(),
            "mcp.app_linear_linear.get_issue".to_string(),
        ];
        assert_eq!(
            classify_node_allowlist(&allowlist),
            ApprovalClassification::NoApproval
        );
    }

    #[test]
    fn notion_read_vs_write() {
        assert_eq!(
            classify("mcp.notion.search"),
            ApprovalClassification::NoApproval
        );
        assert_eq!(
            classify("mcp.notion.get_page"),
            ApprovalClassification::NoApproval
        );
        assert_eq!(
            classify("mcp.notion.update_page"),
            ApprovalClassification::RequiresApproval
        );
        assert_eq!(
            classify("mcp.notion.delete_block"),
            ApprovalClassification::RequiresApproval
        );
    }

    #[test]
    fn suffix_heuristics_cover_unknown_servers() {
        assert_eq!(
            classify("mcp.unknown_vendor.send_message"),
            ApprovalClassification::RequiresApproval,
        );
        assert_eq!(
            classify("mcp.custom_app.publish"),
            ApprovalClassification::RequiresApproval,
        );
        assert_eq!(
            classify("mcp.workflow.delete"),
            ApprovalClassification::RequiresApproval,
        );
    }

    #[test]
    fn unknown_tool_is_user_configurable() {
        assert_eq!(
            classify("mcp.brand_new.do_something"),
            ApprovalClassification::UserConfigurable
        );
        assert_eq!(
            classify("custom_internal_tool"),
            ApprovalClassification::UserConfigurable
        );
    }

    #[test]
    fn search_provider_mcps_never_gate() {
        for tool in [
            "mcp.brave.search",
            "mcp.exa.search",
            "mcp.searxng.query",
            "mcp.serper.web",
        ] {
            assert_eq!(classify(tool), ApprovalClassification::NoApproval, "{tool}");
        }
    }

    #[test]
    fn classify_node_allowlist_returns_required_when_any_tool_requires() {
        let allowlist = vec![
            "read".to_string(),
            "websearch".to_string(),
            "mcp.hubspot.create_contact".to_string(),
        ];
        assert_eq!(
            classify_node_allowlist(&allowlist),
            ApprovalClassification::RequiresApproval
        );
    }

    #[test]
    fn classify_node_allowlist_returns_no_approval_for_pure_reads() {
        let allowlist = vec![
            "read".to_string(),
            "glob".to_string(),
            "websearch".to_string(),
            "mcp.github.list_issues".to_string(),
        ];
        assert_eq!(
            classify_node_allowlist(&allowlist),
            ApprovalClassification::NoApproval
        );
    }

    #[test]
    fn classify_node_allowlist_returns_user_configurable_when_unknown_present() {
        let allowlist = vec!["read".to_string(), "mcp.unknown_thing.do_X".to_string()];
        assert_eq!(
            classify_node_allowlist(&allowlist),
            ApprovalClassification::UserConfigurable
        );
    }

    #[test]
    fn classify_node_allowlist_required_dominates_unknown() {
        let allowlist = vec![
            "read".to_string(),
            "mcp.unknown_thing.do_X".to_string(),
            "send_email".to_string(),
        ];
        assert_eq!(
            classify_node_allowlist(&allowlist),
            ApprovalClassification::RequiresApproval
        );
    }

    #[test]
    fn classify_node_allowlist_empty_is_user_configurable() {
        let empty: Vec<String> = vec![];
        assert_eq!(
            classify_node_allowlist(&empty),
            ApprovalClassification::UserConfigurable
        );
    }

    #[test]
    fn allowlist_is_wildcard_detects_star() {
        assert!(allowlist_is_wildcard(&vec!["*".to_string()]));
        assert!(allowlist_is_wildcard(&vec!["**".to_string()]));
        assert!(allowlist_is_wildcard(&vec!["mcp.*".to_string()]));
        assert!(!allowlist_is_wildcard(&vec!["read".to_string()]));
        assert!(!allowlist_is_wildcard(&Vec::<String>::new()));
    }

    #[test]
    fn coder_merge_and_publish_always_gate() {
        assert_eq!(
            classify("coder.merge"),
            ApprovalClassification::RequiresApproval
        );
        assert_eq!(
            classify("coder.publish"),
            ApprovalClassification::RequiresApproval
        );
    }

    #[test]
    fn whitespace_is_trimmed() {
        assert_eq!(
            classify("  send_email  "),
            ApprovalClassification::RequiresApproval
        );
    }
}
