// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Offline contract demonstration. The synthetic identity below is deliberately
//! not authentication and must never be used by a server adapter.
use std::collections::BTreeMap;
use tandem_enterprise_contract::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContext, TenantContextAssertionClaims,
};
use tandem_solutions::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--schema") {
        println!("{}", serde_json::to_string_pretty(&blueprint_schema())?);
        return Ok(());
    }
    let blueprint = parse_blueprint(include_str!("../fixtures/company-brain-text/solution.json"))?;
    let request: InstallRequest =
        serde_json::from_str(include_str!("../fixtures/company-brain-text/request.json"))?;
    // Separate synthetic host registry; customer JSON contains only binding IDs.
    let approved_models = serde_json::from_str(include_str!(
        "../fixtures/company-brain-text/host-models.json"
    ))?;
    let principal = RequestPrincipal::authenticated_user("fixture-owner", "fixture");
    let context = TenantContextAssertionClaims::new_v1(
        "fixture",
        "fixture",
        1000,
        2000,
        "offline-example",
        TenantContext::explicit_user_workspace(
            "fixture-org",
            "fixture-workspace",
            Some("fixture-deployment".into()),
            "fixture-owner",
        ),
        HumanActor::tandem_user("fixture-owner"),
        AuthorityChain::from_request(principal),
        vec!["workspace:user".into()],
    )
    .into();
    let artifacts = BTreeMap::from([
        (
            "central-brain".into(),
            include_bytes!("../fixtures/company-brain-text/agents/central-brain.json").to_vec(),
        ),
        (
            "review-notes".into(),
            include_bytes!("../fixtures/company-brain-text/routines/review-notes.json").to_vec(),
        ),
    ]);
    let requirements = blueprint.deployment_requirements.clone();
    let plan = resolve(
        &blueprint,
        ResolutionInput {
            host_facts_sha256: None,
            request: &request,
            verified_context: &context,
            now_ms: 1500,
            engine_version: "0.7.2",
            deployment_policy: &blueprint.constraints,
            available_deployment_requirements: &requirements,
            approved_models: &approved_models,
            artifacts: &artifacts,
        },
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "mode": "offline_fixture_only", "composition_hash": plan.composition_hash()?, "plan": plan
        }))?
    );
    Ok(())
}
