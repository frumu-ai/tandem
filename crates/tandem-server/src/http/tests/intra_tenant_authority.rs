use super::*;

use tandem_enterprise_contract::authority::{fixtures, AuthorityAccessRequest};
use tandem_enterprise_contract::{
    AccessPermission, DataClass, OrganizationUnit, OrganizationUnitAccessGrant,
    OrganizationUnitMembership, ScopedGrant,
};

/// Load the seeded `acme` authority graph into an `AppState`'s enterprise org
/// unit / membership / grant stores so server-side enforcement reads it back.
async fn seed_acme_authority(state: &AppState) -> fixtures::AcmeAuthorityFixture {
    let fixture = fixtures::acme_company();
    {
        let mut units = state.enterprise_org_units.write().await;
        for unit in &fixture.graph.units {
            units.insert(unit_key(unit), unit.clone());
        }
    }
    {
        let mut memberships = state.enterprise_org_unit_memberships.write().await;
        for membership in &fixture.graph.memberships {
            memberships.insert(membership_key(membership), membership.clone());
        }
    }
    {
        let mut grants = state.enterprise_org_unit_access_grants.write().await;
        for grant in &fixture.graph.unit_access_grants {
            grants.insert(grant_key(grant), grant.clone());
        }
    }
    fixture
}

fn unit_key(unit: &OrganizationUnit) -> String {
    format!("{}/{}", unit.taxonomy_id, unit.unit_id)
}

fn membership_key(membership: &OrganizationUnitMembership) -> String {
    membership.membership_id.clone()
}

fn grant_key(grant: &OrganizationUnitAccessGrant) -> String {
    grant.grant_id.clone()
}

fn read_request(
    principal: &tandem_enterprise_contract::PrincipalRef,
    resource: &tandem_enterprise_contract::ResourceRef,
    data_class: DataClass,
) -> AuthorityAccessRequest {
    AuthorityAccessRequest::new(
        principal.clone(),
        resource.clone(),
        AccessPermission::Read,
        data_class,
    )
}

#[tokio::test]
async fn junior_engineer_denied_lead_docs_records_decision_and_audit() {
    let state = test_state().await;
    let fixture = seed_acme_authority(&state).await;
    let now = fixtures::BASE_NOW_MS;

    let (decision, decision_id) = state
        .enforce_intra_tenant_access(
            &fixture.tenant_context,
            &read_request(
                &fixture.junior_engineer_agent,
                &fixture.internal_architecture_doc,
                DataClass::Restricted,
            ),
            Vec::new(),
            now,
        )
        .await;

    assert!(
        decision.is_deny(),
        "junior eng agent must be denied lead docs"
    );
    assert_eq!(decision.reason_code, "no_matching_grant");
    let decision_id = decision_id.expect("denial must record a policy decision");

    // The denial is persisted as a policy decision record...
    let decisions = state
        .list_policy_decisions(&fixture.tenant_context, 50)
        .await;
    let recorded = decisions
        .iter()
        .find(|d| d.decision_id == decision_id)
        .expect("recorded policy decision");
    assert_eq!(recorded.decision, tandem_types::PolicyDecisionEffect::Deny);
    assert_eq!(
        recorded.policy_id.as_deref(),
        Some("intra_tenant_authority")
    );
    assert_eq!(
        recorded.resource.as_ref().map(|r| r.resource_id.as_str()),
        Some("internal-architecture")
    );

    // ...and as a tenant-attributed protected audit event.
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"authority.access.denied\""));
    assert!(audit.contains("internal-architecture"));
    assert!(audit.contains("user-junior-eng"));
    assert!(audit.contains("\"org_id\":\"acme\""));
}

#[tokio::test]
async fn senior_engineer_denied_finance_records_fail_closed() {
    let state = test_state().await;
    let fixture = seed_acme_authority(&state).await;

    let (decision, _) = state
        .enforce_intra_tenant_access(
            &fixture.tenant_context,
            &read_request(
                &fixture.lead_engineer,
                &fixture.finance_ledger,
                DataClass::FinancialRecord,
            ),
            Vec::new(),
            fixtures::BASE_NOW_MS,
        )
        .await;

    assert!(
        decision.is_deny(),
        "engineers cannot read finance by default"
    );
    assert_eq!(decision.reason_code, "no_matching_grant");
}

#[tokio::test]
async fn finance_actor_allowed_records_but_denied_engineering_secret() {
    let state = test_state().await;
    let fixture = seed_acme_authority(&state).await;
    let now = fixtures::BASE_NOW_MS;

    let (ledger, _) = state
        .enforce_intra_tenant_access(
            &fixture.tenant_context,
            &read_request(
                &fixture.finance_analyst,
                &fixture.finance_ledger,
                DataClass::FinancialRecord,
            ),
            Vec::new(),
            now,
        )
        .await;
    assert!(ledger.is_allow(), "finance reads financial records");

    let (secret, _) = state
        .enforce_intra_tenant_access(
            &fixture.tenant_context,
            &read_request(
                &fixture.finance_analyst,
                &fixture.engineering_secret,
                DataClass::Credential,
            ),
            Vec::new(),
            now,
        )
        .await;
    assert!(secret.is_deny(), "finance cannot read engineering secrets");

    // An allow decision is still recorded for the ledger read.
    let decisions = state
        .list_policy_decisions(&fixture.tenant_context, 50)
        .await;
    assert!(decisions.iter().any(|d| {
        d.decision == tandem_types::PolicyDecisionEffect::Allow
            && d.resource.as_ref().map(|r| r.resource_id.as_str()) == Some("finance-ledger")
    }));
}

#[tokio::test]
async fn temporary_share_grants_access_then_fails_closed_after_expiry() {
    let state = test_state().await;
    let fixture = seed_acme_authority(&state).await;
    // The expiring share is a direct grant carried alongside the request.
    let direct_grants: Vec<ScopedGrant> = fixture.graph.direct_grants.clone();

    let (during, _) = state
        .enforce_intra_tenant_access(
            &fixture.tenant_context,
            &read_request(
                &fixture.finance_analyst,
                &fixture.internal_architecture_doc,
                DataClass::Restricted,
            ),
            direct_grants.clone(),
            fixtures::SHARE_VALID_NOW_MS,
        )
        .await;
    assert!(during.is_allow(), "share grants access while valid");

    let (after, _) = state
        .enforce_intra_tenant_access(
            &fixture.tenant_context,
            &read_request(
                &fixture.finance_analyst,
                &fixture.internal_architecture_doc,
                DataClass::Restricted,
            ),
            direct_grants,
            fixtures::SHARE_EXPIRED_NOW_MS,
        )
        .await;
    assert!(after.is_deny(), "expired share fails closed");
    assert_eq!(after.reason_code, "no_matching_grant");
}
