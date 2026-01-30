use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use tauri::{AppHandle, Emitter};

/// Watches the `.opencode/plans/` directory for file changes and emits events to the frontend
pub struct PlanWatcher {
    _watcher: RecommendedWatcher,
}

impl PlanWatcher {
    /// Create a new plan watcher for the given workspace
    pub fn new(workspace_path: &Path, app: AppHandle) -> Result<Self, notify::Error> {
        let plans_dir = workspace_path.join(".opencode/plans");

        // Create the plans directory if it doesn't exist
        if !plans_dir.exists() {
            std::fs::create_dir_all(&plans_dir).ok();
        }

        let (tx, rx): (
            std::sync::mpsc::Sender<Result<Event, notify::Error>>,
            Receiver<Result<Event, notify::Error>>,
        ) = channel();

        let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;

        // Watch the plans directory recursively
        watcher.watch(&plans_dir, RecursiveMode::Recursive)?;

        // Spawn a task to handle events
        let app_clone = app.clone();
        std::thread::spawn(move || {
            for res in rx {
                match res {
                    Ok(event) => {
                        tracing::debug!("[PlanWatcher] File event: {:?}", event);

                        // Extract paths from event
                        let paths: Vec<String> = event
                            .paths
                            .iter()
                            .filter_map(|p| p.to_str().map(String::from))
                            .collect();

                        // Emit event to frontend
                        if let Err(e) = app_clone.emit("plan-file-changed", paths) {
                            tracing::error!("[PlanWatcher] Failed to emit event: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("[PlanWatcher] Watch error: {}", e);
                    }
                }
            }
        });

        Ok(Self { _watcher: watcher })
    }
}
