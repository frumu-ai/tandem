//! Fixture-load + golden reachable-set tests for the ACME governance demo
//! (TAN-655).
//!
//! Two guarantees:
//! * **Fixture-load** — every memory row is department- and data-class-tagged,
//!   every tool's security descriptor is shaped so the platform's own classifier
//!   assigns the intended [`ToolRiskTier`], and the Slack-user → unit map lines
//!   up with the seeded units.
//! * **Golden reachable set** — for the fixed demo prompt, each profile reaches a
//!   specific, divergent set of memory rows and tools. The snapshot is committed
//!   as `acme_reachable_sets.golden.json`; regenerate it with
//!   `BLESS_ACME_DEMO_GOLDEN=1 cargo test -p tandem-server acme_demo`.

use std::collections::BTreeSet;

use tandem_core::tool_schema_risk_tier;
use tandem_types::{DataClass, ToolRiskTier};

use super::*;

const GOLDEN: &str = include_str!("acme_reachable_sets.golden.json");

#[test]
fn every_profile_resolves_to_a_seeded_unit() {
    let dataset = acme_demo_dataset();
    assert_eq!(dataset.profiles.len(), 5, "the demo has five profiles");
    for profile in &dataset.profiles {
        // Slack-user → unit map agrees with the profile's own unit id.
        assert_eq!(
            slack_user_to_unit_id(profile.slack_user_id),
            Some(profile.unit_id),
            "slack map must resolve {} to its unit",
            profile.slack_user_id
        );
        // The canonical unit principal is `{taxonomy}/{unit_id}`.
        assert_eq!(
            profile.unit_principal.id,
            format!("{DEMO_TAXONOMY_ID}/{}", profile.unit_id),
        );
        // The requester principal is the resolved channel actor.
        assert_eq!(
            profile.principal.id,
            format!("channel:slack:{}", profile.slack_user_id),
        );
        // A seeded unit + membership exists for the profile.
        assert!(
            dataset
                .graph
                .units
                .iter()
                .any(|unit| unit.principal_ref() == profile.unit_principal),
            "unit missing for {}",
            profile.unit_id
        );
        assert!(
            dataset
                .graph
                .memberships
                .iter()
                .any(|m| m.member == profile.principal && m.unit == profile.unit_principal),
            "membership missing for {}",
            profile.slack_user_id
        );
    }
    assert_eq!(slack_user_to_unit_id("U_UNKNOWN"), None, "fail closed");
}

#[test]
fn memory_rows_are_department_and_data_class_tagged() {
    let dataset = acme_demo_dataset();
    let mut owning_units: BTreeSet<String> = dataset
        .profiles
        .iter()
        .map(DemoProfile::owner_org_unit_id)
        .collect();
    // The credential row is owned by a real-but-unstaffed unit on purpose.
    owning_units.insert(format!("{DEMO_TAXONOMY_ID}/{DEMO_UNSTAFFED_UNIT}"));

    for row in &dataset.memory_rows {
        // Department tag is a canonical `{taxonomy}/{unit_id}`.
        assert!(
            row.owner_org_unit_id
                .starts_with(&format!("{DEMO_TAXONOMY_ID}/")),
            "row {} not department-tagged: {}",
            row.id,
            row.owner_org_unit_id
        );
        assert!(
            owning_units.contains(&row.owner_org_unit_id),
            "row {} owned by an unknown unit {}",
            row.id,
            row.owner_org_unit_id
        );
        // The put metadata carries the department key the read filter gates on and
        // the class under `classification` — the exact key governed tenant-local
        // reads derive the data class from — so the tag can't drift on write.
        let metadata = row.put_metadata();
        assert_eq!(
            metadata
                .get(tandem_memory::types::OWNER_ORG_UNIT_METADATA_KEY)
                .and_then(|value| value.as_str()),
            Some(row.owner_org_unit_id.as_str()),
        );
        assert_eq!(
            metadata.get("classification"),
            Some(&serde_json::to_value(row.data_class).unwrap()),
        );
        // The governed reader must recover the row's exact class from the metadata.
        assert_eq!(
            tandem_memory::types::data_class_from_metadata(Some(&metadata)),
            Some(row.data_class),
            "row {} class must round-trip through classification",
            row.id
        );
    }

    // The financial detail lives only under Finance; source only under Engineering.
    for row in &dataset.memory_rows {
        if row.data_class == DataClass::FinancialRecord {
            assert_eq!(
                row.owner_org_unit_id,
                format!("{DEMO_TAXONOMY_ID}/finance"),
                "FinancialRecord row {} must be finance-owned",
                row.id
            );
        }
        if row.data_class == DataClass::SourceCode {
            assert_eq!(
                row.owner_org_unit_id,
                format!("{DEMO_TAXONOMY_ID}/engineering"),
                "SourceCode row {} must be engineering-owned",
                row.id
            );
        }
    }
}

#[test]
fn tool_set_carries_the_expected_risk_tiers() {
    let dataset = acme_demo_dataset();
    for tool in &dataset.tools {
        // The platform's own classifier agrees with the declared tier.
        assert_eq!(
            tool_schema_risk_tier(&tool.schema),
            tool.expected_risk_tier,
            "risk tier drift on {}",
            tool.schema.name
        );
    }

    let tier = |name: &str| -> ToolRiskTier {
        dataset
            .tools
            .iter()
            .find(|tool| tool.schema.name == name)
            .map(|tool| tool.expected_risk_tier)
            .unwrap_or_else(|| panic!("missing demo tool {name}"))
    };
    // The tiers the acceptance criteria name explicitly.
    assert_eq!(
        tier("mcp.crm.search_accounts"),
        ToolRiskTier::CustomerDataAccess
    );
    assert_eq!(
        tier("mcp.invoices.read_invoices"),
        ToolRiskTier::FinancialRecordAccess
    );
    assert_eq!(
        tier("mcp.contracts.read_contracts"),
        ToolRiskTier::FinancialRecordAccess
    );
    assert_eq!(tier("mcp.email.send_email"), ToolRiskTier::ExternalSend);

    // The financial + external-send tools are approval-gated by default.
    for name in [
        "mcp.invoices.read_invoices",
        "mcp.contracts.read_contracts",
        "mcp.email.send_email",
    ] {
        assert!(
            dataset
                .tools
                .iter()
                .find(|tool| tool.schema.name == name)
                .unwrap()
                .approval_required(),
            "{name} must be approval-gated"
        );
    }
    // A plain read tool is not approval-gated.
    assert!(!dataset
        .tools
        .iter()
        .find(|tool| tool.schema.name == "mcp.github.read_repo")
        .unwrap()
        .approval_required());
}

#[test]
fn memory_reads_are_department_membership_gated() {
    let dataset = acme_demo_dataset();
    let now = DEMO_BASE_NOW_MS;
    let profile = |slack: &str| dataset.profile_for_slack_user(slack).unwrap().clone();
    let row = |id: &str| {
        dataset
            .memory_rows
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .clone()
    };

    let finance = profile("U_FINANCE");
    let sales = profile("U_SALES");
    let engineering = profile("U_ENG");
    let leadership = profile("U_LEADER");
    let contractor = profile("U_CONTRACTOR");
    let invoice = row("finance_invoice_acme");
    let crm = row("sales_crm_acme");
    let secret = row("shared_signing_key");
    let project = row("contractor_project_x");

    // Finance reads its own invoices; nobody else does — not a member of finance.
    assert!(profile_can_read_memory(&finance, &invoice, now));
    for other in [&sales, &engineering, &leadership, &contractor] {
        assert!(
            !profile_can_read_memory(other, &invoice, now),
            "{} must not read the finance invoice",
            other.slack_user_id
        );
    }

    // Sales reads its own CRM row; contractor (single-project) cannot.
    assert!(profile_can_read_memory(&sales, &crm, now));
    assert!(!profile_can_read_memory(&contractor, &crm, now));

    // The unstaffed credential row is unreachable to every profile.
    for who in [&sales, &engineering, &finance, &leadership, &contractor] {
        assert!(
            !profile_can_read_memory(who, &secret, now),
            "{} must not read the shared credential",
            who.slack_user_id
        );
    }

    // The contractor's world is exactly its assigned project.
    assert!(profile_can_read_memory(&contractor, &project, now));
    let contractor_reach = profile_reachable_set(&dataset, &contractor, now);
    assert_eq!(
        contractor_reach["reachable_memory"],
        serde_json::json!(["contractor_project_x"]),
    );
}

#[test]
fn enterprise_clearance_does_not_bypass_department_membership() {
    // The two-layer model: Leadership holds a broad enterprise resource grant that
    // *clears* the customer-data class the Sales CRM row carries, yet the memory
    // filter still denies the specific row because Leadership is not a member of
    // the Sales department. Clearance is necessary but not sufficient.
    let dataset = acme_demo_dataset();
    let now = DEMO_BASE_NOW_MS;
    let leadership = dataset.profile_for_slack_user("U_LEADER").unwrap().clone();
    let crm = dataset
        .memory_rows
        .iter()
        .find(|row| row.id == "sales_crm_acme")
        .unwrap()
        .clone();

    assert!(
        profile_holds_resource_grant(&dataset, &leadership, &crm.resource, crm.data_class, now),
        "leadership should hold the org-wide clearance for customer data"
    );
    assert!(
        !profile_can_read_memory(&leadership, &crm, now),
        "but the sales-owned memory row stays behind the department boundary"
    );
    // Leadership reaches only its own department's memory (the exec summary).
    let reach = profile_reachable_set(&dataset, &leadership, now);
    assert_eq!(
        reach["reachable_memory"],
        serde_json::json!(["leadership_board_summary"]),
    );

    // Engineering's finance denial holds at the clearance layer too (explicit deny).
    let engineering = dataset.profile_for_slack_user("U_ENG").unwrap().clone();
    let invoice = dataset
        .memory_rows
        .iter()
        .find(|row| row.id == "finance_invoice_acme")
        .unwrap()
        .clone();
    assert!(!profile_holds_resource_grant(
        &dataset,
        &engineering,
        &invoice.resource,
        invoice.data_class,
        now
    ));
}

#[test]
fn tool_reachability_diverges_by_department() {
    let dataset = acme_demo_dataset();
    let now = DEMO_BASE_NOW_MS;
    let profile = |slack: &str| dataset.profile_for_slack_user(slack).unwrap().clone();
    let finance = profile("U_FINANCE");
    let sales = profile("U_SALES");
    let engineering = profile("U_ENG");
    let contractor = profile("U_CONTRACTOR");

    // Only Finance reaches the financial tools; only Engineering reaches the repo;
    // email-send is offered to Sales but not the Contractor.
    let can_use = |profile: &DemoProfile, name: &str| {
        let tool = dataset
            .tools
            .iter()
            .find(|tool| tool.schema.name == name)
            .unwrap();
        profile_can_use_tool(&dataset, profile, tool, now)
    };
    assert!(can_use(&finance, "mcp.invoices.read_invoices"));
    assert!(!can_use(&sales, "mcp.invoices.read_invoices"));
    assert!(can_use(&engineering, "mcp.github.read_repo"));
    assert!(!can_use(&finance, "mcp.github.read_repo"));
    assert!(can_use(&sales, "mcp.email.send_email"));
    assert!(!can_use(&contractor, "mcp.email.send_email"));
}

#[test]
fn per_profile_reachable_set_matches_golden() {
    let dataset = acme_demo_dataset();
    let snapshot = reachable_set_snapshot(&dataset, DEMO_BASE_NOW_MS);
    let rendered = format!("{}\n", serde_json::to_string_pretty(&snapshot).unwrap());

    if std::env::var("BLESS_ACME_DEMO_GOLDEN").is_ok() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/acme_demo/acme_reachable_sets.golden.json"
        );
        std::fs::write(path, &rendered).unwrap();
        return;
    }

    let expected: serde_json::Value = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(
        snapshot, expected,
        "reachable-set snapshot drifted from the golden; \
         re-bless with BLESS_ACME_DEMO_GOLDEN=1 if this change is intended"
    );
}
