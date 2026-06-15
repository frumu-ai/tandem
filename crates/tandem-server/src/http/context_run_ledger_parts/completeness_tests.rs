    // ---- EUAI-09 / TAN-250: log-completeness check tests ----

    fn approval_required_decision(
        decision_id: &str,
        tenant: &TenantContext,
    ) -> tandem_types::PolicyDecisionRecord {
        tandem_types::PolicyDecisionRecord {
            decision_id: decision_id.to_string(),
            tenant_context: tenant.clone(),
            actor_id: Some("finance-user".to_string()),
            session_id: None,
            message_id: None,
            run_id: Some("automation-v2-run-fintech".to_string()),
            automation_id: Some("automation-fintech".to_string()),
            node_id: Some("release_funds".to_string()),
            tool: Some("mcp.bank.release_funds".to_string()),
            resource: None,
            data_classes: Vec::new(),
            risk_tier: Some("money_movement".to_string()),
            decision: tandem_types::PolicyDecisionEffect::ApprovalRequired,
            reason_code: "approval_required".to_string(),
            reason: "approval required".to_string(),
            policy_id: None,
            grant_id: None,
            approval_id: Some("approval-1".to_string()),
            audit_event_id: None,
            created_at_ms: 10,
            metadata: json!({}),
        }
    }

    fn protected_tool_outcome(seq: u64, decision_id: Option<&str>) -> ContextRunEventRecord {
        let mut event = tool_effect_event(seq, "mcp.bank.release_funds", "outcome", "succeeded");
        if let Some(id) = decision_id {
            event.payload["record"]["policy_decision_id"] = json!(id);
        }
        event
    }

    fn completeness_finding_kinds(completeness: &Value) -> Vec<String> {
        completeness["findings"]
            .as_array()
            .map(|findings| {
                findings
                    .iter()
                    .filter_map(|finding| finding["kind"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn audit_completeness_complete_when_all_records_present() {
        let run = fintech_audit_fixture_run();
        let context_run = governance_evidence_context_run(&run);
        let tenant = context_run.tenant_context.clone();
        let decisions = vec![approval_required_decision("decision-1", &tenant)];
        let records = context_run_ledger_records(&[protected_tool_outcome(1, Some("decision-1"))]);

        let completeness =
            governance_evidence_completeness(&context_run, Some(&run), &records, &decisions, &[]);

        assert_eq!(completeness["status"].as_str(), Some("complete"));
        assert_eq!(completeness["counts"]["errors"].as_u64(), Some(0));
        assert_eq!(completeness["counts"]["warnings"].as_u64(), Some(0));
        assert!(completeness["event_taxonomy"]
            .as_array()
            .map(|taxonomy| taxonomy.iter().any(|entry| entry == "protected_tool_call"))
            .unwrap_or(false));
    }

    #[test]
    fn audit_completeness_flags_missing_approval_evidence() {
        let run = fintech_audit_fixture_run();
        let context_run = governance_evidence_context_run(&run);
        let tenant = context_run.tenant_context.clone();
        let mut decision = approval_required_decision("decision-1", &tenant);
        decision.approval_id = None; // no approval id and no gate approval recorded
        let records = context_run_ledger_records(&[protected_tool_outcome(1, Some("decision-1"))]);

        let completeness = governance_evidence_completeness(
            &context_run,
            Some(&run),
            &records,
            &[decision],
            &[],
        );

        assert_eq!(completeness["status"].as_str(), Some("incomplete"));
        assert!(completeness_finding_kinds(&completeness)
            .contains(&"missing_approval_evidence".to_string()));
    }

    #[test]
    fn audit_completeness_flags_missing_protected_event() {
        let run = fintech_audit_fixture_run();
        let context_run = governance_evidence_context_run(&run);
        let tenant = context_run.tenant_context.clone();
        let mut decision = approval_required_decision("decision-1", &tenant);
        // References an audit event that is not present in the packet.
        decision.audit_event_id = Some("audit-missing".to_string());
        let records = context_run_ledger_records(&[protected_tool_outcome(1, Some("decision-1"))]);

        let completeness = governance_evidence_completeness(
            &context_run,
            Some(&run),
            &records,
            &[decision],
            &[],
        );

        assert_eq!(completeness["status"].as_str(), Some("incomplete"));
        assert!(completeness_finding_kinds(&completeness)
            .contains(&"missing_protected_audit_event".to_string()));
    }

    #[test]
    fn audit_completeness_flags_tenant_mismatch() {
        let run = fintech_audit_fixture_run();
        let context_run = governance_evidence_context_run(&run);
        let other_tenant = TenantContext::explicit("other-org", "other-ws", None);
        let decision = approval_required_decision("decision-1", &other_tenant);
        let records = context_run_ledger_records(&[protected_tool_outcome(1, Some("decision-1"))]);

        let completeness = governance_evidence_completeness(
            &context_run,
            Some(&run),
            &records,
            &[decision],
            &[],
        );

        assert_eq!(completeness["status"].as_str(), Some("incomplete"));
        assert!(completeness_finding_kinds(&completeness).contains(&"tenant_mismatch".to_string()));
    }

    #[test]
    fn audit_completeness_flags_expired_approval() {
        let run = fintech_audit_fixture_run();
        let context_run = governance_evidence_context_run(&run);
        let tenant = context_run.tenant_context.clone();
        let mut decision = approval_required_decision("decision-1", &tenant);
        // Approval expired at ms 50; the linked outcome executes at ms 100 (seq 10 * 10).
        decision.metadata = json!({ "expires_at_ms": 50 });
        let records = context_run_ledger_records(&[protected_tool_outcome(10, Some("decision-1"))]);

        let completeness = governance_evidence_completeness(
            &context_run,
            Some(&run),
            &records,
            &[decision],
            &[],
        );

        assert_eq!(completeness["status"].as_str(), Some("incomplete"));
        assert!(completeness_finding_kinds(&completeness)
            .contains(&"expired_approval".to_string()));
    }

    #[test]
    fn audit_completeness_flags_protected_call_without_policy_decision() {
        let run = fintech_audit_fixture_run();
        let context_run = governance_evidence_context_run(&run);
        // A money-movement tool succeeded with no linked policy decision (missing receipt).
        let records = context_run_ledger_records(&[protected_tool_outcome(1, None)]);

        let completeness =
            governance_evidence_completeness(&context_run, Some(&run), &records, &[], &[]);

        assert_eq!(completeness["status"].as_str(), Some("incomplete"));
        assert!(completeness_finding_kinds(&completeness)
            .contains(&"missing_policy_decision".to_string()));
    }
