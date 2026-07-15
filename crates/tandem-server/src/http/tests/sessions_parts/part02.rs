// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_rewrites_explicitly_undefined_policy_answers() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"company-overview.md","content":"The knowledgebase does not define policy for crypto prize payouts, token rewards, or blockchain-based giveaways."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "What is the policy for crypto prize payouts?" }),
            },
            StrictKbProviderStep::Text(
                "Crypto prize payouts should avoid collecting wallet keys and require finance review."
                    .to_string(),
            ),
            StrictKbProviderStep::Text(
                json!({
                    "kb_answer_support": "supported",
                    "supported_facts": [
                        "The knowledgebase does not define policy for crypto prize payouts, token rewards, or blockchain-based giveaways."
                    ],
                    "missing_facts": [],
                    "sources": ["company-overview.md"],
                    "answer_text": "The policy is: do not offer or process crypto prize payouts. Northstar Events handles prize fulfillment through approved standard channels only, and any request for crypto payout should be declined/escalated according to internal event ops procedures."
                })
                .to_string(),
            ),
        ],
    )
    .await;
    let messages =
        run_prompt_sync_messages(state, "What is the policy for crypto prize payouts?", true).await;
    let assistant = latest_assistant_text(&messages);
    assert!(
        assistant.contains("I do not see a crypto prize payout policy"),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("Company Overview"));
    assert!(assistant.contains("does not define policy"));
    assert!(!assistant.to_ascii_lowercase().contains("wallet"));
    assert!(!assistant.to_ascii_lowercase().contains("private key"));
    assert!(!assistant.to_ascii_lowercase().contains("finance review"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("do not offer or process"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("approved standard channels"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("approved standard payout channels"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("declined/escalated"));
    assert!(!assistant.to_ascii_lowercase().contains("ops/finance"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("finance escalation"));
    assert!(!assistant.to_ascii_lowercase().contains("ops escalation"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("internal event ops procedures"));
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_blocks_generic_platform_instructions() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"discord-community-rules.md","content":"The bot may explain moderation policy, but must not ban, timeout, delete, or moderate users directly unless a future tool explicitly grants that capability. Moderators may delete spam, move conversations, warn users, or timeout users for up to 24 hours. Permanent bans require Mira Kovac approval during the event."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "Can you ban a Discord user who is spamming?" }),
            },
            StrictKbProviderStep::Text(
                "I cannot ban users directly, but you can right-click the user in Discord and choose Ban User from the moderation menu."
                    .to_string(),
            ),
            StrictKbProviderStep::Text(
                json!({
                    "kb_answer_support": "supported",
                    "supported_facts": [
                        "The bot may explain moderation policy, but must not ban, timeout, delete, or moderate users directly unless a future tool explicitly grants that capability.",
                        "Moderators may delete spam, move conversations, warn users, or timeout users for up to 24 hours.",
                        "Permanent bans require Mira Kovac approval during the event."
                    ],
                    "missing_facts": [],
                    "sources": ["discord-community-rules.md"],
                    "answer_text": "To ban the spammer in Discord, right-click the user, select Ban, choose whether to delete message history, and confirm."
                })
                .to_string(),
            ),
        ],
    )
    .await;
    let messages =
        run_prompt_sync_messages(state, "Can you ban a Discord user who is spamming?", true).await;
    let assistant = latest_assistant_text(&messages);
    assert!(
        assistant.contains("I cannot ban users from here."),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("Discord Community Rules"));
    assert!(assistant.contains("must not ban"));
    assert!(assistant.contains("timeout users for up to 24 hours"));
    assert!(assistant.contains("Mira Kovac"));
    assert!(!assistant.to_ascii_lowercase().contains("right-click"));
    assert!(!assistant.to_ascii_lowercase().contains("select ban"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("delete recent message history"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("delete message history"));
    assert!(!assistant.to_ascii_lowercase().contains("confirm the ban"));
    assert!(!assistant.to_ascii_lowercase().contains("moderation menu"));
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_wildcard_allowlist_still_forces_kb_policy() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"discord-community-rules.md","content":"The bot may only explain moderation policy, but must not ban, timeout, delete, or moderate users directly unless a future tool explicitly grants that capability. Moderators may delete spam, move conversations, warn users, or timeout users for up to 24 hours. Only Mira Kovac can approve permanent bans during the event."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "Can you ban a Discord user who is spamming?" }),
            },
            StrictKbProviderStep::Text(
                "I can’t directly ban a Discord user from here because I don’t have an active Discord moderation/admin connection. Right-click the user, select Ban, delete recent message history, and confirm."
                    .to_string(),
            ),
        ],
    )
    .await;
    let messages = run_prompt_sync_messages_with_allowlist(
        state,
        "Can you ban a Discord user who is spamming?",
        true,
        json!(["*"]),
    )
    .await;
    let assistant = latest_assistant_text(&messages);
    assert!(
        assistant.contains("I cannot ban users from here."),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("Discord Community Rules"));
    assert!(assistant.contains("must not ban"));
    assert!(!assistant.to_ascii_lowercase().contains("right-click"));
    assert!(!assistant.to_ascii_lowercase().contains("select ban"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("delete recent message history"));
    assert!(!assistant.to_ascii_lowercase().contains("confirm"));
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_repairs_provider_stream_decode_errors() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"company-overview.md","content":"Northstar Events is a demo event operations company for hosted knowledge-bot grounding tests."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "What is Northstar Events?" }),
            },
            StrictKbProviderStep::StreamError(
                "provider stream chunk error: error decoding response body".to_string(),
            ),
            StrictKbProviderStep::StreamError(
                "provider stream chunk error: error decoding response body".to_string(),
            ),
            StrictKbProviderStep::CompleteText(
                json!({
                    "kb_answer_support": "supported",
                    "supported_facts": [
                        "Northstar Events is a demo event operations company for hosted knowledge-bot grounding tests."
                    ],
                    "missing_facts": [],
                    "sources": ["company-overview.md"],
                    "answer_text": "Northstar Events is a demo event operations company for hosted knowledge-bot grounding tests."
                })
                .to_string(),
            ),
        ],
    )
    .await;
    let messages = run_prompt_sync_messages(state, "What is Northstar Events?", true).await;
    let assistant = latest_assistant_text(&messages);
    assert!(
        assistant.contains("I do not see that in the connected knowledgebase.")
            || assistant.contains(
                "Northstar Events is a demo event operations company for hosted knowledge-bot grounding tests."
            ),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("Source: Company Overview"));
    assert!(
        !assistant.contains("ENGINE_ERROR"),
        "assistant={}",
        assistant
    );
    assert!(
        !assistant
            .to_ascii_lowercase()
            .contains("provider stream chunk error"),
        "assistant={}",
        assistant
    );
    assert!(
        !assistant
            .to_ascii_lowercase()
            .contains("error decoding response body"),
        "assistant={}",
        assistant
    );
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_answers_supported_facts_with_sources() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"refund-and-billing-policy.md","content":"Refunds over €250 require Sofia Almeida approval."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "Who approves refunds over €250?" }),
            },
            StrictKbProviderStep::Text("Finance likely handles larger refunds.".to_string()),
            StrictKbProviderStep::Text(
                json!({
                    "kb_answer_support": "supported",
                    "supported_facts": ["Refunds over €250 require Sofia Almeida approval."],
                    "missing_facts": [],
                    "sources": ["refund-and-billing-policy.md"],
                    "answer_text": "Refunds over €250 require Sofia Almeida approval."
                })
                .to_string(),
            ),
        ],
    )
    .await;
    let messages = run_prompt_sync_messages(state, "Who approves refunds over €250?", true).await;
    let assistant = latest_assistant_text(&messages);
    assert!(
        assistant.contains("Refunds over €250 require Sofia Almeida approval."),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("Source: Refund And Billing Policy"));
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_preserves_sponsor_setup_times() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"northstar-events/sponsor-faq","content":"Sponsor booth setup starts at 08:30 local venue time on event day. Sponsors must finish booth setup by 10:15. Doors open at 10:30."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "What time does sponsor booth setup start, and when must it be finished?" }),
            },
            StrictKbProviderStep::Text(
                "Sponsor booth setup starts at 7:30 AM on March 14. It must be finished by 9:30 AM on March 14, before attendee registration opens."
                    .to_string(),
            ),
            StrictKbProviderStep::Text(
                json!({
                    "kb_answer_support": "supported",
                    "supported_facts": [
                        "Sponsor booth setup starts at 08:30 local venue time on event day.",
                        "Sponsors must finish booth setup by 10:15.",
                        "Doors open at 10:30."
                    ],
                    "missing_facts": [],
                    "sources": ["northstar-events/sponsor-faq"],
                    "answer_text": "Sponsor booth setup starts at 7:30 AM on March 14. It must be finished by 9:30 AM on March 14, before attendee registration opens."
                })
                .to_string(),
            ),
        ],
    )
    .await;
    let messages = run_prompt_sync_messages(
        state,
        "What time does sponsor booth setup start, and when must it be finished?",
        true,
    )
    .await;
    let assistant = latest_assistant_text(&messages);
    assert!(assistant.contains("08:30"), "assistant={}", assistant);
    assert!(assistant.contains("10:15"), "assistant={}", assistant);
    assert!(assistant.contains("10:30"), "assistant={}", assistant);
    assert!(
        assistant.contains("local venue time"),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("Source: Sponsor FAQ"));
    assert!(!assistant.contains("7:30"), "assistant={}", assistant);
    assert!(!assistant.contains("9:30"), "assistant={}", assistant);
    assert!(!assistant.contains("March 14"), "assistant={}", assistant);
    assert!(
        !assistant
            .to_ascii_lowercase()
            .contains("attendee registration"),
        "assistant={}",
        assistant
    );
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_rejects_unsupported_refund_approver() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"refund-and-billing-policy.md","content":"Refunds over €250 require Sofia Almeida approval."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "Who approves refunds over €250?" }),
            },
            StrictKbProviderStep::Text("Refunds over €250 require Sofia Almeida approval.".to_string()),
            StrictKbProviderStep::Text(
                json!({
                    "kb_answer_support": "supported",
                    "supported_facts": ["Refunds over €250 require Sofia Almeida approval."],
                    "missing_facts": [],
                    "sources": ["refund-and-billing-policy.md"],
                    "answer_text": "Refunds over €250 require Sofia Almeida approval, with backup approval from Bruno Costa."
                })
                .to_string(),
            ),
        ],
    )
    .await;
    let messages = run_prompt_sync_messages(state, "Who approves refunds over €250?", true).await;
    let assistant = latest_assistant_text(&messages);
    assert!(
        assistant.contains("Sofia Almeida"),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("€250"), "assistant={}", assistant);
    assert!(
        !assistant.contains("Bruno Costa"),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("Source: Refund And Billing Policy"));
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_keeps_partial_answers_bounded() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"staff-roles-and-contacts.md","content":"Mira Kovac is Event Director. Responsibilities: event escalation and moderator approvals. Demo email: mira@northstar.example. This demo knowledgebase does not contain real private phone numbers."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "What is Mira Kovac's phone number?" }),
            },
            StrictKbProviderStep::Text(
                "I do not have her phone number, but you can probably look it up in the company directory."
                    .to_string(),
            ),
            StrictKbProviderStep::Text(
                json!({
                    "kb_answer_support": "partial",
                    "supported_facts": [
                        "The staff doc lists Mira Kovac's role and demo email."
                    ],
                    "missing_facts": ["Mira Kovac's phone number"],
                    "sources": ["staff-roles-and-contacts.md"],
                    "answer_text": "I found Mira Kovac in the staff contacts document, but I don’t have the full phone number visible in the available result snippet."
                })
                .to_string(),
            ),
        ],
    )
    .await;
    let messages =
        run_prompt_sync_messages(state, "What is Mira Kovac's phone number?", true).await;
    let assistant = latest_assistant_text(&messages);
    assert!(
        assistant.contains("I do not see a phone number for Mira Kovac"),
        "assistant={}",
        assistant
    );
    assert!(assistant.contains("private phone numbers"));
    assert!(assistant.to_ascii_lowercase().contains("demo email"));
    assert!(!assistant.to_ascii_lowercase().contains("look it up"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("internal staff directory"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("designated ops escalation channel"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("not visible in the available result snippet"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("full phone number visible"));
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_handles_private_phone_fixture_without_inference() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"staff-roles-and-contacts.md","content":"This demo knowledgebase does not contain real private phone numbers."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "What is Mira Kovac's phone number?" }),
            },
            StrictKbProviderStep::Text(
                "Mira Kovac's phone number is not visible, but staff should use the approved internal staff directory or designated ops escalation channel."
                    .to_string(),
            ),
        ],
    )
    .await;
    let messages =
        run_prompt_sync_messages(state, "What is Mira Kovac's phone number?", true).await;
    let assistant = latest_assistant_text(&messages);
    assert!(
        assistant.contains("I do not see a phone number for Mira Kovac"),
        "assistant={}",
        assistant
    );
    assert!(
        assistant.contains("does not contain real private phone numbers"),
        "assistant={}",
        assistant
    );
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("internal staff directory"));
    assert!(!assistant
        .to_ascii_lowercase()
        .contains("designated ops escalation channel"));
    assert!(assistant.contains("Source: Staff Roles And Contacts"));
}

#[tokio::test]
async fn prompt_sync_strict_kb_grounding_falls_back_when_kb_has_no_results() {
    let state = strict_kb_test_state(
        r#"{"documents":[]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "What is Northstar Events?" }),
            },
            StrictKbProviderStep::Text(
                "Northstar Events is probably an event operations company that coordinates live productions."
                    .to_string(),
            ),
        ],
    )
    .await;
    let messages = run_prompt_sync_messages(state, "What is Northstar Events?", true).await;
    let assistant = latest_assistant_text(&messages);
    assert_eq!(
        assistant.trim(),
        "I do not see that in the connected knowledgebase."
    );
}

#[tokio::test]
async fn prompt_sync_without_strict_kb_grounding_preserves_existing_behavior() {
    let state = strict_kb_test_state(
        r#"{"documents":[{"relative_path":"refund-and-billing-policy.md","content":"Refunds over €250 require Sofia Almeida approval."}]}"#,
        vec![
            StrictKbProviderStep::ToolCall {
                tool: "mcp.kb.search_documents".to_string(),
                args: json!({ "query": "Who approves refunds over €250?" }),
            },
            StrictKbProviderStep::Text(
                "Refunds over €250 likely go through finance leadership and Sofia Almeida can help."
                    .to_string(),
            ),
        ],
    )
    .await;
    let messages = run_prompt_sync_messages(state, "Who approves refunds over €250?", false).await;
    let assistant = latest_assistant_text(&messages);
    assert!(assistant.contains("likely go through finance leadership"));
    assert!(!assistant.contains("Source:"));
}
