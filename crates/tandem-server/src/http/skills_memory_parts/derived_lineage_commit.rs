// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

/// The backend invokes this only while holding its actual transaction writer
/// boundary. Reuse the original assertion; renewing a request here would hide
/// expiry during a native store wait.
fn derived_memory_commit_authority(
    state: &AppState,
    _tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> tandem_memory::MemoryCommitAuthority {
    let state = state.clone();
    let verified = verified.cloned();
    Arc::new(move || {
        state
            .enterprise
            .hosted_policy
            .authorize(verified.as_ref())
            .map_err(|reason| {
                tandem_memory::MemoryStoreError::new(
                    tandem_memory::MemoryStoreErrorKind::ScopeViolation,
                    reason,
                )
            })
    })
}

/// Re-evaluate the captured, server-resolved source conjunction at the real
/// synchronous commit boundary. Clock-dependent source/grant restrictions are
/// refreshed without reading any source store while target locks are held.
fn derived_memory_commit_authority_with_lineage(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    lineage: DerivedMemoryLineage,
    filter: MemoryAccessFilter,
    target: Option<GlobalMemoryRecord>,
    operation_allowed: impl Fn(u64) -> bool + Send + Sync + 'static,
) -> tandem_memory::MemoryCommitAuthority {
    let current_identity = derived_memory_commit_authority(state, tenant, verified);
    let target_policy = target.as_ref().map(|record|
        tandem_memory::KnowledgeScopePolicy::from_metadata(record.metadata.as_ref()));
    Arc::new(move || {
        current_identity()?;
        let now = crate::now_ms();
        let mut current_filter = filter.clone();
        current_filter.now_ms = now;
        if !current_filter.decision_for_derived_lineage(&lineage).allowed
            || target.as_ref().is_some_and(|record|
                record.expires_at_ms.is_some_and(|expiry| expiry <= now)
                    || !current_filter.decision_for_global_record(record).allowed)
            || target_policy.as_ref().is_some_and(|policy| match policy {
                Ok(Some(policy)) => policy.read_denial_reason(current_filter.workflow_phase.as_deref(), now).is_some(),
                Ok(None) => false,
                Err(_) => true,
            })
            || !operation_allowed(now)
        {
            return Err(tandem_memory::MemoryStoreError::new(
                tandem_memory::MemoryStoreErrorKind::ScopeViolation,
                "derived_memory_source_authority_expired_or_denied",
            ));
        }
        Ok(())
    })
}

fn derived_memory_commit_error_status(error: tandem_memory::MemoryStoreError) -> StatusCode {
    if error.kind == tandem_memory::MemoryStoreErrorKind::ScopeViolation
        && error.message == "derived_memory_source_authority_expired_or_denied"
    {
        StatusCode::FORBIDDEN
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// Order a derived-memory side effect with hosted policy publication. Native
/// session-source reads happen before this guard to preserve target-store lock
/// ordering. The owned task keeps authority alive through durable completion
/// even if the HTTP caller stops waiting.
async fn commit_derived_memory_with_current_policy<T: Send + 'static>(
    state: &AppState,
    _tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    commit: impl std::future::Future<Output = T> + Send + 'static,
) -> Result<T, StatusCode> {
    let guard = state.enterprise.hosted_policy.lock_publication_owned().await;
    state.enterprise.hosted_policy.authorize(verified)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    tokio::spawn(async move {
        let _guard = guard;
        commit.await
    }).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
#[path = "derived_lineage_commit_tests.rs"]
mod derived_lineage_commit_tests;
