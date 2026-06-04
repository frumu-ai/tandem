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
