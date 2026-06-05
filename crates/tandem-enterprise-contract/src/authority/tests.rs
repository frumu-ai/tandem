use super::fixtures::{self, SHARE_EXPIRED_NOW_MS, SHARE_VALID_NOW_MS};
use super::*;
use crate::{AccessPermission, DataClass, ResourceKind};

fn request(
    principal: &PrincipalRef,
    resource: &ResourceRef,
    data_class: DataClass,
) -> AuthorityAccessRequest {
    AuthorityAccessRequest::new(
        principal.clone(),
        resource.clone(),
        AccessPermission::Read,
        data_class,
    )
}

#[test]
fn junior_engineer_inherits_department_repo_but_not_lead_architecture() {
    let f = fixtures::acme_company();

    // Junior engineering inherits the engineering department's source-code grant.
    let repo = f.graph.evaluate(
        &request(
            &f.junior_engineer,
            &f.engineering_repo,
            DataClass::SourceCode,
        ),
        fixtures::BASE_NOW_MS,
    );
    assert!(
        repo.is_allow(),
        "junior eng should read department source code"
    );
    assert_eq!(repo.reason_code, "matching_allow_grant");

    // But cannot read lead-only internal architecture docs without a grant.
    let architecture = f.graph.evaluate(
        &request(
            &f.junior_engineer,
            &f.internal_architecture_doc,
            DataClass::Restricted,
        ),
        fixtures::BASE_NOW_MS,
    );
    assert!(architecture.is_deny(), "junior eng must not read lead docs");
    assert_eq!(architecture.reason_code, "no_matching_grant");
}

#[test]
fn junior_engineering_agent_is_denied_lead_architecture_docs() {
    let f = fixtures::acme_company();

    let decision = f.graph.evaluate(
        &request(
            &f.junior_engineer_agent,
            &f.internal_architecture_doc,
            DataClass::Restricted,
        ),
        fixtures::BASE_NOW_MS,
    );

    assert!(
        decision.is_deny(),
        "a junior engineering agent must not read lead/internal architecture docs"
    );
    assert_eq!(decision.reason_code, "no_matching_grant");
}

#[test]
fn lead_engineer_reads_internal_architecture_and_secrets() {
    let f = fixtures::acme_company();

    let architecture = f.graph.evaluate(
        &request(
            &f.lead_engineer,
            &f.internal_architecture_doc,
            DataClass::Restricted,
        ),
        fixtures::BASE_NOW_MS,
    );
    assert!(architecture.is_allow());
    assert_eq!(
        architecture
            .source_principal
            .as_ref()
            .map(|p| p.id.as_str()),
        Some("org/lead-engineering")
    );

    let secret = f.graph.evaluate(
        &request(
            &f.lead_engineer,
            &f.engineering_secret,
            DataClass::Credential,
        ),
        fixtures::BASE_NOW_MS,
    );
    assert!(secret.is_allow(), "lead eng holds the credential grant");
}

#[test]
fn senior_engineer_cannot_read_finance_records_by_default() {
    let f = fixtures::acme_company();

    let decision = f.graph.evaluate(
        &request(
            &f.lead_engineer,
            &f.finance_ledger,
            DataClass::FinancialRecord,
        ),
        fixtures::BASE_NOW_MS,
    );

    assert!(
        decision.is_deny(),
        "engineers cannot read finance by default"
    );
    assert_eq!(decision.reason_code, "no_matching_grant");
}

#[test]
fn finance_actor_reads_financial_records_but_not_engineering_secrets() {
    let f = fixtures::acme_company();

    let ledger = f.graph.evaluate(
        &request(
            &f.finance_analyst,
            &f.finance_ledger,
            DataClass::FinancialRecord,
        ),
        fixtures::BASE_NOW_MS,
    );
    assert!(ledger.is_allow(), "finance can read financial records");

    let secret = f.graph.evaluate(
        &request(
            &f.finance_analyst,
            &f.engineering_secret,
            DataClass::Credential,
        ),
        fixtures::BASE_NOW_MS,
    );
    assert!(
        secret.is_deny(),
        "finance must not read restricted engineering secrets"
    );
    assert_eq!(secret.reason_code, "no_matching_grant");
}

#[test]
fn temporary_cross_department_share_grants_access_until_it_expires() {
    let f = fixtures::acme_company();

    // While the share is valid, finance can read the shared architecture doc.
    let during = f.graph.evaluate(
        &request(
            &f.finance_analyst,
            &f.internal_architecture_doc,
            DataClass::Restricted,
        ),
        SHARE_VALID_NOW_MS,
    );
    assert!(
        during.is_allow(),
        "shared access should be granted while valid"
    );
    assert_eq!(
        during.grant_id.as_deref(),
        Some("share-finance-architecture")
    );

    // After expiry the share lapses and access fails closed.
    let after = f.graph.evaluate(
        &request(
            &f.finance_analyst,
            &f.internal_architecture_doc,
            DataClass::Restricted,
        ),
        SHARE_EXPIRED_NOW_MS,
    );
    assert!(after.is_deny(), "expired share must no longer grant access");
    assert_eq!(after.reason_code, "no_matching_grant");
}

#[test]
fn explicit_deny_overrides_executive_org_wide_allow() {
    let f = fixtures::acme_company();

    // The executive org-wide allow reaches financial records...
    let finance = f.graph.evaluate(
        &request(&f.executive, &f.finance_ledger, DataClass::FinancialRecord),
        fixtures::BASE_NOW_MS,
    );
    assert!(finance.is_allow(), "executive holds org-wide read");

    // ...but a targeted deny on the legal hold room wins over that allow.
    let legal_hold = f.graph.evaluate(
        &request(&f.executive, &f.legal_hold_room, DataClass::Restricted),
        fixtures::BASE_NOW_MS,
    );
    assert!(
        legal_hold.is_deny(),
        "legal hold deny overrides org-wide allow"
    );
    assert_eq!(legal_hold.reason_code, "matching_deny_grant");
    assert_eq!(
        legal_hold.grant_id.as_deref(),
        Some("org/executive::g-exec-legal-hold")
    );
}

#[test]
fn support_agent_has_no_engineering_or_finance_access() {
    let f = fixtures::acme_company();

    for (resource, data_class) in [
        (&f.engineering_repo, DataClass::SourceCode),
        (&f.finance_ledger, DataClass::FinancialRecord),
        (&f.internal_architecture_doc, DataClass::Restricted),
    ] {
        let decision = f.graph.evaluate(
            &request(&f.support_agent, resource, data_class),
            fixtures::BASE_NOW_MS,
        );
        assert!(
            decision.is_deny(),
            "support must not reach engineering/finance resources"
        );
    }
}

#[test]
fn resource_from_another_org_is_denied_as_cross_tenant() {
    let f = fixtures::acme_company();
    let foreign = ResourceRef::new("other-co", "other", ResourceKind::DataStore, "ledger");

    let decision = f.graph.evaluate(
        &request(&f.executive, &foreign, DataClass::FinancialRecord),
        fixtures::BASE_NOW_MS,
    );

    assert!(decision.is_deny());
    assert_eq!(decision.reason_code, "resource_outside_tenant");
}

#[test]
fn resolved_unit_principals_include_parent_department_for_role_domain() {
    let f = fixtures::acme_company();

    let units = f
        .graph
        .resolved_unit_principals(&f.junior_engineer, fixtures::BASE_NOW_MS);
    let ids: Vec<&str> = units.iter().map(|u| u.id.as_str()).collect();

    assert!(
        ids.contains(&"org/junior-engineering"),
        "should include the directly-joined role domain"
    );
    assert!(
        ids.contains(&"org/engineering"),
        "should inherit the parent engineering department"
    );
    assert!(
        !ids.contains(&"org/lead-engineering"),
        "must not include the sibling lead role domain"
    );
}

#[test]
fn authority_decision_round_trips_through_json() {
    let f = fixtures::acme_company();
    let decision = f.graph.evaluate(
        &request(
            &f.finance_analyst,
            &f.finance_ledger,
            DataClass::FinancialRecord,
        ),
        fixtures::BASE_NOW_MS,
    );

    let encoded = serde_json::to_value(&decision).expect("serialize decision");
    assert_eq!(encoded["effect"], "allow");
    let decoded: AuthorityDecision = serde_json::from_value(encoded).expect("deserialize decision");
    assert_eq!(decoded, decision);
}

#[test]
fn graph_round_trips_through_json() {
    let f = fixtures::acme_company();
    let encoded = serde_json::to_value(&f.graph).expect("serialize graph");
    let decoded: IntraTenantAuthorityGraph =
        serde_json::from_value(encoded).expect("deserialize graph");
    assert_eq!(decoded, f.graph);
}
