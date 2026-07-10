use serde_json::json;
use tandem_types::TenantContext;

use crate::AppState;

pub(crate) async fn append_cross_tenant_denial(
    state: &AppState,
    channel: &'static str,
    user_id: &str,
    run_id: &str,
    channel_tenant: TenantContext,
    run_tenant: &TenantContext,
) {
    let actor = format!("channel:{channel}:{user_id}");
    crate::audit::append_protected_audit_event_best_effort(
        state,
        "channel.interaction.cross_tenant_denied",
        &channel_tenant,
        Some(actor),
        json!({
            "channel": channel,
            "user_id": user_id,
            "run_id": run_id,
            "channel_tenant": channel_tenant,
            "run_tenant": run_tenant,
            "reason": "channel not bound to this run's tenant",
        }),
    )
    .await;
}
