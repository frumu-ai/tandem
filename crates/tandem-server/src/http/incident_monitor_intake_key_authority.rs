// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[derive(Debug)]
struct IncidentMonitorIntakeKeyDenied(&'static str);

impl std::fmt::Display for IncidentMonitorIntakeKeyDenied {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for IncidentMonitorIntakeKeyDenied {}

fn require_incident_monitor_intake_key_admin(
    state: &AppState,
    verified: Option<&tandem_types::VerifiedTenantContext>,
) -> anyhow::Result<()> {
    state
        .enterprise
        .hosted_policy
        .authorize_permission(verified, tandem_types::AccessPermission::HostedAdmin)
        .map_err(|code| anyhow::Error::new(IncidentMonitorIntakeKeyDenied(code)))
}
