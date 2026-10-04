// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// This is the actual engine lock, not the separate task-batch lock map.
// Construction is private and the requested run binding cannot be changed.
pub(super) struct ContextRunProjectionGuard {
    run_id: String,
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

impl ContextRunProjectionGuard {
    pub(super) fn run_id(&self) -> &str {
        &self.run_id
    }
}

impl ContextRunEngine {
    async fn projection_guard_for(
        &self,
        run_id: &str,
    ) -> Result<Arc<ContextRunProjectionGuard>, StatusCode> {
        if !super::context_run_authority::valid_context_run_id(run_id) {
            return Err(StatusCode::NOT_FOUND);
        }
        let lock = self.lock_for(run_id).await;
        Ok(Arc::new(ContextRunProjectionGuard {
            run_id: run_id.to_owned(),
            _guard: lock.lock_owned().await,
        }))
    }
}

pub(super) async fn context_run_projection_guard_for(
    run_id: &str,
) -> Result<Arc<ContextRunProjectionGuard>, StatusCode> {
    context_run_engine().projection_guard_for(run_id).await
}

pub(super) async fn load_context_run_state(
    state: &AppState,
    run_id: &str,
) -> Result<ContextRunState, StatusCode> {
    let guard = context_run_projection_guard_for(run_id).await?;
    load_context_run_state_with_projection_guard(state, &guard).await
}

pub(super) async fn load_context_run_state_with_projection_guard(
    state: &AppState,
    guard: &ContextRunProjectionGuard,
) -> Result<ContextRunState, StatusCode> {
    let mut run = load_and_repair_context_run_state_locked(state, guard)?;
    // Preserve legacy routine projection repair, but acquire the engine lock
    // once. The token also serializes pending payload.run replay replacements.
    if run.run_type == "routine"
        && run.source_client.as_deref() == Some("routine_runtime")
        && run.tenant_context == TenantContext::local_implicit()
    {
        if let Some(native_id) = guard.run_id().strip_prefix("routine-") {
            if let Some(canonical) = state.get_routine_run(native_id).await {
                if canonical.tenant_context != run.tenant_context {
                    run.tenant_context = canonical.tenant_context;
                    save_context_run_state_with_projection_guard_sync(state, guard, &run)?;
                }
            }
        }
    }
    Ok(run)
}

fn save_context_run_state_with_projection_guard_sync(
    state: &AppState,
    guard: &ContextRunProjectionGuard,
    run: &ContextRunState,
) -> Result<(), StatusCode> {
    if run.run_id != guard.run_id() {
        return Err(StatusCode::BAD_REQUEST);
    }
    save_context_run_state_unchecked_sync(state, run)
}

// Preserve the existing fixture-only raw writer. No production caller can
// write a projection without either the ordinary wrapper or an engine token.
#[cfg(test)]
pub(super) fn save_context_run_state_sync(
    state: &AppState,
    run: &ContextRunState,
) -> Result<(), StatusCode> {
    save_context_run_state_unchecked_sync(state, run)
}

#[cfg(test)]
pub(super) struct ContextRunProjectionWriteTestGate {
    pub(super) started: tokio::sync::oneshot::Sender<()>,
    pub(super) resume: tokio::sync::oneshot::Receiver<()>,
}

pub(super) async fn save_context_run_state(
    state: &AppState,
    run: &ContextRunState,
) -> Result<(), StatusCode> {
    save_context_run_state_impl(
        state,
        run,
        #[cfg(test)]
        None,
    )
    .await
}

#[cfg(test)]
pub(super) async fn save_context_run_state_gated(
    state: &AppState,
    run: &ContextRunState,
    gate: ContextRunProjectionWriteTestGate,
) -> Result<(), StatusCode> {
    save_context_run_state_impl(state, run, Some(gate)).await
}

async fn save_context_run_state_impl(
    state: &AppState,
    run: &ContextRunState,
    #[cfg(test)] gate: Option<ContextRunProjectionWriteTestGate>,
) -> Result<(), StatusCode> {
    // Preserve the public saver's BAD_REQUEST error for invalid directory IDs.
    if !super::context_run_authority::valid_context_run_id(&run.run_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let guard = context_run_projection_guard_for(&run.run_id).await?;
    save_context_run_state_with_projection_guard_impl(
        state,
        &guard,
        run,
        #[cfg(test)]
        gate,
    )
    .await
}

async fn save_context_run_state_with_projection_guard_impl(
    state: &AppState,
    guard: &Arc<ContextRunProjectionGuard>,
    run: &ContextRunState,
    #[cfg(test)] gate: Option<ContextRunProjectionWriteTestGate>,
) -> Result<(), StatusCode> {
    if run.run_id != guard.run_id() {
        return Err(StatusCode::BAD_REQUEST);
    }
    ensure_context_run_dir(state, &run.run_id).await?;
    let path = context_run_state_path(state, &run.run_id);
    let payload =
        serde_json::to_string_pretty(run).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // A blocking write outlives cancellation of its awaiting async caller.
    // Keep an owned engine token in the worker until the actual rename ends.
    let retained_guard = Arc::clone(guard);
    tokio::task::spawn_blocking(move || {
        let _guard = retained_guard;
        #[cfg(test)]
        if let Some(gate) = gate {
            let _ = gate.started.send(());
            let _ = gate.resume.blocking_recv();
        }
        write_string_atomically(&path, &payload)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}
