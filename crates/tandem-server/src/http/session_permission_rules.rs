// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

/// A failed batch must not leave the newly allocated session behind. Existing
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
                "failed to remove newly created session after permission batch failure");
                persistence_error("Failed to remove session after permission batch failure")
            })?;
        return Err(if status == StatusCode::FORBIDDEN {
            http_error(
                status,
                "session permission authority is no longer current",
                ErrorCode::TenantContextDenied,
            )
        } else {
            persistence_error("Failed to persist session permission rules")
        });
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
) -> Result<(), StatusCode> {
    apply_session_permission_rules_with_authority(
        state,
        tenant_context,
        session_id,
        rules,
        |commit| commit(),
    )
    .await
}

#[derive(Debug)]
struct SessionPermissionAuthorityDenied;

impl std::fmt::Display for SessionPermissionAuthorityDenied {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("session permission authority is no longer current")
    }
}

impl std::error::Error for SessionPermissionAuthorityDenied {}

async fn apply_session_permission_rules_checked(
    state: &AppState,
    tenant_context: &TenantContext,
    session_id: &str,
    verified: Option<&VerifiedTenantContext>,
    rules: Option<Vec<Value>>,
) -> Result<(), StatusCode> {
    let commit_state = state.clone();
    let commit_tenant = tenant_context.clone();
    let commit_verified = verified.cloned();
    apply_session_permission_rules_with_authority(
        state,
        tenant_context,
        session_id,
        rules,
        move |commit| {
            commit_state
                .enterprise
                .hosted_policy
                .with_current_policy(|policy| {
                    if !session_permission_rules_allowed(&commit_tenant, commit_verified.as_ref()) {
                        return Err(anyhow::Error::new(SessionPermissionAuthorityDenied));
                    }
                    super::require_hosted_permission_under_policy(
                        &commit_tenant,
                        commit_verified.as_ref(),
                        tandem_types::AccessPermission::HostedAdmin,
                        policy,
                    )
                    .map_err(|_| anyhow::Error::new(SessionPermissionAuthorityDenied))?;
                    // Keep the current-policy read guard through the complete
                    // synchronous disk and live-rule publication.
                    commit()
                })
                .map_err(|_| anyhow::Error::new(SessionPermissionAuthorityDenied))?
        },
    )
    .await
}

async fn apply_session_permission_rules_with_authority(
    state: &AppState,
    tenant_context: &TenantContext,
    session_id: &str,
    rules: Option<Vec<Value>>,
    guard: impl FnOnce(&mut dyn FnMut() -> anyhow::Result<()>) -> anyhow::Result<()> + Send + 'static,
) -> Result<(), StatusCode> {
    let Some(rules) = rules else {
        return Ok(());
    };
    let rules = rules
        .iter()
        .filter_map(parse_permission_rule_input)
        .collect();
    state
        .permissions
        .add_rules_for_session_with_commit_guard(tenant_context, session_id, rules, guard)
        .await
        .map_err(|error| {
            if error.downcast_ref::<SessionPermissionAuthorityDenied>().is_some() {
                StatusCode::FORBIDDEN
            } else {
                tracing::error!(%error, session_id, "failed to persist session permission rule batch");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })
}

#[cfg(test)]
include!("session_permission_rules_tests.rs");
