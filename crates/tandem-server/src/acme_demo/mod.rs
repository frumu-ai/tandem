//! ACME governance-demo dataset (TAN-655).
//!
//! Seeds the "Department-Scoped Slack Agent — Governance Demo" storyline: five
//! Slack requester profiles resolved to organization units inside one tenant
//! (`org = acme`, `workspace = hq`, taxonomy `department`), a set of memory rows
//! tagged by owning department + [`DataClass`], and a small MCP tool set tagged
//! with [`ToolRiskTier`]s. The point of the dataset is that the *same* ACME
//! question yields a *different* reachable memory/tool set per department, so the
//! demo can show governance diverging by requester — enforced by the real
//! intra-tenant authority graph, not by scripted branching.
//!
//! This is the executable companion to the design pinned in
//! `docs/DEPARTMENT_SCOPED_SLACK_DEMO_PROFILES.md` (TAN-653). It is intentionally
//! a pure value builder — no I/O, no live server — so tests, eval seeds, and the
//! forthcoming e2e harness (TAN-667) can all consume the same handles:
//!
//! * [`acme_demo_dataset`] returns the graph, profiles, memory rows, and tools.
//! * [`slack_user_to_unit_id`] is the Slack-user → unit map the TAN-652 resolver
//!   consults to populate a verified context's `org_units`.
//! * [`profile_reachable_set`] renders, for one profile, the resources and tools
//!   it can reach — the shape the golden snapshot in `tests.rs` pins.
//!
//! The existing `tandem_enterprise_contract::authority::fixtures::acme_company`
//! fixture is a *different* graph (workspace `acme`, taxonomy `org`, junior/lead
//! personas) used by the intra-tenant authority unit tests; this dataset is the
//! demo-specific five-profile world and does not replace it.

use serde_json::{json, Value};

use tandem_core::{any_policy_matches, tool_schema_risk_tier};
use tandem_enterprise_contract::authority::{
    AuthorityAccessRequest, IntraTenantAuthorityGraph,
};
use tandem_memory::types::OWNER_ORG_UNIT_METADATA_KEY;
use tandem_types::{
    AccessEffect, AccessPermission, DataClass, OrganizationUnit, OrganizationUnitAccessGrant,
    OrganizationUnitKind, OrganizationUnitMembership, OrganizationUnitMembershipSource,
    PrincipalRef, ResourceKind, ResourceRef, ToolRiskTier, ToolSchema, ToolSecurityDescriptor,
    TenantContext,
};

#[cfg(test)]
mod tests;

/// Organization id for the demo tenant.
pub const DEMO_ORG_ID: &str = "acme";
/// Workspace id for the demo tenant (single workspace; departments are units).
pub const DEMO_WORKSPACE_ID: &str = "hq";
/// Taxonomy id shared by every demo organization unit. The canonical unit
/// principal is `organization_unit("{taxonomy}/{unit_id}")`, e.g.
/// `department/sales` — the exact shape `OrganizationUnit::principal_ref` and the
/// admin path emit, so the authority graph resolves memberships by equality.
pub const DEMO_TAXONOMY_ID: &str = "department";
/// Baseline "now" for the dataset, in epoch milliseconds. Memberships and grants
/// are active at this instant, so evaluations are deterministic.
pub const DEMO_BASE_NOW_MS: u64 = 1_700_000_000_000;

/// The demo prompt every profile asks. Reachability — not the model's answer — is
/// what the demo governs, so the prompt is fixed and the divergence comes from
/// which resources/tools each department can reach.
pub const DEMO_PROMPT: &str =
    "What's the latest on the Hooli account, and can you follow up with them?";

/// One of the five demo requester profiles.
#[derive(Debug, Clone)]
pub struct DemoProfile {
    /// Slack user id, e.g. `U_SALES`.
    pub slack_user_id: &'static str,
    /// Server-resolved channel actor id, e.g. `channel:slack:U_SALES`.
    pub actor_id: String,
    /// Org-unit id, e.g. `sales`.
    pub unit_id: &'static str,
    /// Human-readable department label.
    pub display_name: &'static str,
    /// The unit's kind (department / executive_group / contractor_group).
    pub kind: OrganizationUnitKind,
    /// The requester principal (`channel:slack:{user}` human actor).
    pub principal: PrincipalRef,
    /// The canonical org-unit principal (`department/{unit_id}`).
    pub unit_principal: PrincipalRef,
}

impl DemoProfile {
    /// The `owner_org_unit_id` string this department stamps on its memory
    /// (`{taxonomy}/{unit_id}`) — the same value `active_org_unit` yields.
    pub fn owner_org_unit_id(&self) -> String {
        format!("{DEMO_TAXONOMY_ID}/{}", self.unit_id)
    }
}

/// A memory row in the demo, tagged by owning department and data class.
#[derive(Debug, Clone)]
pub struct DemoMemoryRow {
    /// Stable id for the row (used in the golden snapshot).
    pub id: &'static str,
    /// Owning department, `{taxonomy}/{unit_id}` (stamped as `owner_org_unit_id`).
    pub owner_org_unit_id: String,
    /// The row's data class — governs which grants can read it.
    pub data_class: DataClass,
    /// The resource the row is about (its `resource_id` is the demo "domain").
    pub resource: ResourceRef,
    /// The collecting subject (a department member's channel actor).
    pub subject: String,
    /// Short human summary of the memory content.
    pub summary: &'static str,
}

impl DemoMemoryRow {
    /// The demo "domain" this row belongs to (its resource id, e.g. `crm`).
    pub fn domain(&self) -> &str {
        &self.resource.resource_id
    }

    /// Metadata a governed `memory_put` would carry for this row, so the seed
    /// loader / harness stamps the exact `owner_org_unit_id` the read filter and
    /// SQL column key on. Kept in one place so the tag can't drift from the
    /// authority grants.
    pub fn put_metadata(&self) -> Value {
        json!({
            OWNER_ORG_UNIT_METADATA_KEY: self.owner_org_unit_id,
            "data_class": self.data_class,
            "demo_row_id": self.id,
        })
    }
}

/// A demo MCP tool tagged with its expected risk tier.
#[derive(Debug, Clone)]
pub struct DemoTool {
    /// The tool schema (name + security descriptor) as registered.
    pub schema: ToolSchema,
    /// The risk tier the platform's own classifier assigns this tool. Asserted in
    /// `tests.rs` so the descriptor stays honest.
    pub expected_risk_tier: ToolRiskTier,
}

impl DemoTool {
    /// Whether invoking this tool requires approval by default (its tier's rule).
    pub fn approval_required(&self) -> bool {
        self.expected_risk_tier.approval_required_by_default()
    }
}

/// The fully-seeded ACME governance-demo dataset.
#[derive(Debug, Clone)]
pub struct AcmeDemoDataset {
    pub tenant_context: TenantContext,
    pub graph: IntraTenantAuthorityGraph,
    pub profiles: Vec<DemoProfile>,
    pub memory_rows: Vec<DemoMemoryRow>,
    pub tools: Vec<DemoTool>,
}

impl AcmeDemoDataset {
    /// Look up a profile by Slack user id.
    pub fn profile_for_slack_user(&self, slack_user_id: &str) -> Option<&DemoProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.slack_user_id == slack_user_id)
    }
}

/// Slack-user → org-unit-id map the ingress resolver (TAN-652) consults to place
/// a channel principal in its department. Returns `None` for users outside the
/// demo allowlist (fail closed — no department, no scoped access).
pub fn slack_user_to_unit_id(slack_user_id: &str) -> Option<&'static str> {
    match slack_user_id {
        "U_SALES" => Some("sales"),
        "U_ENG" => Some("engineering"),
        "U_FINANCE" => Some("finance"),
        "U_LEADER" => Some("leadership"),
        "U_CONTRACTOR" => Some("contractor_acme_x"),
        _ => None,
    }
}

fn demo_tenant() -> TenantContext {
    TenantContext::explicit(DEMO_ORG_ID, DEMO_WORKSPACE_ID, None)
}

fn profile(
    slack_user_id: &'static str,
    unit_id: &'static str,
    display_name: &'static str,
    kind: OrganizationUnitKind,
) -> DemoProfile {
    let actor_id = format!("channel:slack:{slack_user_id}");
    DemoProfile {
        slack_user_id,
        actor_id: actor_id.clone(),
        unit_id,
        display_name,
        kind,
        principal: PrincipalRef::human_user(actor_id),
        unit_principal: PrincipalRef::organization_unit(format!(
            "{DEMO_TAXONOMY_ID}/{unit_id}"
        )),
    }
}

fn unit(profile: &DemoProfile) -> OrganizationUnit {
    OrganizationUnit::active(
        profile.unit_id,
        demo_tenant(),
        profile.display_name,
        profile.kind,
        PrincipalRef::human_user("user-admin"),
        DEMO_BASE_NOW_MS,
    )
    .with_taxonomy_id(DEMO_TAXONOMY_ID)
}

fn membership(profile: &DemoProfile) -> OrganizationUnitMembership {
    OrganizationUnitMembership::active(
        format!("m-{}", profile.unit_id),
        demo_tenant(),
        profile.unit_principal.clone(),
        profile.principal.clone(),
        OrganizationUnitMembershipSource::Direct,
        DEMO_BASE_NOW_MS,
    )
}

/// A demo resource in a given "domain". Departments own distinct domains
/// (`crm`, `invoices`, …) so a grant on one domain never widens into another.
fn resource(kind: ResourceKind, domain: &str) -> ResourceRef {
    ResourceRef::new(DEMO_ORG_ID, DEMO_WORKSPACE_ID, kind, domain)
}

/// An org-wide resource, used by the leadership cross-functional read grant.
fn org_wide_resource() -> ResourceRef {
    ResourceRef::new(DEMO_ORG_ID, "*", ResourceKind::Organization, DEMO_ORG_ID)
}

fn allow_grant(
    grant_id: &str,
    unit: &PrincipalRef,
    resource: ResourceRef,
    data_classes: Vec<DataClass>,
) -> OrganizationUnitAccessGrant {
    OrganizationUnitAccessGrant::active(grant_id, demo_tenant(), unit.clone(), resource, DEMO_BASE_NOW_MS)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(data_classes)
}

fn deny_grant(
    grant_id: &str,
    unit: &PrincipalRef,
    resource: ResourceRef,
    data_classes: Vec<DataClass>,
) -> OrganizationUnitAccessGrant {
    OrganizationUnitAccessGrant::active(grant_id, demo_tenant(), unit.clone(), resource, DEMO_BASE_NOW_MS)
        .with_effect(AccessEffect::Deny)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(data_classes)
}

/// A tool-only grant: carries `tool_patterns` but no readable data class, so it
/// contributes to a department's reachable tool set without granting any
/// resource read (kept separate from resource grants for clarity).
fn tool_grant(
    grant_id: &str,
    unit: &PrincipalRef,
    tool_patterns: Vec<String>,
) -> OrganizationUnitAccessGrant {
    OrganizationUnitAccessGrant::active(
        grant_id,
        demo_tenant(),
        unit.clone(),
        org_wide_resource(),
        DEMO_BASE_NOW_MS,
    )
    .with_permissions(vec![AccessPermission::Execute])
    .with_tool_patterns(tool_patterns)
}

fn memory_row(
    id: &'static str,
    owner: &DemoProfile,
    domain_kind: ResourceKind,
    domain: &'static str,
    data_class: DataClass,
    summary: &'static str,
) -> DemoMemoryRow {
    DemoMemoryRow {
        id,
        owner_org_unit_id: owner.owner_org_unit_id(),
        data_class,
        resource: resource(domain_kind, domain),
        subject: owner.actor_id.clone(),
        summary,
    }
}

fn patterns(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

/// A read tool that carries a data class; its risk tier is *derived* by the
/// platform classifier from that class + name, proving the descriptor is shaped
/// so governance tags it correctly.
fn read_tool(name: &str, data_classes: Vec<DataClass>, expected: ToolRiskTier) -> DemoTool {
    let schema = ToolSchema {
        name: name.to_string(),
        description: format!("ACME demo tool {name} (read)"),
        input_schema: json!({"type": "object"}),
        capabilities: Default::default(),
        security: ToolSecurityDescriptor {
            required_permissions: vec![AccessPermission::Read],
            data_classes,
            ..Default::default()
        },
    };
    DemoTool {
        schema,
        expected_risk_tier: expected,
    }
}

/// A tool whose risk tier is stamped *explicitly* on the descriptor, for tools
/// whose name/data-class the generic classifier would otherwise mis-tier (e.g. a
/// financial *read* whose name would trip the money-movement heuristic).
fn tagged_tool(
    name: &str,
    data_classes: Vec<DataClass>,
    risk_tier: ToolRiskTier,
) -> DemoTool {
    let schema = ToolSchema {
        name: name.to_string(),
        description: format!("ACME demo tool {name}"),
        input_schema: json!({"type": "object"}),
        capabilities: Default::default(),
        security: ToolSecurityDescriptor {
            required_permissions: vec![AccessPermission::Read],
            data_classes,
            risk_tier: Some(risk_tier),
            ..Default::default()
        },
    };
    DemoTool {
        schema,
        expected_risk_tier: risk_tier,
    }
}

/// An external-send tool; its tier is derived from the name (`send`) → the
/// `ExternalSend` tier, which is approval-gated by default.
fn send_tool(name: &str) -> DemoTool {
    let schema = ToolSchema {
        name: name.to_string(),
        description: format!("ACME demo tool {name} (external send)"),
        input_schema: json!({"type": "object"}),
        capabilities: Default::default(),
        security: ToolSecurityDescriptor {
            external_side_effect: true,
            ..Default::default()
        },
    };
    DemoTool {
        schema,
        expected_risk_tier: ToolRiskTier::ExternalSend,
    }
}

/// Build the seeded ACME governance-demo dataset.
pub fn acme_demo_dataset() -> AcmeDemoDataset {
    let tenant = demo_tenant();

    // ---- Profiles -------------------------------------------------------
    let sales = profile("U_SALES", "sales", "Sales", OrganizationUnitKind::Department);
    let engineering = profile(
        "U_ENG",
        "engineering",
        "Engineering",
        OrganizationUnitKind::Department,
    );
    let finance = profile(
        "U_FINANCE",
        "finance",
        "Finance",
        OrganizationUnitKind::Department,
    );
    let leadership = profile(
        "U_LEADER",
        "leadership",
        "Leadership",
        OrganizationUnitKind::ExecutiveGroup,
    );
    let contractor = profile(
        "U_CONTRACTOR",
        "contractor_acme_x",
        "ACME-X Contractor",
        OrganizationUnitKind::ContractorGroup,
    );
    let profiles = vec![
        sales.clone(),
        engineering.clone(),
        finance.clone(),
        leadership.clone(),
        contractor.clone(),
    ];

    // ---- Memory rows (department + data-class tagged) -------------------
    let memory_rows = vec![
        // Sales — customer-facing, no financial detail.
        memory_row(
            "sales_crm_hooli",
            &sales,
            ResourceKind::DataStore,
            "crm",
            DataClass::CustomerData,
            "Hooli renewal in flight: primary contact Gavin Belson, expansion interest in seats.",
        ),
        memory_row(
            "sales_support_theme",
            &sales,
            ResourceKind::DataStore,
            "support",
            DataClass::Confidential,
            "Top support theme this quarter for Hooli: onboarding friction on SSO.",
        ),
        memory_row(
            "sales_risk_flag",
            &sales,
            ResourceKind::DataStore,
            "risk",
            DataClass::Confidential,
            "Account risk note: Hooli champion changed roles; relationship risk medium.",
        ),
        // Engineering — source + delivery, no financial detail.
        memory_row(
            "eng_github_auth",
            &engineering,
            ResourceKind::Repository,
            "github",
            DataClass::SourceCode,
            "auth-service main: JWT rotation landed in PR #4821; Hooli SSO integration branch open.",
        ),
        memory_row(
            "eng_linear_milestone",
            &engineering,
            ResourceKind::DataStore,
            "linear",
            DataClass::Internal,
            "Linear: Hooli SSO epic in progress, targeted for the M2 milestone.",
        ),
        memory_row(
            "eng_incident_sev2",
            &engineering,
            ResourceKind::DataStore,
            "incidents",
            DataClass::Internal,
            "Incident log: SEV-2 cache stampede affecting Hooli tenant, mitigated 2026-06-14.",
        ),
        // Finance — the FinancialRecord department.
        memory_row(
            "finance_invoice_hooli",
            &finance,
            ResourceKind::DataStore,
            "invoices",
            DataClass::FinancialRecord,
            "Invoice INV-2043: Hooli, $120k, net-30, currently unpaid (7 days overdue).",
        ),
        memory_row(
            "finance_payment_run",
            &finance,
            ResourceKind::DataStore,
            "payments",
            DataClass::FinancialRecord,
            "Payment run 2026-07-01: $412k disbursed; Hooli refund of $8k pending approval.",
        ),
        memory_row(
            "finance_contract_hooli",
            &finance,
            ResourceKind::DataStore,
            "contracts",
            DataClass::FinancialRecord,
            "Hooli MSA: auto-renew on 2026-09-01 with a 14% price uplift clause.",
        ),
        // Leadership — cross-functional summary at Confidential (not raw finance).
        memory_row(
            "leadership_board_summary",
            &leadership,
            ResourceKind::Document,
            "board",
            DataClass::Confidential,
            "Exec summary: Hooli is a top-5 account; renewal on track, one open SEV and a payment slip.",
        ),
        // A shared credential nobody should read raw (incl. leadership).
        memory_row(
            "shared_signing_key",
            &leadership,
            ResourceKind::SecretProviderCredential,
            "secrets",
            DataClass::Credential,
            "Production signing key for the Hooli tenant; rotation scheduled 2026-08.",
        ),
        // Contractor — a single assigned project, nothing else.
        memory_row(
            "contractor_project_x",
            &contractor,
            ResourceKind::Project,
            "project-x",
            DataClass::Internal,
            "Project X spec: build the Hooli widget export; scope limited to the export pipeline.",
        ),
    ];

    // ---- Units + memberships -------------------------------------------
    let mut graph = IntraTenantAuthorityGraph::new(tenant.clone());
    graph.extend_units(profiles.iter().map(unit));
    graph.extend_memberships(profiles.iter().map(membership));

    // ---- Resource + tool grants ----------------------------------------
    let read_tools_all = [
        "mcp.crm.*",
        "mcp.support.*",
        "mcp.github.*",
        "mcp.linear.*",
        "mcp.incidents.*",
    ];
    graph.extend_unit_access_grants(vec![
        // Sales: customer data + support/risk; email send (approval-gated).
        allow_grant(
            "g-sales-crm",
            &sales.unit_principal,
            resource(ResourceKind::DataStore, "crm"),
            vec![DataClass::CustomerData, DataClass::Confidential],
        ),
        allow_grant(
            "g-sales-support",
            &sales.unit_principal,
            resource(ResourceKind::DataStore, "support"),
            vec![DataClass::Confidential],
        ),
        allow_grant(
            "g-sales-risk",
            &sales.unit_principal,
            resource(ResourceKind::DataStore, "risk"),
            vec![DataClass::Confidential],
        ),
        tool_grant(
            "g-sales-tools",
            &sales.unit_principal,
            patterns(&["mcp.crm.*", "mcp.support.*", "mcp.email.*"]),
        ),
        // Engineering: source + delivery; explicit deny on finance domains.
        allow_grant(
            "g-eng-github",
            &engineering.unit_principal,
            resource(ResourceKind::Repository, "github"),
            vec![DataClass::SourceCode, DataClass::Internal],
        ),
        allow_grant(
            "g-eng-linear",
            &engineering.unit_principal,
            resource(ResourceKind::DataStore, "linear"),
            vec![DataClass::Internal],
        ),
        allow_grant(
            "g-eng-incidents",
            &engineering.unit_principal,
            resource(ResourceKind::DataStore, "incidents"),
            vec![DataClass::Internal],
        ),
        deny_grant(
            "g-eng-deny-invoices",
            &engineering.unit_principal,
            resource(ResourceKind::DataStore, "invoices"),
            vec![DataClass::FinancialRecord],
        ),
        deny_grant(
            "g-eng-deny-contracts",
            &engineering.unit_principal,
            resource(ResourceKind::DataStore, "contracts"),
            vec![DataClass::FinancialRecord],
        ),
        tool_grant(
            "g-eng-tools",
            &engineering.unit_principal,
            patterns(&["mcp.github.*", "mcp.linear.*", "mcp.incidents.*", "mcp.email.*"]),
        ),
        // Finance: the FinancialRecord department.
        allow_grant(
            "g-finance-invoices",
            &finance.unit_principal,
            resource(ResourceKind::DataStore, "invoices"),
            vec![DataClass::FinancialRecord, DataClass::Confidential],
        ),
        allow_grant(
            "g-finance-payments",
            &finance.unit_principal,
            resource(ResourceKind::DataStore, "payments"),
            vec![DataClass::FinancialRecord, DataClass::Confidential],
        ),
        allow_grant(
            "g-finance-contracts",
            &finance.unit_principal,
            resource(ResourceKind::DataStore, "contracts"),
            vec![DataClass::FinancialRecord, DataClass::Confidential],
        ),
        tool_grant(
            "g-finance-tools",
            &finance.unit_principal,
            patterns(&["mcp.invoices.*", "mcp.contracts.*", "mcp.email.*"]),
        ),
        // Leadership: org-wide read at non-financial classes (finance +
        // credentials are redacted — not listed here, so they fail closed).
        allow_grant(
            "g-leadership-org-wide",
            &leadership.unit_principal,
            org_wide_resource(),
            vec![
                DataClass::Internal,
                DataClass::Confidential,
                DataClass::CustomerData,
                DataClass::SourceCode,
            ],
        ),
        tool_grant(
            "g-leadership-tools",
            &leadership.unit_principal,
            patterns(
                &read_tools_all
                    .iter()
                    .copied()
                    .chain(["mcp.email.*"])
                    .collect::<Vec<_>>(),
            ),
        ),
        // Contractor: only the assigned project.
        allow_grant(
            "g-contractor-project",
            &contractor.unit_principal,
            resource(ResourceKind::Project, "project-x"),
            vec![DataClass::Internal],
        ),
        tool_grant(
            "g-contractor-tools",
            &contractor.unit_principal,
            patterns(&["mcp.projects.x.*"]),
        ),
    ]);

    // ---- Tool set (tagged with risk tiers) -----------------------------
    let tools = vec![
        read_tool(
            "mcp.crm.search_accounts",
            vec![DataClass::CustomerData],
            ToolRiskTier::CustomerDataAccess,
        ),
        read_tool(
            "mcp.support.list_summaries",
            vec![DataClass::Confidential],
            ToolRiskTier::ReadDiscover,
        ),
        read_tool(
            "mcp.github.read_repo",
            vec![DataClass::SourceCode],
            ToolRiskTier::ReadDiscover,
        ),
        read_tool(
            "mcp.linear.list_issues",
            vec![DataClass::Internal],
            ToolRiskTier::ReadDiscover,
        ),
        read_tool(
            "mcp.incidents.list_incidents",
            vec![DataClass::Internal],
            ToolRiskTier::ReadDiscover,
        ),
        tagged_tool(
            "mcp.invoices.read_invoices",
            vec![DataClass::FinancialRecord],
            ToolRiskTier::FinancialRecordAccess,
        ),
        tagged_tool(
            "mcp.contracts.read_contracts",
            vec![DataClass::FinancialRecord],
            ToolRiskTier::FinancialRecordAccess,
        ),
        read_tool(
            "mcp.projects.x.read_spec",
            vec![DataClass::Internal],
            ToolRiskTier::ReadDiscover,
        ),
        send_tool("mcp.email.send_email"),
    ];

    AcmeDemoDataset {
        tenant_context: tenant,
        graph,
        profiles,
        memory_rows,
        tools,
    }
}

/// Whether `profile` can read `row`, per the intra-tenant authority graph, at the
/// row's own data class (Read permission).
pub fn profile_can_read(
    dataset: &AcmeDemoDataset,
    profile: &DemoProfile,
    row: &DemoMemoryRow,
    now_ms: u64,
) -> bool {
    let request = AuthorityAccessRequest::new(
        profile.principal.clone(),
        row.resource.clone(),
        AccessPermission::Read,
        row.data_class,
    );
    dataset.graph.evaluate(&request, now_ms).is_allow()
}

/// Whether `profile` may invoke `tool`, per the union of tool patterns on the
/// department's effective allow grants (glob match on the tool name).
pub fn profile_can_use_tool(
    dataset: &AcmeDemoDataset,
    profile: &DemoProfile,
    tool: &DemoTool,
    now_ms: u64,
) -> bool {
    let allowed_patterns: Vec<String> = dataset
        .graph
        .effective_grants(&profile.principal, now_ms)
        .into_iter()
        .filter(|grant| grant.effect == AccessEffect::Allow)
        .flat_map(|grant| grant.tool_patterns)
        .map(|pattern| pattern.trim().to_ascii_lowercase())
        .filter(|pattern| !pattern.is_empty())
        .collect();
    any_policy_matches(&allowed_patterns, &tool.schema.name.to_ascii_lowercase())
}

/// Render the reachable resource + tool set for one profile — the per-profile
/// shape the golden snapshot pins. Lists are sorted for a stable diff.
pub fn profile_reachable_set(
    dataset: &AcmeDemoDataset,
    profile: &DemoProfile,
    now_ms: u64,
) -> Value {
    let mut resources: Vec<String> = dataset
        .memory_rows
        .iter()
        .filter(|row| profile_can_read(dataset, profile, row, now_ms))
        .map(|row| row.id.to_string())
        .collect();
    resources.sort();

    let mut tools: Vec<Value> = dataset
        .tools
        .iter()
        .filter(|tool| profile_can_use_tool(dataset, profile, tool, now_ms))
        .map(|tool| {
            json!({
                "tool": tool.schema.name,
                "risk_tier": tool_schema_risk_tier(&tool.schema).as_str(),
                "approval_required": tool.approval_required(),
            })
        })
        .collect();
    tools.sort_by(|a, b| a["tool"].as_str().cmp(&b["tool"].as_str()));

    json!({
        "slack_user": profile.slack_user_id,
        "org_unit": profile.owner_org_unit_id(),
        "reachable_memory": resources,
        "reachable_tools": tools,
    })
}

/// Render the full per-profile reachable-set snapshot for [`DEMO_PROMPT`].
pub fn reachable_set_snapshot(dataset: &AcmeDemoDataset, now_ms: u64) -> Value {
    json!({
        "prompt": DEMO_PROMPT,
        "tenant": {
            "org_id": DEMO_ORG_ID,
            "workspace_id": DEMO_WORKSPACE_ID,
            "taxonomy_id": DEMO_TAXONOMY_ID,
        },
        "profiles": dataset
            .profiles
            .iter()
            .map(|profile| profile_reachable_set(dataset, profile, now_ms))
            .collect::<Vec<_>>(),
    })
}
