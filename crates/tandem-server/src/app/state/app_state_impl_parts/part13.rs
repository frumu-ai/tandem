// Provider OAuth credential upkeep for AppState.
//
// Split out of part01 to keep that file within the repository's per-file line
// budget. Included into `app/state/mod.rs`, so it shares that module's imports.

impl AppState {
    /// Spawn a background task that keeps provider OAuth credentials (currently
    /// the OpenAI Codex ChatGPT sign-in) fresh independent of the control panel.
    ///
    /// Historically the Codex OAuth access token was only refreshed as a side
    /// effect of the control panel polling `GET /provider/auth`. Channels,
    /// automations, and scheduled runs never hit that endpoint, so once the
    /// token expired every run failed with `AUTHENTICATION_ERROR` until an
    /// operator reopened the panel (TAN-594). This task performs an initial
    /// refresh pass on startup — so a restart that lands past expiry self-heals
    /// — and then re-checks on a short interval. The underlying refresh is a
    /// cheap local expiry check that only makes a network call when the token
    /// is within the refresh skew window of expiring.
    pub fn spawn_provider_oauth_refresh(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            // Initial pass so a boot past expiry recovers without the panel.
            state.refresh_provider_oauth_once().await;
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                state.refresh_provider_oauth_once().await;
            }
        });
    }

    async fn refresh_provider_oauth_once(&self) {
        // Single-tenant / local deployments use the implicit tenant. Multi-tenant
        // hosted refresh is tracked separately; per-tenant enumeration is not
        // wired here yet.
        let tenant = TenantContext::local_implicit();
        if let Err(error) =
            crate::http::config_providers::refresh_openai_codex_oauth_if_needed(self, &tenant).await
        {
            tracing::warn!(
                target: "tandem_server::provider_oauth_refresh",
                %error,
                "background Codex OAuth refresh failed"
            );
        }
    }
}
