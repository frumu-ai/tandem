use crate::types::{GlobalMemoryRecord, MemoryAccessFilter, MemorySourceAccessTarget};
use tandem_enterprise_contract::{
    AccessPermission, AssertionMetadata, AuthorityChain, CrossTenantGrant, CrossTenantGrantClaims,
    CrossTenantGrantHeader, CrossTenantGrantParty, CrossTenantGrantRecord, DataBoundary, DataClass,
    GrantSource, PrincipalRef, RequestPrincipal, ResourceKind, ResourceRef, ResourceScope,
    ScopedGrant, StrictTenantContext, TenantContext,
};

#[test]
fn memory_access_filter_allows_active_cross_tenant_projection() {
    let issuer = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "admin-a");
    let audience = TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "user-b");
    let subject = PrincipalRef::human_user("user-b");
    let resource = ResourceRef::new(
        "org-a",
        "workspace-a",
        ResourceKind::DocumentCollection,
        "finance-drive",
    );
    let claims = CrossTenantGrantClaims::new_v1(
        "grant-finance",
        CrossTenantGrantParty::from_tenant_context(&issuer),
        CrossTenantGrantParty::from_tenant_context(&audience),
        subject.clone(),
        ResourceScope::root(resource.clone()),
        vec![AccessPermission::Read],
        vec![DataClass::FinancialRecord],
        1_000,
        5_000,
        PrincipalRef::human_user("admin-a"),
    );
    let record = CrossTenantGrantRecord::active(
        CrossTenantGrant::new(
            CrossTenantGrantHeader::ed25519("grant-key"),
            claims,
            "signature-bytes",
        ),
        1_000,
    );
    let request_principal = RequestPrincipal::authenticated_user("user-b", "test");
    let mut strict = StrictTenantContext::new(
        audience,
        subject,
        AuthorityChain::from_request(request_principal),
        ResourceScope::root(ResourceRef::new(
            "org-b",
            "workspace-b",
            ResourceKind::Workspace,
            "workspace-b",
        )),
        AssertionMetadata::new("issuer", "runtime", 1_000, 5_000, "assertion-b"),
    )
    .with_data_boundary(DataBoundary::allow(vec![DataClass::FinancialRecord]));

    assert!(record.project_into_strict_context(&mut strict, 2_000));
    let filter = MemoryAccessFilter::strict(strict, 2_000);
    let target = MemorySourceAccessTarget {
        resource_ref: resource,
        data_class: DataClass::FinancialRecord,
        source_binding_id: Some("finance-drive".to_string()),
        source_object_id: Some("statement-q4".to_string()),
    };

    assert!(filter.allows_source_target(&target));
}

#[test]
fn governed_read_filter_denies_missing_strict_projection() {
    let filter = MemoryAccessFilter::governed(None, 2_000);
    let decision = filter.decision_for_global_record(&global_record(None));

    assert!(!decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("missing_strict_projection")
    );
}

#[test]
fn local_noop_read_filter_preserves_legacy_visibility() {
    let filter = MemoryAccessFilter::local_noop(2_000);
    let decision = filter.decision_for_global_record(&global_record(Some(serde_json::json!({
        "memory_trust": {
            "label": "connector_sourced"
        }
    }))));

    assert!(decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("local_noop"));
}

#[test]
fn governed_read_filter_allows_internal_tenant_memory_with_default_boundary() {
    let filter = MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000);
    let decision = filter.decision_for_global_record(&global_record(None));

    assert!(decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("tenant_local_memory_allowed")
    );
}

#[test]
fn workflow_phase_read_filter_preserves_internal_tenant_memory_visibility() {
    let filter = MemoryAccessFilter::strict_with_workflow_phase(
        tenant_strict(DataBoundary::unrestricted()),
        2_000,
        "draft",
    );
    let decision = filter.decision_for_global_record(&global_record(None));

    assert!(decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("tenant_local_memory_allowed")
    );
}

#[test]
fn governed_read_filter_denies_restricted_memory_with_default_boundary() {
    let filter = MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000);
    let decision = filter.decision_for_global_record(&global_record(Some(serde_json::json!({
        "classification": "restricted"
    }))));

    assert!(!decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("data_class_denied_by_boundary")
    );
}

#[test]
fn governed_read_filter_allows_restricted_memory_with_explicit_boundary() {
    let filter = MemoryAccessFilter::strict(
        tenant_strict(DataBoundary::allow(vec![DataClass::Restricted])),
        2_000,
    );
    let decision = filter.decision_for_global_record(&global_record(Some(serde_json::json!({
        "classification": "restricted"
    }))));

    assert!(decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("tenant_local_memory_allowed")
    );
}

#[test]
fn governed_read_filter_derives_boundary_from_allow_grants() {
    let resource = ResourceRef::new(
        "org-a",
        "workspace-a",
        ResourceKind::DocumentCollection,
        "finance-drive",
    );
    let grant = ScopedGrant::new(
        "grant-finance",
        PrincipalRef::human_user("user-a"),
        resource.clone(),
        GrantSource::Direct,
    )
    .with_permissions(vec![AccessPermission::Read])
    .with_data_classes(vec![DataClass::FinancialRecord]);
    let strict = tenant_strict(DataBoundary::unrestricted()).with_grants(vec![grant]);
    let filter = MemoryAccessFilter::strict(strict, 2_000);
    let target = MemorySourceAccessTarget {
        resource_ref: resource,
        data_class: DataClass::FinancialRecord,
        source_binding_id: Some("finance-drive".to_string()),
        source_object_id: Some("statement-q4".to_string()),
    };

    assert!(filter.allows_source_target(&target));
}

#[test]
fn governed_read_filter_ignores_expired_grants_when_deriving_boundary() {
    let grant = ScopedGrant::new(
        "grant-finance-expired",
        PrincipalRef::human_user("user-a"),
        ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::Workspace,
            "workspace-a",
        ),
        GrantSource::Direct,
    )
    .with_permissions(vec![AccessPermission::Read])
    .with_data_classes(vec![DataClass::FinancialRecord])
    .with_expires_at_ms(1_500);
    let strict = tenant_strict(DataBoundary::unrestricted()).with_grants(vec![grant]);
    let filter = MemoryAccessFilter::strict(strict, 2_000);
    let decision = filter.decision_for_global_record(&global_record(Some(serde_json::json!({
        "classification": "financial_record"
    }))));

    assert!(!decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("data_class_denied_by_boundary")
    );
}

#[test]
fn governed_read_filter_denies_connector_sourced_memory_without_resource_metadata() {
    let filter = MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000);
    let decision = filter.decision_for_global_record(&global_record(Some(serde_json::json!({
        "memory_trust": {
            "label": "connector_sourced"
        }
    }))));

    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("missing_resource_ref"));
}

#[test]
fn governed_read_filter_denies_source_binding_without_data_class() {
    let filter = MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000);
    let decision = filter.decision_for_global_record(&global_record(Some(serde_json::json!({
        "enterprise_source_binding": {
            "binding_id": "finance-drive",
            "connector_id": "manual-upload",
            "resource_ref": {
                "organization_id": "org-a",
                "workspace_id": "workspace-a",
                "resource_kind": "document_collection",
                "resource_id": "finance-drive"
            }
        }
    }))));

    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("missing_data_class"));
}

#[test]
fn workflow_phase_read_filter_requires_registered_source_bound_scope() {
    let resource = ResourceRef::new(
        "org-a",
        "workspace-a",
        ResourceKind::DocumentCollection,
        "finance-drive",
    );
    let grant = ScopedGrant::new(
        "grant-finance",
        PrincipalRef::human_user("user-a"),
        resource.clone(),
        GrantSource::Direct,
    )
    .with_permissions(vec![AccessPermission::Read])
    .with_data_classes(vec![DataClass::FinancialRecord]);
    let strict = tenant_strict(DataBoundary::unrestricted()).with_grants(vec![grant]);
    let record = global_record(Some(serde_json::json!({
        "enterprise_source_binding": {
            "binding_id": "finance-drive",
            "connector_id": "manual-upload",
            "resource_ref": {
                "organization_id": "org-a",
                "workspace_id": "workspace-a",
                "resource_kind": "document_collection",
                "resource_id": "finance-drive"
            },
            "data_class": "financial_record",
            "source_object_id": "statement-q4"
        }
    })));

    let plain_decision =
        MemoryAccessFilter::strict(strict.clone(), 2_000).decision_for_global_record(&record);
    let workflow_decision = MemoryAccessFilter::strict_with_workflow_phase(strict, 2_000, "draft")
        .decision_for_global_record(&record);

    assert!(plain_decision.allowed);
    assert!(!workflow_decision.allowed);
    assert_eq!(
        workflow_decision.reason.as_deref(),
        Some("knowledge_scope_registry_missing")
    );
}

#[test]
fn org_unit_restricted_record_visible_only_to_members() {
    let restricted = global_record(Some(serde_json::json!({
        "owner_org_unit_id": "ou-eng"
    })));

    // A member of the owning unit reads the record.
    let member_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000)
            .with_caller_org_units(["ou-eng".to_string(), "ou-platform".to_string()]);
    let decision = member_filter.decision_for_global_record(&restricted);
    assert!(decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("tenant_local_memory_allowed")
    );

    // A principal in a different unit of the same tenant is denied.
    let non_member_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000)
            .with_caller_org_units(["ou-sales".to_string()]);
    let decision = non_member_filter.decision_for_global_record(&restricted);
    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("org_unit_scope_mismatch"));

    // No membership information at all denies, fail closed.
    let no_units_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000);
    let decision = no_units_filter.decision_for_global_record(&restricted);
    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("org_unit_scope_mismatch"));
}

#[test]
fn unscoped_record_is_fail_closed_for_department_scoped_caller() {
    // TAN-647: a record with no owning unit is invisible to a department-scoped
    // caller (fail closed), so legacy/untagged rows never leak across departments.
    let department_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000)
            .with_caller_org_units(["ou-sales".to_string()]);
    let decision = department_filter.decision_for_global_record(&global_record(None));
    assert!(!decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("org_unit_absent_fail_closed")
    );

    // …but an explicit `tenant_shared` record stays visible to every department.
    let shared = global_record(Some(serde_json::json!({ "tenant_shared": true })));
    let decision = department_filter.decision_for_global_record(&shared);
    assert!(decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("tenant_local_memory_allowed")
    );

    // A caller with NO department identity keeps the pre-org-unit visibility.
    let no_department_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000);
    let decision = no_department_filter.decision_for_global_record(&global_record(None));
    assert!(decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("tenant_local_memory_allowed")
    );
}

#[test]
fn local_noop_ignores_org_unit_restriction() {
    let filter = MemoryAccessFilter::local_noop(2_000);
    let decision = filter.decision_for_global_record(&global_record(Some(serde_json::json!({
        "owner_org_unit_id": "ou-eng"
    }))));
    assert!(decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("local_noop"));
}

#[test]
fn unscoped_chunk_is_fail_closed_for_department_scoped_caller() {
    // TAN-647: the fail-closed department default applies to the chunk read path
    // too (shared decision_for_target).
    let untagged = tenant_chunk(None);
    let department_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000)
            .with_caller_org_units(["ou-sales".to_string()]);
    let decision = department_filter.decision_for_chunk(&untagged);
    assert!(!decision.allowed);
    assert_eq!(
        decision.reason.as_deref(),
        Some("org_unit_absent_fail_closed")
    );

    // An explicit tenant_shared chunk stays visible to every department.
    let mut shared = tenant_chunk(None);
    shared.metadata = Some(serde_json::json!({ "tenant_shared": true }));
    let decision = department_filter.decision_for_chunk(&shared);
    assert!(decision.allowed);

    // A caller with no department identity keeps prior visibility.
    let no_department_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000);
    assert!(no_department_filter.decision_for_chunk(&untagged).allowed);
}

#[test]
fn subject_owned_chunk_without_department_stays_readable_by_owner() {
    // TAN-647 (review, P1): the absent-department fail-closed must not hide a
    // caller's own subject-owned memory that has no department yet. Such records
    // are governed by the subject check, not the department default.
    let private = tenant_chunk(Some("user-a")); // subject-owned, no department

    // The owner, reading under a department-scoped context, still sees it.
    let owner_department_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000)
            .with_caller_org_units(["ou-sales".to_string()])
            .with_caller_subject("user-a");
    assert!(owner_department_filter.decision_for_chunk(&private).allowed);

    // A different subject in another department is denied by the subject check
    // (no cross-department/cross-user leak).
    let other_department_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000)
            .with_caller_org_units(["ou-eng".to_string()])
            .with_caller_subject("user-b");
    let decision = other_department_filter.decision_for_chunk(&private);
    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("subject_scope_mismatch"));
}

#[test]
fn subject_restricted_chunk_visible_only_to_owner() {
    let restricted = tenant_chunk(Some("user-a"));

    // The owning subject reads the chunk.
    let owner_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000)
            .with_caller_subject("user-a");
    let decision = owner_filter.decision_for_chunk(&restricted);
    assert!(decision.allowed);

    // A different subject in the same tenant is denied.
    let other_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000)
            .with_caller_subject("user-b");
    let decision = other_filter.decision_for_chunk(&restricted);
    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("subject_scope_mismatch"));

    // No caller-subject information denies, fail closed.
    let no_subject_filter =
        MemoryAccessFilter::strict(tenant_strict(DataBoundary::unrestricted()), 2_000);
    let decision = no_subject_filter.decision_for_chunk(&restricted);
    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("subject_scope_mismatch"));

    // Unrestricted chunks keep shared visibility for any subject.
    let shared = tenant_chunk(None);
    let decision = other_filter.decision_for_chunk(&shared);
    assert!(decision.allowed);

    // Local mode is unaffected.
    let decision = MemoryAccessFilter::local_noop(2_000).decision_for_chunk(&restricted);
    assert!(decision.allowed);
}

fn tenant_chunk(subject: Option<&str>) -> crate::types::MemoryChunk {
    crate::types::MemoryChunk {
        id: "chunk-a".to_string(),
        content: "archived exchange".to_string(),
        tier: crate::types::MemoryTier::Global,
        session_id: None,
        project_id: None,
        source: "chat_exchange".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: crate::types::MemoryTenantScope {
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            deployment_id: None,
        },
        subject: subject.map(ToString::to_string),
        created_at: chrono::Utc::now(),
        token_count: 3,
        metadata: None,
    }
}

fn tenant_strict(data_boundary: DataBoundary) -> StrictTenantContext {
    let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
    let principal = RequestPrincipal::authenticated_user("user-a", "test");
    StrictTenantContext::new(
        tenant,
        PrincipalRef::human_user("user-a"),
        AuthorityChain::from_request(principal),
        ResourceScope::root(ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::Workspace,
            "workspace-a",
        )),
        AssertionMetadata::new("issuer", "runtime", 1_000, 5_000, "assertion-a"),
    )
    .with_data_boundary(data_boundary)
}

fn global_record(metadata: Option<serde_json::Value>) -> GlobalMemoryRecord {
    GlobalMemoryRecord {
        id: "memory-a".to_string(),
        user_id: "user-a".to_string(),
        source_type: "note".to_string(),
        content: "tenant memory".to_string(),
        content_hash: "hash-a".to_string(),
        run_id: "run-a".to_string(),
        session_id: Some("session-a".to_string()),
        message_id: None,
        tool_name: None,
        project_tag: Some("project-a".to_string()),
        channel_tag: None,
        host_tag: None,
        metadata,
        provenance: None,
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "shared".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
        expires_at_ms: None,
    }
}
