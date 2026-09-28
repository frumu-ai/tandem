// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

/// A denied create must not leave the newly allocated session behind. Existing
/// sessions use the checked helper directly and are never deleted on denial.
async fn apply_created_session_permission_rules(
    state: &AppState,
    session: &Session,
    rules: Option<Vec<Value>>,
) -> Result<(), HttpError> {
    if let Err(status) = apply_session_permission_rules_checked(
        state,
        &session.tenant_context,
        &session.id,
        session.verified_tenant_context.as_ref(),
        rules,
    )
    .await
    {
        state
            .storage
            .delete_session(&session.id)
            .await
            .map_err(|error| {
                tracing::error!(error = %error, session_id = %session.id,
                "failed to remove newly created session after permission denial");
                persistence_error("Failed to remove denied session")
            })?;
        return Err(http_error(
            status,
            "session permission authority is no longer current",
            ErrorCode::TenantContextDenied,
        ));
    }
    Ok(())
}

/// Internal channel-profile rules are administrator-authored, not supplied by
/// the channel user. Public create/update requests use the checked seam below.
pub(super) async fn apply_session_permission_rules(
    state: &AppState,
    tenant_context: &TenantContext,
    session_id: &str,
    rules: Option<Vec<Value>>,
) {
    let _ = apply_session_permission_rules_with_authority(
        state,
        tenant_context,
        session_id,
        rules,
        || Ok(()),
    )
    .await;
}

async fn apply_session_permission_rules_checked(
    state: &AppState,
    tenant_context: &TenantContext,
    session_id: &str,
    verified: Option<&VerifiedTenantContext>,
    rules: Option<Vec<Value>>,
) -> Result<(), StatusCode> {
    apply_session_permission_rules_with_authority(state, tenant_context, session_id, rules, || {
        if !session_permission_rules_allowed(tenant_context, verified) {
            return Err(StatusCode::FORBIDDEN);
        }
        state
            .enterprise
            .hosted_policy
            .authorize_permission(verified, tandem_types::AccessPermission::HostedAdmin)
            .map_err(|_| StatusCode::FORBIDDEN)
    })
    .await
}

async fn apply_session_permission_rules_with_authority(
    state: &AppState,
    tenant_context: &TenantContext,
    session_id: &str,
    rules: Option<Vec<Value>>,
    authorize: impl Fn() -> Result<(), StatusCode>,
) -> Result<(), StatusCode> {
    let Some(rules) = rules else {
        return Ok(());
    };
    for raw in rules {
        let Some((permission, pattern, action)) = parse_permission_rule_input(&raw) else {
            continue;
        };
        state
            .permissions
            .add_rule_for_session_checked(
                tenant_context,
                session_id,
                permission,
                pattern,
                action,
                &authorize,
            )
            .await?;
    }
    Ok(())
}

#[cfg(test)]
include!("session_permission_rules_tests.rs");
