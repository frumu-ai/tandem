use std::path::{Path, PathBuf};

use tandem_core::resolve_shared_paths;

fn validate_env_path(env_value: &str, env_name: &str) -> Option<PathBuf> {
    let trimmed = env_value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains("..") {
        tracing::warn!(
            "Rejected {} - contains path traversal '..': {}",
            env_name,
            trimmed
        );
        return None;
    }

    let path = PathBuf::from(trimmed);

    if !path.is_absolute() {
        tracing::warn!("Rejected {} - must be absolute path: {}", env_name, trimmed);
        return None;
    }

    match path.canonicalize() {
        Ok(canonical) => {
            let canonical_str = canonical.to_string_lossy().to_string();
            if !is_safe_directory(&canonical) {
                tracing::warn!(
                    "Rejected {} - points to unsafe directory: {}",
                    env_name,
                    canonical_str
                );
                return None;
            }
            Some(canonical)
        }
        Err(e) => {
            tracing::warn!(
                "Rejected {} - path does not exist or cannot be read ({}): {}",
                env_name,
                e,
                trimmed
            );
            None
        }
    }
}

fn is_safe_directory(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_string();

    let unsafe_prefixes = [
        "/etc", "/sys", "/proc", "/root", "/boot", "/dev", "/var/www",
    ];

    for prefix in &unsafe_prefixes {
        if path_str.starts_with(prefix) {
            return false;
        }
    }

    true
}

pub(crate) fn resolve_shared_resources_path() -> PathBuf {
    resolve_canonical_data_file_path("system/shared_resources.json")
}

pub(crate) fn resolve_memory_audit_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        if let Some(base) = validate_env_path(&dir, "TANDEM_STATE_DIR") {
            return if path_is_data_dir(&base) {
                base.join("memory").join("audit.log.jsonl")
            } else {
                base.join("data").join("memory").join("audit.log.jsonl")
            };
        }
    }
    default_state_dir().join("memory").join("audit.log.jsonl")
}

pub(crate) fn resolve_protected_audit_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        if let Some(base) = validate_env_path(&dir, "TANDEM_STATE_DIR") {
            return if path_is_data_dir(&base) {
                base.join("audit").join("protected_events.log.jsonl")
            } else {
                base.join("data")
                    .join("audit")
                    .join("protected_events.log.jsonl")
            };
        }
    }
    default_state_dir()
        .join("audit")
        .join("protected_events.log.jsonl")
}

pub(crate) fn resolve_routines_path() -> PathBuf {
    resolve_canonical_data_file_path("routines/routines.json")
}

pub(crate) fn resolve_routine_history_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STORAGE_DIR") {
        if let Some(base) = validate_env_path(&root, "TANDEM_STORAGE_DIR") {
            return base.join("routine_history.json");
        }
    }
    resolve_canonical_data_file_path("routines/routine_history.json")
}

pub(crate) fn resolve_routine_runs_path() -> PathBuf {
    resolve_canonical_data_file_path("routines/routine_runs.json")
}

pub(crate) fn resolve_automations_v2_path() -> PathBuf {
    resolve_canonical_data_file_path("automations_v2.json")
}

pub(crate) fn resolve_channel_automation_drafts_path() -> PathBuf {
    resolve_canonical_data_file_path("channel_automation_drafts.json")
}

pub(crate) fn resolve_channel_user_capabilities_path() -> PathBuf {
    resolve_canonical_data_file_path("channel_user_capabilities.json")
}

pub(crate) fn resolve_approval_message_map_path() -> PathBuf {
    resolve_canonical_data_file_path("approval_message_map.json")
}

pub(crate) fn resolve_automation_v2_runs_path() -> PathBuf {
    resolve_canonical_data_file_path("automation_v2_runs.json")
}

pub(crate) fn resolve_automation_v2_runs_archive_path() -> PathBuf {
    resolve_canonical_data_file_path("automation_v2_runs_archive.json")
}

pub(crate) fn resolve_automation_governance_path() -> PathBuf {
    resolve_canonical_data_file_path("automation_governance.json")
}

pub(crate) fn resolve_optimization_campaigns_path() -> PathBuf {
    resolve_canonical_data_file_path("optimization_campaigns.json")
}

pub(crate) fn resolve_optimization_experiments_path() -> PathBuf {
    resolve_canonical_data_file_path("optimization_experiments.json")
}

pub(crate) fn resolve_automation_attempt_receipts_dir() -> PathBuf {
    resolve_canonical_data_file_path("automation_attempt_receipts")
}

pub(crate) fn resolve_automation_published_artifacts_dir() -> PathBuf {
    resolve_canonical_data_file_path("automation_published_artifacts")
}

pub(crate) fn resolve_benchmark_runs_dir() -> PathBuf {
    resolve_canonical_data_file_path("benchmarks/runs")
}

pub(crate) fn resolve_canonical_data_file_path(file_name: &str) -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        if let Some(base) = validate_env_path(&root, "TANDEM_STATE_DIR") {
            return if path_is_data_dir(&base) {
                base.join(file_name)
            } else {
                base.join("data").join(file_name)
            };
        }
    }
    default_state_dir().join(file_name)
}

pub(crate) fn resolve_legacy_root_file_path(file_name: &str) -> Option<PathBuf> {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        if let Some(base) = validate_env_path(&root, "TANDEM_STATE_DIR") {
            if !path_is_data_dir(&base) {
                return Some(base.join(file_name));
            }
        }
    }
    resolve_shared_paths()
        .ok()
        .map(|paths| paths.canonical_root.join(file_name))
}

pub(crate) fn resolve_workflow_runs_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        if let Some(base) = validate_env_path(&root, "TANDEM_STATE_DIR") {
            return base.join("workflow_runs.json");
        }
    }
    default_state_dir().join("workflow_runs.json")
}

pub(crate) fn resolve_workflow_planner_sessions_path() -> PathBuf {
    resolve_canonical_data_file_path("workflow-planner/sessions.json")
}

pub(crate) fn resolve_workflow_learning_candidates_path() -> PathBuf {
    resolve_canonical_data_file_path("workflow_learning_candidates.json")
}

pub(crate) fn resolve_context_packs_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        if let Some(base) = validate_env_path(&root, "TANDEM_STATE_DIR") {
            return base.join("context_packs.json");
        }
    }
    default_state_dir().join("context_packs.json")
}

pub(crate) fn resolve_bug_monitor_config_path() -> PathBuf {
    resolve_canonical_data_file_path("bug-monitor/config.json")
}

pub(crate) fn resolve_bug_monitor_drafts_path() -> PathBuf {
    resolve_canonical_data_file_path("bug-monitor/drafts.json")
}

pub(crate) fn resolve_bug_monitor_incidents_path() -> PathBuf {
    resolve_canonical_data_file_path("bug-monitor/incidents.json")
}

pub(crate) fn resolve_bug_monitor_posts_path() -> PathBuf {
    resolve_canonical_data_file_path("bug-monitor/posts.json")
}

pub(crate) fn resolve_bug_monitor_log_watcher_state_path() -> PathBuf {
    resolve_canonical_data_file_path("bug-monitor/log-watcher-state.json")
}

pub(crate) fn resolve_bug_monitor_log_evidence_dir() -> PathBuf {
    resolve_canonical_data_file_path("bug-monitor/evidence")
}

pub(crate) fn resolve_bug_monitor_intake_keys_path() -> PathBuf {
    resolve_canonical_data_file_path("bug-monitor/intake-keys.json")
}

pub(crate) fn resolve_external_actions_path() -> PathBuf {
    resolve_canonical_data_file_path("actions/external_actions.json")
}

pub(crate) fn legacy_failure_reporter_path(file_name: &str) -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join(file_name);
        }
    }
    default_state_dir().join(file_name)
}

pub(crate) fn resolve_workflow_hook_overrides_path() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("workflow_hook_overrides.json");
        }
    }
    default_state_dir().join("workflow_hook_overrides.json")
}

pub(crate) fn resolve_builtin_workflows_dir() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_BUILTIN_WORKFLOW_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    default_state_dir().join("builtin_workflows")
}

pub(crate) fn resolve_agent_team_audit_path() -> PathBuf {
    if let Ok(base) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = base.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed)
                .join("agent-team")
                .join("audit.log.jsonl");
        }
    }
    default_state_dir()
        .join("agent-team")
        .join("audit.log.jsonl")
}

pub(crate) fn default_state_dir() -> PathBuf {
    if let Ok(paths) = resolve_shared_paths() {
        return paths.engine_state_dir;
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("tandem").join("data");
    }
    dirs::home_dir()
        .map(|home| home.join(".tandem").join("data"))
        .unwrap_or_else(|| PathBuf::from(".tandem"))
}

pub(crate) fn sibling_backup_path(path: &PathBuf) -> PathBuf {
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    let backup_name = format!("{base}.bak");
    path.with_file_name(backup_name)
}

pub(crate) fn sibling_tmp_path(path: &PathBuf) -> PathBuf {
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    let tmp_name = format!("{base}.tmp");
    path.with_file_name(tmp_name)
}

fn path_is_data_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("data"))
        .unwrap_or(false)
}
