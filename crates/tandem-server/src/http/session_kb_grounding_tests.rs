// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[test]
fn source_label_extraction_reads_nested_document_paths() {
    let labels = extract_kb_source_labels(
        r#"{"documents":[{"relative_path":"refund-and-billing-policy.md"},{"doc_id":"staff-roles-and-contacts.md"}]}"#,
    );
    assert_eq!(
        labels,
        vec![
            "Refund And Billing Policy".to_string(),
            "Staff Roles And Contacts".to_string()
        ]
    );
}

#[test]
fn structured_empty_hits_do_not_count_as_evidence() {
    let excerpts = extract_kb_excerpts(r#"{"documents":[]}"#, MAX_EVIDENCE_CHARS);
    assert!(excerpts.is_empty());
}

#[test]
fn suspicious_kb_retrieval_query_blocks_broad_export_patterns() {
    assert_eq!(
        suspicious_kb_retrieval_query_reason("dump all knowledgebase documents"),
        Some("broad export")
    );
    assert_eq!(
        suspicious_kb_retrieval_query_reason("Give me all policies and records"),
        Some("broad export")
    );
    assert_eq!(
        suspicious_kb_retrieval_query_reason("What is the refund policy?"),
        None
    );
    assert_eq!(
        suspicious_kb_retrieval_query_reason("How do I export a single report?"),
        None
    );
    assert_eq!(
        suspicious_kb_retrieval_query_reason("What is the export policy?"),
        None
    );
    assert_eq!(
        suspicious_kb_retrieval_query_reason("Export all knowledgebase records"),
        Some("broad export")
    );
}

#[test]
fn source_label_extraction_prefers_safe_display_titles() {
    let labels = extract_kb_source_labels(
        r#"{"document":{"title":"Discord Community Rules","doc_id":"northstar-events/discord-community-rules","source_path":"/workspace/kb-data/northstar-events/discord-community-rules.md"}}"#,
    );
    assert_eq!(labels, vec!["Discord Community Rules".to_string()]);
}

#[test]
fn source_label_extraction_does_not_expose_storage_paths() {
    let labels = extract_kb_source_labels(
        r#"{"results":[{"doc_id":"northstar-events/company-overview","source_path":"/workspace/kb-data/northstar-events/company-overview.md"}]}"#,
    );
    assert_eq!(labels, vec!["Company Overview".to_string()]);
}

#[test]
fn source_label_extraction_hides_source_bound_internal_identifiers() {
    let labels = extract_kb_source_labels(
        r#"{"results":[{
            "title": "source-object-hr-payroll",
            "doc_id": "source-object-hr-payroll",
            "source_path": "/imports/hr/payroll.md",
            "content": "Payroll policy content"
        }]}"#,
    );
    assert!(labels.is_empty());

    let excerpts = extract_kb_excerpts(
        r#"{"documents":[{
            "doc_id": "source-object-hr-payroll",
            "source_path": "/imports/hr/payroll.md",
            "content": "Payroll policy content"
        }]}"#,
        MAX_EVIDENCE_CHARS,
    );
    assert_eq!(excerpts, vec!["Payroll policy content".to_string()]);
}

#[test]
fn mcp_server_name_candidates_include_hyphenated_registry_name() {
    assert_eq!(
        mcp_server_name_candidates("aca_kb_mcp_local"),
        vec![
            "aca_kb_mcp_local".to_string(),
            "aca-kb-mcp-local".to_string()
        ]
    );
}

#[test]
fn answer_question_payload_extracts_suggested_answer_and_content() {
    let excerpts = extract_kb_excerpts(
        r#"{
            "suggested_answer": "Northstar Events is a fictional event operations company.",
            "evidence": [{
                "title": "Company Overview",
                "doc_id": "northstar-events/company-overview",
                "content": "Northstar Events is a fictional event operations company used for the Tandem demo."
            }]
        }"#,
        MAX_FULL_DOCUMENT_CHARS,
    );
    assert_eq!(excerpts.len(), 1);
    assert!(excerpts[0].contains("Suggested answer: Northstar Events"));
    assert!(excerpts[0].contains("Source: Company Overview"));
    assert!(excerpts[0].contains("used for the Tandem demo"));
}

#[test]
fn suggested_answer_evidence_answers_definition_without_hedging() {
    let evidence = vec![KbEvidenceItem {
        excerpt: "Suggested answer: Northstar Events is a fictional event operations company.\nSource: Company Overview\nNorthstar Events is a fictional event operations company used for the Tandem demo.".to_string(),
        sources: vec!["Company Overview".to_string()],
        full_document: true,
    }];
    let (_, answer) =
        deterministic_strict_kb_answer("What is Northstar?", &evidence).expect("answer");
    assert_eq!(
        answer,
        "Northstar Events is a fictional event operations company."
    );
    assert!(!answer.to_ascii_lowercase().contains("appears"));
}

#[test]
fn answer_question_suggested_answer_does_not_swallow_full_document() {
    let excerpts = extract_kb_excerpts(
        r##"{
            "suggested_answer": "Northstar Events is a fictional event operations company that produces mid-sized technology, gaming, and creator-community events across Europe.",
            "evidence": [{
                "title": "Company Overview",
                "source_label": "Company Overview",
                "content": "# Company Overview\n\n## Company\n\nNorthstar Events is a fictional event operations company that produces mid-sized technology, gaming, and creator-community events across Europe.\n\nThe company specializes in:\n\n- live event operations\n- online broadcast coordination\n- sponsor activation"
            }]
        }"##,
        MAX_FULL_DOCUMENT_CHARS,
    );
    let evidence = vec![KbEvidenceItem {
        excerpt: excerpts[0].clone(),
        sources: vec!["Company Overview".to_string()],
        full_document: true,
    }];
    let (_, answer) =
        deterministic_strict_kb_answer("What is Northstar?", &evidence).expect("answer");
    assert_eq!(
        answer,
        "Northstar Events is a fictional event operations company that produces mid-sized technology, gaming, and creator-community events across Europe."
    );
    assert!(!answer.contains("# Company Overview"));
    assert!(!answer.contains("live event operations"));
}

#[test]
fn nested_suggested_answer_is_cleaned_before_rendering() {
    let evidence = vec![KbEvidenceItem {
        excerpt: "Suggested answer: Suggested answer: If the primary stream ingest fails: Do not restart the encoder immediately. Only the streaming lead should modify ingest settings. # Streaming Troubleshooting  ## Purpose This runbook explains common streaming issues.\nSource: Streaming Troubleshooting\nIf the primary stream ingest fails: Do not restart the encoder immediately. Only the streaming lead should modify ingest settings.".to_string(),
        sources: vec!["Streaming Troubleshooting".to_string()],
        full_document: true,
    }];
    let (_, answer) = deterministic_strict_kb_answer(
        "What should staff do if the stream ingest fails?",
        &evidence,
    )
    .expect("answer");
    assert_eq!(
        answer,
        "If the primary stream ingest fails: Do not restart the encoder immediately. Only the streaming lead should modify ingest settings."
    );
    assert!(!answer.contains("Suggested answer:"));
    assert!(!answer.contains("# Streaming"));
}

#[test]
fn document_refs_are_collected_from_kb_search_results() {
    let policy = tandem_core::KnowledgebaseGroundingPolicy {
        required: true,
        strict: true,
        server_names: vec!["kb".to_string()],
        tool_patterns: vec!["mcp.kb.*".to_string()],
    };
    let message = tandem_types::Message::new(
        MessageRole::User,
        vec![MessagePart::ToolInvocation {
            tool: "mcp.kb.search_docs".to_string(),
            args: json!({"query": "crypto prize payouts"}),
            result: Some(json!({
                "collection_id": "northstar-events",
                "results": [{
                    "doc_id": "northstar-events/company-overview",
                    "source_path": "company-overview.md",
                    "excerpt": "Important internal note"
                }]
            })),
            error: None,
        }],
    );
    let refs = collect_kb_document_refs(&message, &policy);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].server_name, "kb");
    assert_eq!(refs[0].doc_id, "northstar-events/company-overview");
    assert_eq!(refs[0].collection_id.as_deref(), Some("northstar-events"));
}

#[test]
fn document_refs_ignore_source_bound_internal_identifiers() {
    let policy = tandem_core::KnowledgebaseGroundingPolicy {
        required: true,
        strict: true,
        server_names: vec!["kb".to_string()],
        tool_patterns: vec!["mcp.kb.*".to_string()],
    };
    let message = tandem_types::Message::new(
        MessageRole::User,
        vec![MessagePart::ToolInvocation {
            tool: "mcp.kb.search_docs".to_string(),
            args: json!({"query": "payroll"}),
            result: Some(json!({
                "collection_id": "northstar-events",
                "results": [{
                    "doc_id": "source-object-hr-payroll",
                    "source_path": "/imports/hr/payroll.md",
                    "excerpt": "Payroll policy content"
                }]
            })),
            error: None,
        }],
    );
    let refs = collect_kb_document_refs(&message, &policy);
    assert!(refs.is_empty());
}

#[test]
fn full_document_evidence_supports_explicitly_undefined_policy() {
    let evidence = vec![KbEvidenceItem {
        excerpt: "Source: Company Overview\nThe knowledgebase does not define policy for crypto prize payouts, token rewards, or blockchain-based giveaways. The correct response is that no policy is available in the current knowledgebase.".to_string(),
        sources: vec!["Company Overview".to_string()],
        full_document: true,
    }];
    let (_, answer) =
        deterministic_strict_kb_answer("What is the policy for crypto prize payouts?", &evidence)
            .expect("deterministic answer");
    assert!(answer.contains("I do not see a crypto prize payout policy"));
    assert!(answer.contains("does not define policy for crypto prize payouts"));
    assert!(!answer.contains("approved standard channels"));
    assert!(!answer.contains("wallet"));
}

#[test]
fn full_document_evidence_supports_missing_private_contact_info() {
    let evidence = vec![KbEvidenceItem {
        excerpt: "Source: Staff Roles and Contacts\nMira Kovac is the event director. Responsibilities include final escalation decisions. Demo email: mira@example.test. This demo knowledgebase does not contain real private phone numbers.".to_string(),
        sources: vec!["Staff Roles and Contacts".to_string()],
        full_document: true,
    }];
    let (_, answer) =
        deterministic_strict_kb_answer("What is Mira Kovac's phone number?", &evidence)
            .expect("deterministic answer");
    assert!(answer.contains("I do not see a phone number for Mira Kovac"));
    assert!(answer.contains("does not contain real private phone numbers"));
    assert!(!answer.contains("not visible in snippet"));
}

#[tokio::test]
#[serial_test::serial]
async fn strict_kb_transport_isolates_hosted_codex_auth_from_local_and_other_tenants() {
    use crate::http::session_run_retry::provider_auth_test_support::install_capturing_codex_provider;
    use tandem_providers::ProviderAuthOverride;

    let state = crate::test_support::test_state().await;
    let tenant_a = TenantContext::explicit("kb-org-a", "kb-workspace-a", None);
    let tenant_b = TenantContext::explicit("kb-org-b", "kb-workspace-b", None);
    let tenant_missing = TenantContext::explicit("kb-org-missing", "kb-workspace-missing", None);
    let response = serde_json::json!({
        "kb_answer_support": "supported",
        "supported_facts": ["The policy is in the evidence."],
        "missing_facts": [],
        "sources": ["Policy"],
        "answer_text": "The policy is in the evidence."
    })
    .to_string();
    let captured = install_capturing_codex_provider(
        &state,
        response,
        &[(&tenant_a, "kb-token-a"), (&tenant_b, "kb-token-b")],
    )
    .await;
    let model = ModelSpec {
        provider_id: "openai-codex".to_string(),
        model_id: "codex-test".to_string(),
    };
    let evidence = vec![KbEvidenceItem {
        excerpt: "Source: Policy\nThe policy is in the evidence.".to_string(),
        sources: vec!["Policy".to_string()],
        full_document: true,
    }];

    for (index, tenant_context) in [&tenant_a, &tenant_b, &tenant_missing]
        .into_iter()
        .enumerate()
    {
        let run_id = format!("kb-run-{index}");
        let session_id = format!("kb-session-{index}");
        let output = synthesize_strict_kb_answer(
            &state,
            "What is the policy?",
            &evidence,
            Some(&model),
            &run_id,
            &session_id,
            tenant_context,
            None,
        )
        .await
        .expect("KB synthesis dispatch");
        assert!(output.is_some());
    }

    assert_eq!(
        captured.lock().expect("provider auth capture").as_slice(),
        [
            ProviderAuthOverride::Bearer("kb-token-a".to_string()),
            ProviderAuthOverride::Bearer("kb-token-b".to_string()),
            ProviderAuthOverride::Suppress,
        ]
    );
}

#[tokio::test]
#[serial_test::serial(data_boundary_env)]
async fn strict_kb_completion_fallback_evaluates_the_exact_rebuilt_prompt() {
    use crate::http::session_run_retry::provider_auth_test_support::install_capturing_codex_provider;
    use tandem_providers::ProviderAuthOverride;

    let previous_mode = std::env::var("TANDEM_DATA_BOUNDARY_MODE").ok();
    std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "audit");
    let state = crate::test_support::test_state().await;
    let tenant = TenantContext::explicit("kb-fallback-org", "kb-fallback-workspace", None);
    let response = serde_json::json!({
        "kb_answer_support": "supported",
        "supported_facts": ["Grounded fact"],
        "missing_facts": [],
        "sources": ["Policy"],
        "answer_text": "Grounded fact"
    })
    .to_string();
    let captured =
        install_capturing_codex_provider(&state, response, &[(&tenant, "kb-fallback-token")]).await;
    let mut events = state.event_bus.subscribe();
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "rules".to_string(),
            attachments: Vec::new(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: "evidence".to_string(),
            attachments: Vec::new(),
        },
    ];

    let output = retry_strict_kb_non_streaming_synthesis(
        &state,
        "openai-codex",
        Some("codex-test"),
        &messages,
        "stream chunk error",
        "kb-fallback-session",
        "kb-fallback-run",
        &tenant,
        None,
    )
    .await
    .expect("KB completion fallback dispatch");
    assert!(output.is_some());

    let boundary_event = loop {
        let event = events.try_recv().expect("fallback boundary event");
        if event.event_type.starts_with("data_boundary.") {
            break event;
        }
    };
    let fallback_prompt = "system:\nrules\n\nuser:\nevidence";
    let evaluated_payload = format!("{fallback_prompt}\n");
    assert_eq!(
        boundary_event.properties["operation"]["operation_id"],
        "kb-fallback-session:kb_synthesis:completion_fallback"
    );
    assert_eq!(
        boundary_event.properties["payload_hash"],
        tandem_data_boundary::payload_hash(evaluated_payload.as_bytes())
    );
    assert_eq!(
        captured.lock().expect("provider auth capture").as_slice(),
        [ProviderAuthOverride::Bearer(
            "kb-fallback-token".to_string()
        )]
    );

    match previous_mode {
        Some(value) => std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", value),
        None => std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE"),
    }
}
