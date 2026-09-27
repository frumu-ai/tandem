// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

fn can_read_events(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> bool {
    if let Some(verified) = verified {
        let identity = &verified.tenant_context;
        if identity.org_id != tenant.org_id
            || identity.workspace_id != tenant.workspace_id
            || identity.deployment_id != tenant.deployment_id
            || identity.actor_id != tenant.actor_id
        {
            return false;
        }
    }
    state
        .enterprise
        .hosted_policy
        .authorize_permission(
            verified,
            tandem_types::AccessPermission::HostedAutomationRead,
        )
        .is_ok()
}

// Revalidate at the output boundary, including ready frames and idle streams.
// A denial ends the stream permanently; a later policy reload cannot revive it.
pub(super) fn guard_automation_events<S>(
    stream: S,
    state: AppState,
    tenant: TenantContext,
    verified: Option<VerifiedTenantContext>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>>
where
    S: Stream<Item = Result<Event, std::convert::Infallible>> + Send + 'static,
{
    let timer = tokio::time::interval(Duration::from_secs(1));
    futures::stream::unfold(
        (Box::pin(stream), state, tenant, verified, timer),
        |(mut stream, state, tenant, verified, mut timer)| async move {
            loop {
                if !can_read_events(&state, &tenant, verified.as_ref()) {
                    return None;
                }
                tokio::select! {
                    biased;
                    _ = timer.tick() => continue,
                    item = stream.next() => {
                        let item = item?;
                        if !can_read_events(&state, &tenant, verified.as_ref()) {
                            return None;
                        }
                        return Some((item, (stream, state, tenant, verified, timer)));
                    }
                }
            }
        },
    )
}
