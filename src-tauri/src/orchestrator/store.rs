// Orchestrator Store
// Persistence layer for run state, tasks, budget, and event logs
// See: docs/orchestration_plan.md

use crate::error::{Result, TandemError};
use crate::orchestrator::types::{Budget, OrchestratorEvent, Run, Task};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

// ============================================================================
// Orchestrator Store
// ============================================================================

/// Persistence layer for orchestrator state
pub struct OrchestratorStore {
    /// Base directory for orchestrator data
    base_dir: PathBuf,
}

impl OrchestratorStore {
    /// Create a new store at the given workspace path
    pub fn new(workspace_path: &Path) -> Result<Self> {
        let base_dir = workspace_path.join(".tandem").join("orchestrator");

        // Ensure base directory exists
        fs::create_dir_all(&base_dir).map_err(|e| {
            TandemError::IoError(format!("Failed to create orchestrator directory: {}", e))
        })?;

        Ok(Self { base_dir })
    }

    /// Get the run directory for a specific run
    fn run_dir(&self, run_id: &str) -> PathBuf {
        self.base_dir.join(run_id)
    }

    /// Create a new run directory
    pub fn create_run_dir(&self, run_id: &str) -> Result<PathBuf> {
        let dir = self.run_dir(run_id);
        fs::create_dir_all(&dir)
            .map_err(|e| TandemError::IoError(format!("Failed to create run directory: {}", e)))?;

        // Create artifacts subdirectory
        fs::create_dir_all(dir.join("artifacts")).map_err(|e| {
            TandemError::IoError(format!("Failed to create artifacts directory: {}", e))
        })?;

        Ok(dir)
    }

    /// Save run state
    pub fn save_run(&self, run: &Run) -> Result<()> {
        let dir = self.run_dir(&run.run_id);
        fs::create_dir_all(&dir)
            .map_err(|e| TandemError::IoError(format!("Failed to create run directory: {}", e)))?;

        let path = dir.join("run.json");
        let content = serde_json::to_string_pretty(run).map_err(|e| {
            TandemError::SerializationError(format!("Failed to serialize run: {}", e))
        })?;

        atomic_write(&path, &content)
    }

    /// Load run state
    pub fn load_run(&self, run_id: &str) -> Result<Run> {
        let path = self.run_dir(run_id).join("run.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to read run file: {}", e)))?;

        serde_json::from_str(&content)
            .map_err(|e| TandemError::ParseError(format!("Failed to parse run file: {}", e)))
    }

    /// Save task list
    pub fn save_tasks(&self, run_id: &str, tasks: &[Task]) -> Result<()> {
        let path = self.run_dir(run_id).join("tasks.json");
        let content = serde_json::to_string_pretty(tasks).map_err(|e| {
            TandemError::SerializationError(format!("Failed to serialize tasks: {}", e))
        })?;

        atomic_write(&path, &content)
    }

    /// Load task list
    pub fn load_tasks(&self, run_id: &str) -> Result<Vec<Task>> {
        let path = self.run_dir(run_id).join("tasks.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to read tasks file: {}", e)))?;

        serde_json::from_str(&content)
            .map_err(|e| TandemError::ParseError(format!("Failed to parse tasks file: {}", e)))
    }

    /// Save budget state
    pub fn save_budget(&self, run_id: &str, budget: &Budget) -> Result<()> {
        let path = self.run_dir(run_id).join("budget.json");
        let content = serde_json::to_string_pretty(budget).map_err(|e| {
            TandemError::SerializationError(format!("Failed to serialize budget: {}", e))
        })?;

        atomic_write(&path, &content)
    }

    /// Load budget state
    pub fn load_budget(&self, run_id: &str) -> Result<Budget> {
        let path = self.run_dir(run_id).join("budget.json");
        let content = fs::read_to_string(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to read budget file: {}", e)))?;

        serde_json::from_str(&content)
            .map_err(|e| TandemError::ParseError(format!("Failed to parse budget file: {}", e)))
    }

    /// Append event to log
    pub fn append_event(&self, run_id: &str, event: &OrchestratorEvent) -> Result<()> {
        let path = self.run_dir(run_id).join("events.log");

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to open events log: {}", e)))?;

        let line = serde_json::to_string(event).map_err(|e| {
            TandemError::SerializationError(format!("Failed to serialize event: {}", e))
        })?;

        writeln!(file, "{}", line)
            .map_err(|e| TandemError::IoError(format!("Failed to write event: {}", e)))?;

        Ok(())
    }

    /// Load all events for a run
    pub fn load_events(&self, run_id: &str) -> Result<Vec<OrchestratorEvent>> {
        let path = self.run_dir(run_id).join("events.log");

        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to open events log: {}", e)))?;

        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line.map_err(|e| {
                TandemError::IoError(format!("Failed to read events log line: {}", e))
            })?;

            if let Ok(event) = serde_json::from_str(&line) {
                events.push(event);
            }
        }

        Ok(events)
    }

    /// Save summary markdown
    pub fn save_summary(&self, run_id: &str, summary: &str) -> Result<()> {
        let path = self.run_dir(run_id).join("latest_summary.md");
        atomic_write(&path, summary)
    }

    /// Load summary
    pub fn load_summary(&self, run_id: &str) -> Result<String> {
        let path = self.run_dir(run_id).join("latest_summary.md");
        fs::read_to_string(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to read summary: {}", e)))
    }

    /// Save artifact for a task
    pub fn save_artifact(
        &self,
        run_id: &str,
        task_id: &str,
        filename: &str,
        content: &str,
    ) -> Result<PathBuf> {
        let artifact_dir = self.run_dir(run_id).join("artifacts").join(task_id);
        fs::create_dir_all(&artifact_dir).map_err(|e| {
            TandemError::IoError(format!("Failed to create artifact directory: {}", e))
        })?;

        let path = artifact_dir.join(filename);
        atomic_write(&path, content)?;

        Ok(path)
    }

    /// Load artifact
    pub fn load_artifact(&self, run_id: &str, task_id: &str, filename: &str) -> Result<String> {
        let path = self
            .run_dir(run_id)
            .join("artifacts")
            .join(task_id)
            .join(filename);

        fs::read_to_string(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to read artifact: {}", e)))
    }

    /// List all runs
    pub fn list_runs(&self) -> Result<Vec<String>> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut runs = Vec::new();

        for entry in fs::read_dir(&self.base_dir).map_err(|e| {
            TandemError::IoError(format!("Failed to read orchestrator directory: {}", e))
        })? {
            let entry = entry.map_err(|e| {
                TandemError::IoError(format!("Failed to read directory entry: {}", e))
            })?;

            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    runs.push(name.to_string());
                }
            }
        }

        Ok(runs)
    }

    /// Delete a run
    pub fn delete_run(&self, run_id: &str) -> Result<()> {
        let dir = self.run_dir(run_id);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| {
                TandemError::IoError(format!("Failed to delete run directory: {}", e))
            })?;
        }
        Ok(())
    }

    /// Check if a run exists
    pub fn run_exists(&self, run_id: &str) -> bool {
        self.run_dir(run_id).join("run.json").exists()
    }
}

/// Atomic write using temp file and rename
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let temp_path = path.with_extension("tmp");

    fs::write(&temp_path, content)
        .map_err(|e| TandemError::IoError(format!("Failed to write temp file: {}", e)))?;

    fs::rename(&temp_path, path)
        .map_err(|e| TandemError::IoError(format!("Failed to rename temp file: {}", e)))?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::types::{OrchestratorConfig, TaskState};
    use tempfile::tempdir;

    #[test]
    fn test_create_run_dir() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();

        let run_dir = store.create_run_dir("test_run").unwrap();
        assert!(run_dir.exists());
        assert!(run_dir.join("artifacts").exists());
    }

    #[test]
    fn test_save_load_run() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();

        let run = Run::new(
            "run_1".to_string(),
            "session_1".to_string(),
            "Test objective".to_string(),
            OrchestratorConfig::default(),
        );

        store.save_run(&run).unwrap();
        let loaded = store.load_run("run_1").unwrap();

        assert_eq!(loaded.run_id, run.run_id);
        assert_eq!(loaded.objective, run.objective);
    }

    #[test]
    fn test_save_load_tasks() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();
        store.create_run_dir("run_1").unwrap();

        let tasks = vec![
            Task::new("1".to_string(), "Task 1".to_string(), "Desc 1".to_string()),
            Task::new("2".to_string(), "Task 2".to_string(), "Desc 2".to_string()),
        ];

        store.save_tasks("run_1", &tasks).unwrap();
        let loaded = store.load_tasks("run_1").unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "1");
    }

    #[test]
    fn test_append_load_events() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();
        store.create_run_dir("run_1").unwrap();

        let event1 = OrchestratorEvent::RunCreated {
            run_id: "run_1".to_string(),
            objective: "Test".to_string(),
            timestamp: chrono::Utc::now(),
        };

        let event2 = OrchestratorEvent::PlanningStarted {
            run_id: "run_1".to_string(),
            timestamp: chrono::Utc::now(),
        };

        store.append_event("run_1", &event1).unwrap();
        store.append_event("run_1", &event2).unwrap();

        let events = store.load_events("run_1").unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_save_load_artifact() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();
        store.create_run_dir("run_1").unwrap();

        let content = "--- a/file.rs\n+++ b/file.rs\n@@ test @@";
        let path = store
            .save_artifact("run_1", "task_1", "patch.diff", content)
            .unwrap();

        assert!(path.exists());

        let loaded = store
            .load_artifact("run_1", "task_1", "patch.diff")
            .unwrap();
        assert_eq!(loaded, content);
    }

    #[test]
    fn test_list_runs() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();

        store.create_run_dir("run_1").unwrap();
        store.create_run_dir("run_2").unwrap();

        // Create a non-directory file to ensure it's ignored
        std::fs::write(
            temp.path()
                .join(".tandem")
                .join("orchestrator")
                .join("some_file"),
            "content",
        )
        .unwrap();

        let runs = store.list_runs().unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs.contains(&"run_1".to_string()));
        assert!(runs.contains(&"run_2".to_string()));
    }
}
