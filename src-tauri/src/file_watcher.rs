use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use tauri::{AppHandle, Emitter, WebviewWindow};

/// Watches plan directories for file changes and emits events to the frontend.
pub struct PlanWatcher {
    _watcher: RecommendedWatcher,
}

impl PlanWatcher {
    /// Create a new plan watcher for the given workspace
    pub fn new(workspace_path: &Path, app: AppHandle) -> Result<Self, notify::Error> {
        let canonical_plans_dir = workspace_path.join(".tandem").join("plans");
        let legacy_plans_dir = workspace_path.join(".opencode").join("plans");

        // Create canonical plans directory if it doesn't exist.
        if !canonical_plans_dir.exists() {
            std::fs::create_dir_all(&canonical_plans_dir).ok();
        }

        let (tx, rx): (
            std::sync::mpsc::Sender<Result<Event, notify::Error>>,
            Receiver<Result<Event, notify::Error>>,
        ) = channel();

        let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;

        // Watch canonical plans directory recursively.
        watcher.watch(&canonical_plans_dir, RecursiveMode::Recursive)?;
        // Also watch legacy plans dir when present for read-compatibility during migration window.
        if legacy_plans_dir.exists() {
            watcher.watch(&legacy_plans_dir, RecursiveMode::Recursive)?;
        }

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

/// Watches an arbitrary directory tree and emits debounced change events to the frontend.
///
/// Intended use: refresh the Files view when external processes (or tools) create/delete files.
pub struct FileTreeWatcher {
    _watcher: RecommendedWatcher,
}

impl FileTreeWatcher {
    pub fn new(root: &Path, app: AppHandle, window: WebviewWindow) -> Result<Self, notify::Error> {
        let (tx, rx): (
            std::sync::mpsc::Sender<Result<Event, notify::Error>>,
            Receiver<Result<Event, notify::Error>>,
        ) = channel();

        let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;
        watcher.watch(root, RecursiveMode::Recursive)?;

        let app_clone = app.clone();
        let window_clone = window.clone();
        let root_str = root.to_string_lossy().to_string();
        std::thread::spawn(move || {
            use std::collections::HashSet;

            loop {
                // Block waiting for at least one event. If the sender is dropped, exit.
                let first = match rx.recv() {
                    Ok(v) => v,
                    Err(_) => return,
                };

                let mut pending: HashSet<String> = HashSet::new();
                match first {
                    Ok(event) => {
                        for p in event.paths {
                            if let Some(s) = p.to_str() {
                                pending.insert(s.to_string());
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("[FileTreeWatcher] Watch error: {}", e);
                    }
                }

                // Debounce: collect additional events until the stream goes quiet.
                loop {
                    match rx.recv_timeout(Duration::from_millis(180)) {
                        Ok(Ok(event)) => {
                            for p in event.paths {
                                if let Some(s) = p.to_str() {
                                    pending.insert(s.to_string());
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            tracing::error!("[FileTreeWatcher] Watch error: {}", e);
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }

                let payload = serde_json::json!({
                    "root": root_str,
                    "paths": pending.into_iter().collect::<Vec<_>>(),
                });

                // Emit directly to the active window; AppHandle::emit can be missed depending on scope.
                if let Err(e) = window_clone.emit("file-tree-changed", payload.clone()) {
                    tracing::error!("[FileTreeWatcher] Failed to emit window event: {}", e);
                    // Fallback attempt: app-wide emit for older listeners (best-effort).
                    let _ = app_clone.emit("file-tree-changed", payload);
                }
            }
        });

        Ok(Self { _watcher: watcher })
    }
}
