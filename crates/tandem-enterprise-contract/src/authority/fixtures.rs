//! Seed fixtures for the intra-tenant authority graph (CT-18 / TAN-89).
//!
//! [`acme_company`] builds a single-tenant authority graph for a fictional
//! company `acme` with engineering, finance, sales, HR, executive, and support
//! personas. Departments are modeled as [`OrganizationUnit`]s inside one tenant
//! (`org = acme`, `workspace = acme`) rather than separate workspaces, so the
//! boundaries between them are governed by grants and data classes — exactly
//! the intra-tenant authority the graph exists to enforce.
//!
//! The fixture intentionally exercises every shape the acceptance criteria
//! call for:
//!
//! * role-domain nesting (junior/lead engineering under an engineering
//!   department) so a child role inherits department grants but not sibling
//!   grants,
//! * an executive org-wide allow that a targeted deny grant still overrides
//!   (segregation of duties / legal hold),
//! * an expiring cross-department share ("unless shared") that lapses, and
//! * resources spanning source code, internal architecture, credentials,
//!   financial records, customer data, and HR compensation.

use crate::{
    AccessEffect, AccessPermission, DataClass, GrantSource, OrganizationUnit,
    OrganizationUnitAccessGrant, OrganizationUnitKind, OrganizationUnitMembership,
    OrganizationUnitMembershipSource, PrincipalRef, ResourceKind, ResourcePathSegment, ResourceRef,
    ScopedGrant, TenantContext,
};

use super::IntraTenantAuthorityGraph;

/// Organization id for the seeded company.
pub const ORG_ID: &str = "acme";
/// Workspace id for the seeded company (single workspace; departments are units).
pub const WORKSPACE_ID: &str = "acme";
/// Taxonomy id shared by every seeded organization unit.
pub const TAXONOMY_ID: &str = "org";

/// Baseline "now" for the fixture, in epoch milliseconds.
pub const BASE_NOW_MS: u64 = 1_700_000_000_000;
/// A moment while the expiring cross-department share is still valid.
pub const SHARE_VALID_NOW_MS: u64 = BASE_NOW_MS + 1_800_000; // +30m
/// A moment after the expiring cross-department share has lapsed.
pub const SHARE_EXPIRED_NOW_MS: u64 = BASE_NOW_MS + 7_200_000; // +2h
const SHARE_EXPIRES_AT_MS: u64 = BASE_NOW_MS + 3_600_000; // +1h

/// A fully-seeded `acme` authority graph plus the named personas and resources
/// referenced by its grants, so tests and eval seeds can assert against stable
/// handles instead of reconstructing refs.
#[derive(Debug, Clone)]
pub struct AcmeAuthorityFixture {
    pub tenant_context: TenantContext,
    pub graph: IntraTenantAuthorityGraph,

    // Personas.
    pub junior_engineer: PrincipalRef,
    pub junior_engineer_agent: PrincipalRef,
    pub lead_engineer: PrincipalRef,
    pub finance_analyst: PrincipalRef,
    pub sales_rep: PrincipalRef,
    pub hr_partner: PrincipalRef,
    pub executive: PrincipalRef,
    pub support_agent: PrincipalRef,

    // Resources.
    pub engineering_repo: ResourceRef,
    pub internal_architecture_doc: ResourceRef,
    pub engineering_secret: ResourceRef,
    pub finance_ledger: ResourceRef,
    pub sales_pipeline: ResourceRef,
    pub hr_compensation: ResourceRef,
    pub legal_hold_room: ResourceRef,
}

/// Build the seeded `acme` authority graph fixture.
pub fn acme_company() -> AcmeAuthorityFixture {
    let tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let admin = PrincipalRef::human_user("user-admin");

    // ---- Personas -------------------------------------------------------
    let junior_engineer = PrincipalRef::human_user("user-junior-eng");
    let junior_engineer_agent =
        PrincipalRef::agent_worker("agent-junior-eng").with_tenant_actor_id("user-junior-eng");
    let lead_engineer = PrincipalRef::human_user("user-lead-eng");
    let finance_analyst = PrincipalRef::human_user("user-finance");
    let sales_rep = PrincipalRef::human_user("user-sales");
    let hr_partner = PrincipalRef::human_user("user-hr");
    let executive = PrincipalRef::human_user("user-exec");
    let support_agent = PrincipalRef::human_user("user-support");

    // ---- Organization units --------------------------------------------
    let engineering = unit(
        "engineering",
        "Engineering",
        OrganizationUnitKind::Department,
        None,
    );
    let junior_eng = unit(
        "junior-engineering",
        "Junior Engineering",
        OrganizationUnitKind::RoleDomain,
        Some("engineering"),
    );
    let lead_eng = unit(
        "lead-engineering",
        "Lead Engineering",
        OrganizationUnitKind::RoleDomain,
        Some("engineering"),
    );
    let finance = unit("finance", "Finance", OrganizationUnitKind::Department, None);
    let sales = unit("sales", "Sales", OrganizationUnitKind::Department, None);
    let hr = unit("hr", "People", OrganizationUnitKind::Department, None);
    let executive_unit = unit(
        "executive",
        "Executive",
        OrganizationUnitKind::ExecutiveGroup,
        None,
    );
    let support = unit("support", "Support", OrganizationUnitKind::Department, None);

    // ---- Resources ------------------------------------------------------
    let engineering_repo = resource(
        ResourceKind::Repository,
        "eng-tandem",
        "engineering",
        "Engineering",
    );
    let internal_architecture_doc = resource(
        ResourceKind::Document,
        "internal-architecture",
        "engineering",
        "Engineering",
    );
    let engineering_secret = resource(
        ResourceKind::SecretProviderCredential,
        "prod-signing-key",
        "engineering",
        "Engineering",
    );
    let finance_ledger = resource(
        ResourceKind::DataStore,
        "finance-ledger",
        "finance",
        "Finance",
    );
    let sales_pipeline = resource(ResourceKind::DataStore, "sales-pipeline", "sales", "Sales");
    let hr_compensation = resource(ResourceKind::Document, "compensation-plan", "hr", "People");
    let legal_hold_room = resource(ResourceKind::DataRoom, "litigation-2026", "legal", "Legal");
    let org_wide = ResourceRef::new(ORG_ID, "*", ResourceKind::Organization, ORG_ID);

    // ---- Memberships ----------------------------------------------------
    let memberships = vec![
        membership("m-junior-eng", &junior_eng, &junior_engineer),
        membership("m-junior-eng-agent", &junior_eng, &junior_engineer_agent),
        membership("m-lead-eng", &lead_eng, &lead_engineer),
        membership("m-finance", &finance, &finance_analyst),
        membership("m-sales", &sales, &sales_rep),
        membership("m-hr", &hr, &hr_partner),
        membership("m-exec", &executive_unit, &executive),
        membership("m-support", &support, &support_agent),
    ];

    // ---- Unit access grants --------------------------------------------
    // Engineering department: shared source code (inherited by junior + lead).
    let g_eng_repo = unit_grant("g-eng-repo", &engineering, &engineering_repo)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::SourceCode, DataClass::Internal]);
    // Lead engineering only: internal architecture + production credentials.
    let g_lead_architecture =
        unit_grant("g-lead-architecture", &lead_eng, &internal_architecture_doc)
            .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
            .with_data_classes(vec![
                DataClass::Restricted,
                DataClass::Confidential,
                DataClass::Internal,
            ]);
    let g_lead_secret = unit_grant("g-lead-secret", &lead_eng, &engineering_secret)
        .with_permissions(vec![
            AccessPermission::View,
            AccessPermission::Read,
            AccessPermission::Execute,
        ])
        .with_data_classes(vec![DataClass::Credential]);
    // Finance department: financial records.
    let g_finance_ledger = unit_grant("g-finance-ledger", &finance, &finance_ledger)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::FinancialRecord, DataClass::Confidential]);
    // Sales department: customer pipeline.
    let g_sales_pipeline = unit_grant("g-sales-pipeline", &sales, &sales_pipeline)
        .with_permissions(vec![
            AccessPermission::View,
            AccessPermission::Read,
            AccessPermission::Edit,
        ])
        .with_data_classes(vec![DataClass::CustomerData, DataClass::Internal]);
    // HR department: compensation.
    let g_hr_comp = unit_grant("g-hr-comp", &hr, &hr_compensation)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::Confidential, DataClass::Executive]);
    // Executive: org-wide read across sensitive classes...
    let g_exec_org_wide = unit_grant("g-exec-org-wide", &executive_unit, &org_wide)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![
            DataClass::Internal,
            DataClass::Confidential,
            DataClass::Restricted,
            DataClass::Executive,
            DataClass::FinancialRecord,
        ]);
    // ...but a legal hold denies even the executive (segregation of duties).
    let g_exec_legal_hold_deny = unit_grant("g-exec-legal-hold", &executive_unit, &legal_hold_room)
        .with_effect(AccessEffect::Deny)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::Restricted]);

    // ---- Direct grants --------------------------------------------------
    // A finance analyst is temporarily shared the internal architecture doc so
    // they can reconcile an engineering spend report; the share expires.
    let temporary_share = ScopedGrant::new(
        "share-finance-architecture",
        finance_analyst.clone(),
        internal_architecture_doc.clone(),
        GrantSource::Delegation,
    )
    .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
    .with_data_classes(vec![DataClass::Restricted, DataClass::Internal])
    .with_delegation_id("share-finance-architecture")
    .with_expires_at_ms(SHARE_EXPIRES_AT_MS);

    let mut graph = IntraTenantAuthorityGraph::new(tenant.clone());
    graph.extend_units(vec![
        engineering,
        junior_eng,
        lead_eng,
        finance,
        sales,
        hr,
        executive_unit,
        support,
    ]);
    graph.extend_memberships(memberships);
    graph.extend_unit_access_grants(vec![
        g_eng_repo,
        g_lead_architecture,
        g_lead_secret,
        g_finance_ledger,
        g_sales_pipeline,
        g_hr_comp,
        g_exec_org_wide,
        g_exec_legal_hold_deny,
    ]);
    graph.extend_direct_grants(vec![temporary_share]);

    let _ = admin; // reserved for created_by attribution in future seeds.

    AcmeAuthorityFixture {
        tenant_context: tenant,
        graph,
        junior_engineer,
        junior_engineer_agent,
        lead_engineer,
        finance_analyst,
        sales_rep,
        hr_partner,
        executive,
        support_agent,
        engineering_repo,
        internal_architecture_doc,
        engineering_secret,
        finance_ledger,
        sales_pipeline,
        hr_compensation,
        legal_hold_room,
    }
}

fn unit(
    unit_id: &str,
    display_name: &str,
    kind: OrganizationUnitKind,
    parent_unit_id: Option<&str>,
) -> OrganizationUnit {
    let tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let admin = PrincipalRef::human_user("user-admin");
    let mut unit =
        OrganizationUnit::active(unit_id, tenant, display_name, kind, admin, BASE_NOW_MS)
            .with_taxonomy_id(TAXONOMY_ID);
    if let Some(parent) = parent_unit_id {
        unit = unit.with_parent_unit_id(parent);
    }
    unit
}

fn membership(
    membership_id: &str,
    unit: &OrganizationUnit,
    member: &PrincipalRef,
) -> OrganizationUnitMembership {
    OrganizationUnitMembership::active(
        membership_id,
        TenantContext::explicit(ORG_ID, WORKSPACE_ID, None),
        unit.principal_ref(),
        member.clone(),
        OrganizationUnitMembershipSource::Direct,
        BASE_NOW_MS,
    )
}

fn unit_grant(
    grant_id: &str,
    unit: &OrganizationUnit,
    resource: &ResourceRef,
) -> OrganizationUnitAccessGrant {
    OrganizationUnitAccessGrant::active(
        grant_id,
        TenantContext::explicit(ORG_ID, WORKSPACE_ID, None),
        unit.principal_ref(),
        resource.clone(),
        BASE_NOW_MS,
    )
}

fn resource(
    kind: ResourceKind,
    resource_id: &str,
    department_id: &str,
    department_name: &str,
) -> ResourceRef {
    ResourceRef::new(ORG_ID, WORKSPACE_ID, kind, resource_id).with_parent_path(vec![
        ResourcePathSegment::named(ResourceKind::Department, department_id, department_name),
    ])
}
