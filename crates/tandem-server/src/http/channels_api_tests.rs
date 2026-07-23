// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[test]
fn public_demo_sanitizes_enabled_tools_and_mcp_servers() {
    let prefs = ChannelToolPreferences {
        enabled_tools: vec![
            "websearch".to_string(),
            WORKFLOW_PLANNER_PSEUDO_TOOL.to_string(),
            "bash".to_string(),
            "webfetch_html".to_string(),
            "bash".to_string(),
        ],
        disabled_tools: vec![
            "read".to_string(),
            WORKFLOW_PLANNER_PSEUDO_TOOL.to_string(),
            "read".to_string(),
        ],
        enabled_mcp_servers: vec!["github".to_string(), "slack".to_string()],
        enabled_mcp_tools: vec![
            "mcp.github.create_issue".to_string(),
            "mcp.slack.post_message".to_string(),
        ],
    };

    let sanitized = sanitize_tool_preferences_for_security_profile(
        prefs,
        tandem_channels::config::ChannelSecurityProfile::PublicDemo,
    );

    assert_eq!(
        sanitized.enabled_tools,
        vec!["websearch".to_string(), "webfetch_html".to_string()]
    );
    assert_eq!(sanitized.disabled_tools, vec!["read".to_string()]);
    assert!(sanitized.enabled_mcp_servers.is_empty());
    assert!(sanitized.enabled_mcp_tools.is_empty());
}

#[test]
fn operator_keeps_existing_tool_preferences() {
    let prefs = ChannelToolPreferences {
        enabled_tools: vec!["bash".to_string(), "bash".to_string()],
        disabled_tools: vec!["read".to_string(), "".to_string()],
        enabled_mcp_servers: vec!["github".to_string(), "github".to_string()],
        enabled_mcp_tools: vec![
            "mcp.github.create_issue".to_string(),
            "mcp.github.create_issue".to_string(),
        ],
    };

    let sanitized = sanitize_tool_preferences_for_security_profile(
        prefs,
        tandem_channels::config::ChannelSecurityProfile::Operator,
    );

    assert_eq!(sanitized.enabled_tools, vec!["bash".to_string()]);
    assert_eq!(sanitized.disabled_tools, vec!["read".to_string()]);
    assert_eq!(sanitized.enabled_mcp_servers, vec!["github".to_string()]);
    assert_eq!(
        sanitized.enabled_mcp_tools,
        vec!["mcp.github.create_issue".to_string()]
    );
}

#[test]
fn operator_drops_exact_mcp_tools_when_server_is_disabled() {
    let prefs = ChannelToolPreferences {
        enabled_tools: vec!["read".to_string()],
        disabled_tools: Vec::new(),
        enabled_mcp_servers: vec!["notion".to_string()],
        enabled_mcp_tools: vec![
            "mcp.github.create_issue".to_string(),
            "mcp.notion.search".to_string(),
        ],
    };

    let sanitized = sanitize_tool_preferences_for_security_profile(
        prefs,
        tandem_channels::config::ChannelSecurityProfile::Operator,
    );

    assert_eq!(sanitized.enabled_mcp_servers, vec!["notion".to_string()]);
    assert_eq!(
        sanitized.enabled_mcp_tools,
        vec!["mcp.notion.search".to_string()]
    );
}

#[test]
fn group_channel_scope_summaries_groups_by_scope_and_orders_by_recency() {
    let mut map = HashMap::new();
    map.insert(
        "telegram:chat:123:alice".to_string(),
        ChannelSessionRecord {
            session_id: "s1".to_string(),
            created_at_ms: 1,
            last_seen_at_ms: 10,
            channel: "telegram".to_string(),
            sender: "alice".to_string(),
            scope_id: Some("chat:123".to_string()),
            scope_kind: Some("room".to_string()),
            tool_preferences: None,
            workflow_planner_session_id: None,
        },
    );
    map.insert(
        "telegram:chat:123:bob".to_string(),
        ChannelSessionRecord {
            session_id: "s2".to_string(),
            created_at_ms: 2,
            last_seen_at_ms: 30,
            channel: "telegram".to_string(),
            sender: "bob".to_string(),
            scope_id: Some("chat:123".to_string()),
            scope_kind: Some("room".to_string()),
            tool_preferences: None,
            workflow_planner_session_id: None,
        },
    );
    map.insert(
        "telegram:topic:1:2:carol".to_string(),
        ChannelSessionRecord {
            session_id: "s3".to_string(),
            created_at_ms: 3,
            last_seen_at_ms: 20,
            channel: "telegram".to_string(),
            sender: "carol".to_string(),
            scope_id: Some("topic:1:2".to_string()),
            scope_kind: Some("topic".to_string()),
            tool_preferences: None,
            workflow_planner_session_id: None,
        },
    );
    map.insert(
        "discord:channel:9:dave".to_string(),
        ChannelSessionRecord {
            session_id: "s4".to_string(),
            created_at_ms: 4,
            last_seen_at_ms: 40,
            channel: "discord".to_string(),
            sender: "dave".to_string(),
            scope_id: Some("channel:9".to_string()),
            scope_kind: Some("room".to_string()),
            tool_preferences: None,
            workflow_planner_session_id: None,
        },
    );

    let scopes = group_channel_scope_summaries("telegram", &map);
    assert_eq!(scopes.len(), 2);
    assert_eq!(scopes[0].scope_id, "chat:123");
    assert_eq!(scopes[0].session_count, 2);
    assert_eq!(scopes[0].sender_count, 2);
    assert_eq!(scopes[0].last_seen_at_ms, 30);
    assert_eq!(scopes[1].scope_id, "topic:1:2");
    assert_eq!(scopes[1].session_count, 1);
    assert_eq!(scopes[1].sender_count, 1);
}

#[test]
fn merge_channel_tool_preferences_layers_scope_over_base() {
    let base = ChannelToolPreferences {
        enabled_tools: vec!["read".to_string(), "grep".to_string()],
        disabled_tools: vec!["write".to_string()],
        enabled_mcp_servers: vec!["github".to_string()],
        enabled_mcp_tools: vec!["mcp.github.get_issue".to_string()],
    };
    let scoped = ChannelToolPreferences {
        enabled_tools: vec!["search".to_string(), "read".to_string()],
        disabled_tools: vec!["write".to_string(), "edit".to_string()],
        enabled_mcp_servers: vec!["notion".to_string(), "github".to_string()],
        enabled_mcp_tools: vec![
            "mcp.github.create_issue".to_string(),
            "mcp.notion.search_pages".to_string(),
        ],
    };

    let merged = merge_channel_tool_preferences(base, scoped);

    assert_eq!(
        merged.enabled_tools,
        vec!["read".to_string(), "grep".to_string(), "search".to_string()]
    );
    assert_eq!(
        merged.disabled_tools,
        vec!["write".to_string(), "edit".to_string()]
    );
    assert_eq!(
        merged.enabled_mcp_servers,
        vec!["github".to_string(), "notion".to_string()]
    );
    assert_eq!(
        merged.enabled_mcp_tools,
        vec![
            "mcp.github.get_issue".to_string(),
            "mcp.github.create_issue".to_string(),
            "mcp.notion.search_pages".to_string()
        ]
    );
}
