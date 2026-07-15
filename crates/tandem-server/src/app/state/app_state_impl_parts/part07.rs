// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Top-level state helpers split from mod.rs for the file-size gate
// (same module via include!).

pub async fn run_session_part_persister(state: AppState) {
    crate::app::tasks::run_session_part_persister(state).await
}

pub async fn run_status_indexer(state: AppState) {
    crate::app::tasks::run_status_indexer(state).await
}

pub async fn run_agent_team_supervisor(state: AppState) {
    crate::app::tasks::run_agent_team_supervisor(state).await
}

pub async fn run_incident_monitor(state: AppState) {
    crate::app::tasks::run_incident_monitor(state).await
}

pub async fn run_incident_monitor_recovery_sweep(state: AppState) {
    crate::app::tasks::run_incident_monitor_recovery_sweep(state).await
}

pub async fn run_usage_aggregator(state: AppState) {
    crate::app::tasks::run_usage_aggregator(state).await
}

pub async fn run_optimization_scheduler(state: AppState) {
    crate::app::tasks::run_optimization_scheduler(state).await
}

pub fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

/// Constant-time equality for secrets, tokens, and their hashes. Both inputs are
/// hashed and the fixed-length digests compared without early exit, so neither
/// the contents nor the lengths leak through timing.
pub fn constant_time_str_eq(left: &str, right: &str) -> bool {
    let left_digest = Sha256::digest(left.as_bytes());
    let right_digest = Sha256::digest(right.as_bytes());
    let mut diff = 0u8;
    for (a, b) in left_digest.iter().zip(right_digest.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Durably write `payload` to `path` via a temp file + fsync + atomic rename.
/// The blocking file work runs on a blocking thread, which keeps fsync off the
/// async reactor and, just as importantly, keeps this helper's async future
/// tiny — it only awaits a join handle rather than holding open `File`s across
/// several `.await`s. Callers persist inside large multi-await futures (e.g. the
/// webhook queue), so an inflated future here compounds into a stack-overflow
/// risk on the default 2 MiB worker/test stack.
async fn write_state_file_atomically(
    path: &std::path::PathBuf,
    payload: String,
) -> anyhow::Result<()> {
    let path = path.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;
        let tmp = path.with_extension("tmp");
        // Write to a temp file and fsync it before the rename so a crash
        // mid-write cannot leave a torn/partial file in place of the real state.
        {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(payload.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        // fsync the parent directory so the rename itself is durable across a
        // crash.
        if let Some(parent) = path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    })
    .await??;
    Ok(())
}

fn automation_status_uses_scheduler_capacity(status: &AutomationRunStatus) -> bool {
    matches!(status, AutomationRunStatus::Running)
}

fn automation_status_holds_workspace_lock(status: &AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Running | AutomationRunStatus::Pausing
    )
}

pub async fn run_routine_scheduler(state: AppState) {
    crate::app::tasks::run_routine_scheduler(state).await
}

pub async fn run_routine_executor(state: AppState) {
    crate::app::tasks::run_routine_executor(state).await
}

pub async fn build_routine_prompt(state: &AppState, run: &RoutineRunRecord) -> String {
    crate::app::routines::build_routine_prompt(state, run).await
}

pub fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_len {
            break;
        }
        end = next;
    }
    let mut out = input[..end].to_string();
    out.push_str("...<truncated>");
    out
}

pub async fn append_configured_output_artifacts(state: &AppState, run: &RoutineRunRecord) {
    crate::app::routines::append_configured_output_artifacts(state, run).await
}

pub fn default_model_spec_from_effective_config(config: &Value) -> Option<ModelSpec> {
    let provider_id = config
        .get("default_provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    let model_id = config
        .get("providers")
        .and_then(|v| v.get(provider_id))
        .and_then(|v| v.get("default_model"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    // Heal a retired openai-codex default so automations/routines that dispatch
    // this as the request model don't hit a provider 400 for an unsupported model.
    let model_id = if provider_id == "openai-codex" {
        tandem_providers::openai_codex_effective_default_model(Some(model_id))
    } else {
        model_id.to_string()
    };
    Some(ModelSpec {
        provider_id: provider_id.to_string(),
        model_id,
    })
}

pub async fn resolve_routine_model_spec_for_run(
    state: &AppState,
    run: &RoutineRunRecord,
) -> (Option<ModelSpec>, String) {
    crate::app::routines::resolve_routine_model_spec_for_run(state, run).await
}

fn normalize_non_empty_list(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in raw {
        let normalized = item.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    out
}

#[cfg(not(feature = "browser"))]
impl AppState {
    pub async fn close_browser_sessions_for_owner(&self, _owner_session_id: &str) -> usize {
        0
    }

    pub async fn close_all_browser_sessions(&self) -> usize {
        0
    }

    pub async fn browser_status(&self) -> serde_json::Value {
        // Mirrors the serialized `tandem_browser::BrowserStatus` readiness
        // contract so /browser/status keeps a stable shape in builds compiled
        // without the `browser` feature.
        serde_json::json!({
            "enabled": false,
            "runnable": false,
            "headless_default": true,
            "sidecar": { "found": false },
            "browser": { "found": false },
            "blocking_issues": [{
                "code": "browser_feature_disabled",
                "message": "this server build was compiled without the `browser` feature",
            }],
            "recommendations": [],
            "install_hints": [],
        })
    }

    pub async fn browser_smoke_test(
        &self,
        _url: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("browser feature disabled")
    }

    pub async fn install_browser_sidecar(&self) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("browser feature disabled")
    }

    pub async fn browser_health_summary(&self) -> serde_json::Value {
        serde_json::json!({ "enabled": false })
    }
}
