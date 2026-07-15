// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

    #[test]
    fn fintech_audit_package_excludes_unauthorized_scoped_artifacts() {
        let mut run = fintech_audit_fixture_run();
        let finance_resource = tandem_types::ResourceRef::new(
            "acme",
            "finance",
            tandem_types::ResourceKind::DataStore,
            "finance-ledger",
        );
        let engineering_resource = tandem_types::ResourceRef::new(
            "acme",
            "engineering",
            tandem_types::ResourceKind::Repository,
            "product-api",
        );
        run.checkpoint.node_outputs = HashMap::from([
            (
                "finance_summary".to_string(),
                json!({
                    "artifact_id": "finance-summary",
                    "resource_ref": finance_resource,
                    "data_class": "financial_record",
                }),
            ),
            (
                "engineering_patch".to_string(),
                json!({
                    "artifact_id": "engineering-patch",
                    "resource_ref": engineering_resource,
                    "data_class": "source_code",
                }),
            ),
        ]);
        let strict_context = test_artifact_export_context(
            tandem_types::ResourceRef::new(
                "acme",
                "finance",
                tandem_types::ResourceKind::DataStore,
                "finance-ledger",
            ),
            tandem_types::DataClass::FinancialRecord,
        );

        let package = fintech_audit_package_for_automation_v2_run_records_authorized(
            &run,
            &[],
            Some(&strict_context),
        );

        let artifacts = package["artifacts"].as_array().expect("artifacts");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0]["node_id"].as_str(), Some("finance_summary"));
        assert!(
            package["limitations"]
                .as_array()
                .expect("limitations")
                .iter()
                .any(|row| row
                    .as_str()
                    .is_some_and(|value| value.contains("engineering_patch"))),
            "engineering scoped artifact should be excluded from the package"
        );
    }

    #[test]
    fn fintech_audit_package_excludes_scoped_artifacts_without_strict_projection() {
        let mut run = fintech_audit_fixture_run();
        run.checkpoint.node_outputs = HashMap::from([(
            "hr_compensation".to_string(),
            json!({
                "artifact_id": "hr-compensation",
                "resource_ref": {
                    "organization_id": "acme",
                    "workspace_id": "hr",
                    "resource_kind": "document",
                    "resource_id": "compensation-bands"
                },
                "data_class": "financial_record"
            }),
        )]);

        let package =
            fintech_audit_package_for_automation_v2_run_records_authorized(&run, &[], None);

        assert_eq!(package["artifacts"].as_array().map(Vec::len), Some(0));
        assert!(
            package["limitations"]
                .as_array()
                .expect("limitations")
                .iter()
                .any(|row| row
                    .as_str()
                    .is_some_and(|value| value.contains("missing_strict_projection"))),
            "scoped artifacts should fail closed without strict projection"
        );
    }

    #[tokio::test]
    async fn persists_fintech_audit_package_to_context_run_artifact() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::new_starting("test".to_string(), true);
        state.shared_resources_path = root.path().join("system").join("shared.json");
        let run = fintech_audit_fixture_run();

        let receipt = persist_fintech_audit_package_for_automation_v2_run(&state, &run)
            .await
            .expect("persist package");
        let path = receipt["path"].as_str().expect("path");
        let raw = std::fs::read_to_string(path).expect("audit package file");
        let persisted: Value = serde_json::from_str(&raw).expect("package json");

        assert_eq!(receipt["artifact_id"], "fintech-audit-package");
        assert_eq!(persisted["run_id"], "automation-v2-run-fintech");
        assert_eq!(
            persisted["artifacts"][0]["node_id"].as_str(),
            Some("draft_compliance_brief")
        );
    }

    fn test_artifact_export_context(
        resource: tandem_types::ResourceRef,
        data_class: tandem_types::DataClass,
    ) -> tandem_types::StrictTenantContext {
        let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
            "acme",
            "finance",
            Some("deployment-test".to_string()),
            "finance-user",
        );
        let principal = tandem_types::PrincipalRef::human_user("finance-user");
        let grant = tandem_types::ScopedGrant::new(
            "grant-artifact-export",
            principal.clone(),
            resource.clone(),
            tandem_types::GrantSource::Direct,
        )
        .with_permissions(vec![tandem_types::AccessPermission::Read])
        .with_data_classes(vec![data_class]);
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
                "assertion-artifact-export",
            ),
        )
        .with_grants(vec![grant])
        .with_data_boundary(tandem_types::DataBoundary::allow(vec![data_class]))
    }
