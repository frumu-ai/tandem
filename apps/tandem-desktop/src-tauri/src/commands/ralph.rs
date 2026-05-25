// Ralph Loop Commands
// ============================================================================

use crate::ralph::types::{IterationRecord, RalphConfig, RalphStateSnapshot};

/// Start a new Ralph Loop
#[tauri::command]
pub async fn ralph_start(
    state: State<'_, AppState>,
    session_id: String,
    prompt: String,
    config: Option<RalphConfig>,
) -> Result<String> {
    let workspace_path = {
        let workspace = state.workspace_path.read().unwrap();
        workspace
            .as_ref()
            .ok_or_else(|| TandemError::Ralph("No workspace set".to_string()))?
            .clone()
    };

    let config = config.unwrap_or_default();
    let run_id = state
        .ralph_manager
        .start(
            session_id,
            prompt,
            config,
            workspace_path,
            state.sidecar.clone(),
            state.stream_hub.clone(),
        )
        .await?;

    Ok(run_id)
}

/// Cancel a running Ralph Loop
#[tauri::command]
pub async fn ralph_cancel(state: State<'_, AppState>, run_id: String) -> Result<()> {
    state.ralph_manager.cancel(&run_id).await
}

/// Pause a running Ralph Loop
#[tauri::command]
pub async fn ralph_pause(state: State<'_, AppState>, run_id: String) -> Result<()> {
    state.ralph_manager.pause(&run_id).await
}

/// Resume a paused Ralph Loop
#[tauri::command]
pub async fn ralph_resume(state: State<'_, AppState>, run_id: String) -> Result<()> {
    state.ralph_manager.resume(&run_id).await
}

/// Add context to be injected in next iteration
#[tauri::command]
pub async fn ralph_add_context(
    state: State<'_, AppState>,
    run_id: String,
    text: String,
) -> Result<()> {
    state.ralph_manager.add_context(&run_id, text).await
}

/// Get current Ralph Loop status
#[tauri::command]
pub async fn ralph_status(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RalphStateSnapshot> {
    state.ralph_manager.status(&run_id).await
}

/// Get Ralph Loop iteration history
#[tauri::command]
pub async fn ralph_history(
    state: State<'_, AppState>,
    run_id: String,
    limit: Option<usize>,
) -> Result<Vec<IterationRecord>> {
    state
        .ralph_manager
        .history(&run_id, limit.unwrap_or(50))
        .await
}
