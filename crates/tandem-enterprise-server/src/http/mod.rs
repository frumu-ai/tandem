// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

mod routes_enterprise;
mod routes_enterprise_cross_tenant;
mod routes_enterprise_google_drive;
mod routes_enterprise_lifecycle;
mod routes_enterprise_onboarding;
mod routes_enterprise_org_units;
mod routes_enterprise_policies;

pub fn apply_routes(router: tandem_server::ServerRouter) -> tandem_server::ServerRouter {
    routes_enterprise_policies::apply(routes_enterprise_cross_tenant::apply(
        routes_enterprise::apply(router),
    ))
}
