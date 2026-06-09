mod routes_enterprise;
mod routes_enterprise_cross_tenant;
mod routes_enterprise_google_drive;
mod routes_enterprise_lifecycle;
mod routes_enterprise_onboarding;
mod routes_enterprise_org_units;

pub fn apply_routes(router: tandem_server::ServerRouter) -> tandem_server::ServerRouter {
    routes_enterprise_cross_tenant::apply(routes_enterprise::apply(router))
}
