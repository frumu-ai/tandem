// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

mod export_authority_tests {
    use super::*;

    fn strict_context_with_classes(
        run_id: &str,
        data_classes: Vec<tandem_types::DataClass>,
    ) -> tandem_types::StrictTenantContext {
        let resource = tandem_types::ResourceRef::new(
            "org-a",
            "workspace-a",
            tandem_types::ResourceKind::AuditExport,
            run_id,
        );
        let tenant_context =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "auditor-1");
        let principal = tandem_types::PrincipalRef::human_user("auditor-1");
        let grant = tandem_types::ScopedGrant::new(
            "grant-export",
            principal.clone(),
            resource.clone(),
            tandem_types::GrantSource::Direct,
        )
        .with_permissions(vec![tandem_types::AccessPermission::Read])
        .with_data_classes(data_classes);
        tandem_types::StrictTenantContext::new(
            tenant_context,
            principal.clone(),
            tandem_types::AuthorityChain::from_request(
                tandem_types::RequestPrincipal::authenticated_user(principal.id, "tandem-web"),
            ),
            tandem_types::ResourceScope::root(resource),
            tandem_types::AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                9_999_999_999_999,
                "assertion-export",
            ),
        )
        .with_grants(vec![grant])
    }

    fn decision_with_classes(data_classes: Vec<tandem_types::DataClass>) -> PolicyDecisionRecord {
        serde_json::from_value(json!({
            "decision_id": "decision-1",
            "tenant_context": TenantContext::local_implicit(),
            "data_classes": data_classes,
            "decision": "allow",
            "reason_code": "test",
            "reason": "test fixture",
            "created_at_ms": 1_500,
        }))
        .expect("policy decision fixture")
    }

    #[test]
    fn export_allowed_when_every_included_class_is_granted() {
        let strict = strict_context_with_classes(
            "run-1",
            vec![
                tandem_types::DataClass::Internal,
                tandem_types::DataClass::Restricted,
            ],
        );
        let decisions = vec![decision_with_classes(vec![
            tandem_types::DataClass::Restricted,
        ])];
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &decisions, None, 2_000),
            None
        );
    }

    #[test]
    fn export_rejected_when_a_policy_decision_class_is_not_granted() {
        // The principal can read Internal evidence but a included policy
        // decision carries Restricted data: the whole package is rejected,
        // so restricted data is never included in an unauthorized export.
        let strict = strict_context_with_classes("run-1", vec![tandem_types::DataClass::Internal]);
        let decisions = vec![decision_with_classes(vec![
            tandem_types::DataClass::Restricted,
        ])];
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &decisions, None, 2_000),
            Some(tandem_types::DataClass::Restricted)
        );
    }

    #[test]
    fn export_rejected_without_any_grant_for_the_run_resource() {
        // Grant is scoped to a different run: baseline Internal evidence is
        // already unreadable, fail closed.
        let strict =
            strict_context_with_classes("run-other", vec![tandem_types::DataClass::Internal]);
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &[], None, 2_000),
            Some(tandem_types::DataClass::Internal)
        );
    }

    #[test]
    fn export_rejected_when_an_artifact_class_is_not_granted() {
        // A node output carrying a Restricted artifact class gates the export
        // even when no policy decision carries that class (Codex P1 on
        // PR #1557): artifact metadata is serialized into the package, so
        // its classes must be readable too.
        let strict = strict_context_with_classes("run-1", vec![tandem_types::DataClass::Internal]);
        let mut run: crate::automation_v2::types::AutomationV2RunRecord =
            serde_json::from_value(json!({
                "run_id": "run-1",
                "automation_id": "auto-1",
                "tenant_context": TenantContext::local_implicit(),
                "trigger_type": "manual",
                "status": "completed",
                "created_at_ms": 1_500,
                "updated_at_ms": 1_500,
                "checkpoint": {},
            }))
            .expect("run fixture");
        run.checkpoint.node_outputs.insert(
            "export_step".to_string(),
            json!({
                "artifact_id": "artifact-1",
                "data_class": "restricted",
                "content": "redacted"
            }),
        );
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &[], Some(&run), 2_000),
            Some(tandem_types::DataClass::Restricted)
        );

        // With the Restricted grant the same package exports.
        let granted = strict_context_with_classes(
            "run-1",
            vec![
                tandem_types::DataClass::Internal,
                tandem_types::DataClass::Restricted,
            ],
        );
        assert_eq!(
            governance_evidence_export_denial(&granted, "run-1", &[], Some(&run), 2_000),
            None
        );
    }

    #[test]
    fn expired_assertion_fails_closed() {
        let strict = strict_context_with_classes("run-1", vec![tandem_types::DataClass::Internal]);
        // now_ms beyond the assertion expiry of the fixture
        assert_eq!(
            governance_evidence_export_denial(&strict, "run-1", &[], None, 99_999_999_999_999),
            Some(tandem_types::DataClass::Internal)
        );
    }
}
