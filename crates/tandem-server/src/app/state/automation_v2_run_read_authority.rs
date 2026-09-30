// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Follow the context engine's process-local lock-map convention. All native
// writers/readers of this store use the same active path in one AppState.
async fn automation_v2_run_history_lock(
    active_path: &Path,
) -> std::sync::Arc<tokio::sync::RwLock<()>> {
    type HistoryLocks = tokio::sync::Mutex<
        std::collections::HashMap<PathBuf, std::sync::Arc<tokio::sync::RwLock<()>>>,
    >;
    static LOCKS: std::sync::OnceLock<HistoryLocks> = std::sync::OnceLock::new();
    let locks = LOCKS.get_or_init(HistoryLocks::default);
    let mut locks = locks.lock().await;
    locks
        .entry(automation_v2_run_history_root(active_path))
        .or_insert_with(|| std::sync::Arc::new(tokio::sync::RwLock::new(())))
        .clone()
}

pub(crate) struct AutomationV2RunHistoryReadGuard {
    active_path: PathBuf,
    _guard: tokio::sync::OwnedRwLockReadGuard<()>,
}

pub(crate) async fn automation_v2_run_history_read_guard(
    active_path: &Path,
) -> AutomationV2RunHistoryReadGuard {
    let lock = automation_v2_run_history_lock(active_path).await;
    AutomationV2RunHistoryReadGuard {
        active_path: active_path.to_owned(),
        _guard: lock.read_owned().await,
    }
}

/// The canonical hot/history preference, shared by the ordinary getter and
/// the final no-await stream decision. Reject a misbound native record rather
/// than treating the file name or map key as its owning identity.
pub(crate) fn select_automation_v2_run_source(
    run_id: &str,
    hot: Option<AutomationV2RunRecord>,
    history: Option<AutomationV2RunRecord>,
) -> Option<AutomationV2RunRecord> {
    let valid = |run: &AutomationV2RunRecord| {
        run.run_id == run_id && !automation_v2_run_is_nonterminal_recovered_context_run(run)
    };
    match (hot.filter(valid), history.filter(valid)) {
        (Some(hot), Some(history)) => {
            let history_has_pending_gate =
                history
                    .checkpoint
                    .awaiting_gate
                    .as_ref()
                    .is_some_and(|gate| {
                        !automation_run_is_terminal(&hot.status)
                            && hot.checkpoint.awaiting_gate.is_none()
                            && !super::automation_gate_has_settled_decision(&hot, &gate.node_id)
                    });
            let history_has_more_detail = history.checkpoint.node_outputs.len()
                > hot.checkpoint.node_outputs.len()
                || (hot.runtime_context.is_none() && history.runtime_context.is_some())
                || (hot.automation_snapshot.is_none() && history.automation_snapshot.is_some());
            if history_has_pending_gate || history_has_more_detail {
                Some(history)
            } else {
                Some(hot)
            }
        }
        (Some(hot), None) => Some(hot),
        (None, Some(history)) => Some(history),
        (None, None) => None,
    }
}

pub(crate) struct AutomationV2RunReadSources {
    history: Option<AutomationV2RunRecord>,
    recovered: Option<AutomationV2RunRecord>,
}

/// Await the native parsers while their history writer is excluded. The
/// caller separately holds the actual projection engine token during recovery.
/// This API cannot re-lock history, even when a writer is queued behind it.
pub(crate) async fn load_automation_v2_run_read_sources(
    state: &crate::AppState,
    guard: &AutomationV2RunHistoryReadGuard,
    run_id: &str,
) -> Option<AutomationV2RunReadSources> {
    if guard.active_path != state.automation_v2_runs_path {
        return None;
    }
    let history = load_automation_v2_run_history_shard_with_guard(guard, run_id).await;
    let recovered =
        super::automation_v2_context_recovery::get_recovered_automation_v2_run(state, run_id)
            .await
            .filter(|run| run.run_id == run_id);
    Some(AutomationV2RunReadSources { history, recovered })
}

pub(crate) fn current_automation_v2_run_read_source(
    run_id: &str,
    hot: Option<&AutomationV2RunRecord>,
    sources: &AutomationV2RunReadSources,
) -> Option<AutomationV2RunRecord> {
    select_automation_v2_run_source(run_id, hot.cloned(), sources.history.clone())
        .or_else(|| sources.recovered.clone())
}
