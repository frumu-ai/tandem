// Project Management (Multi-Workspace Support)
// ============================================================================

/// Check if a directory is a Git repository
#[tauri::command]
pub fn is_git_repo(path: String) -> bool {
    let git_dir = PathBuf::from(&path).join(".git");
    git_dir.exists() && git_dir.is_dir()
}

/// Check if Git is installed on the system
#[tauri::command]
pub fn is_git_installed() -> bool {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("--version");

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.output().is_ok()
}

/// Initialize a Git repository in the specified directory
#[tauri::command]
pub fn initialize_git_repo(path: String) -> Result<()> {
    let path_buf = PathBuf::from(&path);

    // Verify the path exists and is a directory
    if !path_buf.exists() || !path_buf.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Invalid directory: {}",
            path
        )));
    }

    // Check if already a git repo
    if is_git_repo(path.clone()) {
        return Ok(()); // Already initialized, no-op
    }

    // Run git init
    let mut cmd = std::process::Command::new("git");
    cmd.arg("init").current_dir(&path_buf);

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd
        .output()
        .map_err(|e| TandemError::Sidecar(format!("Failed to run git init: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TandemError::Sidecar(format!("git init failed: {}", stderr)));
    }

    tracing::info!("Initialized Git repository at: {}", path);
    Ok(())
}

/// Get comprehensive Git status for a directory
#[derive(serde::Serialize)]
pub struct GitStatus {
    pub git_installed: bool,
    pub is_repo: bool,
    pub can_enable_undo: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UserRepoContext {
    pub workspace_root: String,
    pub repo_root: Option<String>,
    pub repo_slug: Option<String>,
    pub current_branch: Option<String>,
    pub default_branch: Option<String>,
    pub git_installed: bool,
    pub is_repo: bool,
}

fn run_git_capture(path: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(path);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn parse_repo_slug(remote: &str) -> Option<String> {
    let trimmed = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("git@") {
        let mut parts = rest.splitn(2, ':');
        let _host = parts.next()?;
        let path = parts.next()?.trim_matches('/');
        return if path.is_empty() {
            None
        } else {
            Some(path.to_string())
        };
    }
    if let Some(index) = trimmed.find("://") {
        let without_scheme = &trimmed[index + 3..];
        let mut parts = without_scheme.splitn(2, '/');
        let _host = parts.next()?;
        let path = parts.next()?.trim_matches('/');
        return if path.is_empty() {
            None
        } else {
            Some(path.to_string())
        };
    }
    let normalized = trimmed.trim_matches('/');
    if normalized.contains('/') {
        Some(normalized.to_string())
    } else {
        None
    }
}

#[tauri::command]
pub fn resolve_user_repo_context(path: String) -> Result<UserRepoContext> {
    let path_buf = PathBuf::from(&path);
    if !path_buf.exists() || !path_buf.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Invalid directory: {}",
            path
        )));
    }

    let git_installed = is_git_installed();
    if !git_installed {
        return Ok(UserRepoContext {
            workspace_root: path,
            repo_root: None,
            repo_slug: None,
            current_branch: None,
            default_branch: None,
            git_installed: false,
            is_repo: false,
        });
    }

    let repo_root = run_git_capture(&path_buf, &["rev-parse", "--show-toplevel"]);
    let is_repo = repo_root.is_some();
    let current_branch = if is_repo {
        run_git_capture(&path_buf, &["branch", "--show-current"])
    } else {
        None
    };
    let remote_url = if is_repo {
        run_git_capture(&path_buf, &["remote", "get-url", "origin"])
    } else {
        None
    };
    let repo_slug = remote_url.as_deref().and_then(parse_repo_slug);
    let default_branch = if is_repo {
        run_git_capture(
            &path_buf,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        )
        .map(|value| value.trim_start_matches("origin/").to_string())
        .or_else(|| run_git_capture(&path_buf, &["config", "--get", "init.defaultBranch"]))
    } else {
        None
    };

    Ok(UserRepoContext {
        workspace_root: path,
        repo_root,
        repo_slug,
        current_branch,
        default_branch,
        git_installed,
        is_repo,
    })
}

#[tauri::command]
pub fn check_git_status(path: String) -> GitStatus {
    let git_installed = is_git_installed();
    let is_repo = is_git_repo(path);
    let can_enable_undo = git_installed && !is_repo;

    GitStatus {
        git_installed,
        is_repo,
        can_enable_undo,
    }
}

/// Add a new project folder
#[tauri::command]
pub fn add_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<crate::state::UserProject> {
    use crate::state::UserProject;

    let path_buf = PathBuf::from(&path);

    // Verify the path exists and is a directory
    if !path_buf.exists() {
        return Err(TandemError::NotFound(format!(
            "Path does not exist: {}",
            path
        )));
    }

    if !path_buf.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a directory: {}",
            path
        )));
    }

    migrate_workspace_legacy_namespace_if_needed(&path_buf)?;

    // Create new project
    let project = UserProject::new(path_buf, name);

    // Add to state
    {
        let mut projects = state.user_projects.write().unwrap();
        projects.push(project.clone());
    }

    // Save to store
    if let Ok(store) = app.store("settings.json") {
        let projects = state.user_projects.read().unwrap();
        store.set("user_projects", serde_json::to_value(&*projects).unwrap());
        let _ = store.save();
    }

    tracing::info!("Added project: {} at {}", project.name, project.path);

    Ok(project)
}

/// Remove a project
#[tauri::command]
pub fn remove_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<()> {
    // Remove from state
    {
        let mut projects = state.user_projects.write().unwrap();
        projects.retain(|p| p.id != project_id);
    }

    // If this was the active project, clear it
    {
        let active_id = state.active_project_id.read().unwrap();
        if active_id.as_ref() == Some(&project_id) {
            drop(active_id);
            let mut active = state.active_project_id.write().unwrap();
            *active = None;

            // Also clear workspace path
            let mut workspace = state.workspace_path.write().unwrap();
            *workspace = None;
        }
    }

    // Save to store
    if let Ok(store) = app.store("settings.json") {
        let projects = state.user_projects.read().unwrap();
        store.set("user_projects", serde_json::to_value(&*projects).unwrap());

        let active_id = state.active_project_id.read().unwrap();
        if active_id.is_none() {
            let _ = store.delete("active_project_id");
        }

        let _ = store.save();
    }

    tracing::info!("Removed project: {}", project_id);

    Ok(())
}

/// Get all user projects
#[tauri::command]
pub fn get_user_projects(state: State<'_, AppState>) -> Vec<crate::state::UserProject> {
    let projects = state.user_projects.read().unwrap();
    projects.clone()
}

/// Set the active project (and update workspace)
#[tauri::command]
pub async fn set_active_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<()> {
    use crate::state::UserProject;

    // Find the project
    let project: UserProject = {
        let projects = state.user_projects.read().unwrap();
        projects
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Project not found: {}", project_id)))?
    };

    // Set as active
    {
        let mut active = state.active_project_id.write().unwrap();
        *active = Some(project_id.clone());
    }

    // Update last accessed time
    {
        let mut projects = state.user_projects.write().unwrap();
        if let Some(p) = projects.iter_mut().find(|p| p.id == project_id) {
            p.last_accessed = chrono::Utc::now();
        }
    }

    // Set workspace path
    let path_buf = project.path_buf();
    migrate_workspace_legacy_namespace_if_needed(&path_buf)?;
    state.set_workspace(path_buf.clone());
    let normalized_workspace = normalize_workspace_path(&path_buf.to_string_lossy())
        .unwrap_or_else(|| path_buf.to_string_lossy().to_string());

    // Invalidate orchestrator engines bound to other workspaces so new runs always use
    // the active project context.
    {
        let mut engines = state.orchestrator_engines.write().unwrap();
        let before = engines.len();
        engines.retain(|_, engine| {
            normalize_workspace_path(&engine.workspace_path_string())
                .map(|root| root == normalized_workspace)
                .unwrap_or(false)
        });
        let removed = before.saturating_sub(engines.len());
        if removed > 0 {
            tracing::info!(
                "Workspace switch invalidated {} orchestrator engine(s); active workspace={}",
                removed,
                normalized_workspace
            );
        }
    }

    // Update sidecar workspace - this sets it for when sidecar restarts
    state.sidecar.set_workspace(path_buf.clone()).await;

    // Restart the sidecar if it's running so it picks up the new workspace.
    // In shared-engine mode we do not restart, because other clients (Desktop/TUI) may be attached.
    let sidecar_state = state.sidecar.state().await;
    if sidecar_state == crate::sidecar::SidecarState::Running {
        if state.sidecar.shared_mode().await {
            tracing::info!(
                "Shared sidecar mode enabled; skipping sidecar restart on workspace switch"
            );
        } else {
            tracing::info!("Restarting sidecar with new workspace: {}", project.path);

            // Stop the sidecar
            let _ = state.sidecar.stop().await;

            // Wait a moment for cleanup
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Get the sidecar path (checks AppData first, then resources)
            let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;

            // Sync env vars BEFORE starting so the sidecar actually picks them up.
            let providers = {
                let config = state.providers_config.read().unwrap();
                config.clone()
            };
            let _ = sync_provider_config_file(&providers);
            sync_ollama_env(&state, &providers).await;
            sync_provider_keys_env(&app, &state, &providers).await;
            sync_channel_tokens_env(&app, &state).await;

            // Restart with new workspace
            state
                .sidecar
                .start_with_app(Some(app.clone()), sidecar_path.to_string_lossy().as_ref())
                .await?;

            tracing::info!("Sidecar restarted successfully");
        }
    }

    // Save to store
    if let Ok(store) = app.store("settings.json") {
        store.set("active_project_id", serde_json::json!(project_id));
        let projects = state.user_projects.read().unwrap();
        store.set("user_projects", serde_json::to_value(&*projects).unwrap());
        let _ = store.save();
    }

    tracing::info!("Set active project: {} ({})", project.name, project.path);

    Ok(())
}

/// Get the active project
#[tauri::command]
pub fn get_active_project(state: State<'_, AppState>) -> Option<crate::state::UserProject> {
    let active_id = state.active_project_id.read().unwrap();
    let project_id = active_id.as_ref()?;

    let projects = state.user_projects.read().unwrap();
    projects.iter().find(|p| &p.id == project_id).cloned()
}
