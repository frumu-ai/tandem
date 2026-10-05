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
