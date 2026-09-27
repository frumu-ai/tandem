// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[derive(Debug)]
struct IncidentMonitorConfigDenied(&'static str);

impl std::fmt::Display for IncidentMonitorConfigDenied {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for IncidentMonitorConfigDenied {}

async fn put_authorized_incident_monitor_config(
    state: &AppState,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    config: IncidentMonitorConfig,
) -> anyhow::Result<IncidentMonitorConfig> {
    let authorize = || {
        state
            .enterprise
            .hosted_policy
            .authorize_permission(verified, tandem_types::AccessPermission::HostedAdmin)
            .map_err(|code| anyhow::Error::new(IncidentMonitorConfigDenied(code)))
    };
    authorize()?;
    state
        .put_incident_monitor_config_checked(config, authorize)
        .await
}
