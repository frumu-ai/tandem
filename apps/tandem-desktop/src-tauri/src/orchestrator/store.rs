// Orchestrator Store
// Persistence layer for run state, tasks, budget, and event logs
// See: docs/orchestration_plan.md

use crate::error::{Result, TandemError};
use crate::orchestrator::types::{
    Blackboard, BlackboardArtifactRef, BlackboardItem, BlackboardPatchOp, BlackboardPatchRecord,
    Budget, CheckpointSnapshot, OrchestratorEvent, Run, RunEventRecord, Task,
};
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

    /// Append legacy event to log
    pub fn append_event(&self, run_id: &str, event: &OrchestratorEvent) -> Result<()> {
        let run_dir = self.run_dir(run_id);
        // Ensure the run directory exists. The engine can emit events before the run is fully
        // persisted/created on disk (e.g. on early failures), and Windows will error with
        // "The system cannot find the path specified" if the parent dir doesn't exist.
        fs::create_dir_all(&run_dir)
            .map_err(|e| TandemError::IoError(format!("Failed to create run dir: {}", e)))?;

        let path = run_dir.join("events.log");

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

    /// Append sequenced event to the canonical JSONL log.
    pub fn append_run_event(&self, run_id: &str, record: &RunEventRecord) -> Result<()> {
        let run_dir = self.run_dir(run_id);
        fs::create_dir_all(&run_dir)
            .map_err(|e| TandemError::IoError(format!("Failed to create run dir: {}", e)))?;

        let path = run_dir.join("events.jsonl");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to open events jsonl: {}", e)))?;
        let line = serde_json::to_string(record).map_err(|e| {
            TandemError::SerializationError(format!("Failed to serialize run event: {}", e))
        })?;
        writeln!(file, "{}", line)
            .map_err(|e| TandemError::IoError(format!("Failed to write run event: {}", e)))?;
        Ok(())
    }

    pub fn latest_run_event_seq(&self, run_id: &str) -> Result<u64> {
        let path = self.run_dir(run_id).join("events.jsonl");
        if !path.exists() {
            return Ok(0);
        }
        let file = File::open(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to open events jsonl: {}", e)))?;
        let reader = BufReader::new(file);
        let mut latest = 0u64;
        for line in reader.lines() {
            let line = line
                .map_err(|e| TandemError::IoError(format!("Failed reading events jsonl: {}", e)))?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<RunEventRecord>(&line) {
                latest = latest.max(record.seq);
            }
        }
        Ok(latest)
    }

    pub fn load_run_events(
        &self,
        run_id: &str,
        since_seq: Option<u64>,
        tail: Option<usize>,
    ) -> Result<Vec<RunEventRecord>> {
        let path = self.run_dir(run_id).join("events.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to open events jsonl: {}", e)))?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();
        for line in reader.lines() {
            let line = line
                .map_err(|e| TandemError::IoError(format!("Failed reading events jsonl: {}", e)))?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<RunEventRecord>(&line) {
                if let Some(min_seq) = since_seq {
                    if record.seq <= min_seq {
                        continue;
                    }
                }
                records.push(record);
            }
        }
        records.sort_by_key(|record| record.seq);
        if let Some(tail_count) = tail {
            if records.len() > tail_count {
                records = records.split_off(records.len() - tail_count);
            }
        }
        Ok(records)
    }

    /// Append blackboard patch to append-only patch log and update materialized view.
    pub fn append_blackboard_patch(
        &self,
        run_id: &str,
        patch: &BlackboardPatchRecord,
    ) -> Result<()> {
        let run_dir = self.run_dir(run_id);
        fs::create_dir_all(&run_dir)
            .map_err(|e| TandemError::IoError(format!("Failed to create run dir: {}", e)))?;

        let patch_path = run_dir.join("blackboard_patches.jsonl");
        let mut patch_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&patch_path)
            .map_err(|e| {
                TandemError::IoError(format!("Failed to open blackboard patches jsonl: {}", e))
            })?;
        let patch_line = serde_json::to_string(patch).map_err(|e| {
            TandemError::SerializationError(format!("Failed to serialize blackboard patch: {}", e))
        })?;
        writeln!(patch_file, "{}", patch_line).map_err(|e| {
            TandemError::IoError(format!("Failed to write blackboard patch: {}", e))
        })?;

        let mut materialized = self.load_blackboard(run_id).unwrap_or_default();
        apply_blackboard_patch(&mut materialized, patch)?;
        materialized.revision = patch.seq;

        let materialized_path = run_dir.join("blackboard.json");
        let content = serde_json::to_string_pretty(&materialized).map_err(|e| {
            TandemError::SerializationError(format!(
                "Failed to serialize materialized blackboard: {}",
                e
            ))
        })?;
        atomic_write(&materialized_path, &content)?;
        Ok(())
    }

    pub fn load_blackboard_patches(
        &self,
        run_id: &str,
        since_seq: Option<u64>,
        tail: Option<usize>,
    ) -> Result<Vec<BlackboardPatchRecord>> {
        let path = self.run_dir(run_id).join("blackboard_patches.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&path).map_err(|e| {
            TandemError::IoError(format!("Failed to open blackboard patches jsonl: {}", e))
        })?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|e| {
                TandemError::IoError(format!("Failed reading blackboard patches jsonl: {}", e))
            })?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<BlackboardPatchRecord>(&line) {
                if let Some(min_seq) = since_seq {
                    if record.seq <= min_seq {
                        continue;
                    }
                }
                records.push(record);
            }
        }
        records.sort_by_key(|record| record.seq);
        if let Some(tail_count) = tail {
            if records.len() > tail_count {
                records = records.split_off(records.len() - tail_count);
            }
        }
        Ok(records)
    }

    pub fn load_blackboard(&self, run_id: &str) -> Result<Blackboard> {
        let path = self.run_dir(run_id).join("blackboard.json");
        if !path.exists() {
            return Ok(Blackboard::default());
        }
        let content = fs::read_to_string(&path).map_err(|e| {
            TandemError::IoError(format!(
                "Failed to read blackboard materialized view: {}",
                e
            ))
        })?;
        serde_json::from_str(&content).map_err(|e| {
            TandemError::ParseError(format!(
                "Failed to parse blackboard materialized view: {}",
                e
            ))
        })
    }

    pub fn save_checkpoint(&self, run_id: &str, checkpoint: &CheckpointSnapshot) -> Result<()> {
        let checkpoint_dir = self.run_dir(run_id).join("checkpoints");
        fs::create_dir_all(&checkpoint_dir).map_err(|e| {
            TandemError::IoError(format!("Failed to create checkpoint directory: {}", e))
        })?;

        let file_name = format!("{:020}.json", checkpoint.seq);
        let path = checkpoint_dir.join(file_name);
        let content = serde_json::to_string_pretty(checkpoint).map_err(|e| {
            TandemError::SerializationError(format!("Failed to serialize checkpoint: {}", e))
        })?;
        atomic_write(&path, &content)
    }

    pub fn load_latest_checkpoint(&self, run_id: &str) -> Result<Option<CheckpointSnapshot>> {
        let checkpoint_dir = self.run_dir(run_id).join("checkpoints");
        if !checkpoint_dir.exists() {
            return Ok(None);
        }

        let mut latest_name: Option<String> = None;
        for entry in fs::read_dir(&checkpoint_dir).map_err(|e| {
            TandemError::IoError(format!("Failed to read checkpoint directory: {}", e))
        })? {
            let entry = entry.map_err(|e| {
                TandemError::IoError(format!("Failed to read checkpoint entry: {}", e))
            })?;
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") {
                continue;
            }
            let should_replace = match latest_name.as_ref() {
                Some(current) => name > *current,
                None => true,
            };
            if should_replace {
                latest_name = Some(name);
            }
        }

        let Some(file_name) = latest_name else {
            return Ok(None);
        };
        let path = checkpoint_dir.join(file_name);
        let content = fs::read_to_string(&path)
            .map_err(|e| TandemError::IoError(format!("Failed to read checkpoint file: {}", e)))?;
        let snapshot = serde_json::from_str(&content).map_err(|e| {
            TandemError::ParseError(format!("Failed to parse checkpoint file: {}", e))
        })?;
        Ok(Some(snapshot))
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

fn apply_blackboard_patch(
    blackboard: &mut Blackboard,
    patch: &BlackboardPatchRecord,
) -> Result<()> {
    match patch.op {
        BlackboardPatchOp::AddFact => {
            let value: BlackboardItem =
                serde_json::from_value(patch.payload.clone()).map_err(|e| {
                    TandemError::ParseError(format!("Invalid AddFact patch payload: {}", e))
                })?;
            blackboard.facts.push(value);
        }
        BlackboardPatchOp::AddDecision => {
            let value: BlackboardItem =
                serde_json::from_value(patch.payload.clone()).map_err(|e| {
                    TandemError::ParseError(format!("Invalid AddDecision patch payload: {}", e))
                })?;
            blackboard.decisions.push(value);
        }
        BlackboardPatchOp::AddOpenQuestion => {
            let value: BlackboardItem =
                serde_json::from_value(patch.payload.clone()).map_err(|e| {
                    TandemError::ParseError(format!("Invalid AddOpenQuestion patch payload: {}", e))
                })?;
            blackboard.open_questions.push(value);
        }
        BlackboardPatchOp::AddArtifact => {
            let value: BlackboardArtifactRef = serde_json::from_value(patch.payload.clone())
                .map_err(|e| {
                    TandemError::ParseError(format!("Invalid AddArtifact patch payload: {}", e))
                })?;
            blackboard.artifacts.push(value);
        }
        BlackboardPatchOp::SetRollingSummary => {
            let value = patch
                .payload
                .as_str()
                .ok_or_else(|| {
                    TandemError::ParseError(
                        "Invalid SetRollingSummary patch payload: expected string".to_string(),
                    )
                })?
                .to_string();
            blackboard.summaries.rolling = value;
        }
        BlackboardPatchOp::SetLatestContextPack => {
            let value = patch
                .payload
                .as_str()
                .ok_or_else(|| {
                    TandemError::ParseError(
                        "Invalid SetLatestContextPack patch payload: expected string".to_string(),
                    )
                })?
                .to_string();
            blackboard.summaries.latest_context_pack = value;
        }
    }
    Ok(())
}

/// Atomic write using temp file and rename with restrictive permissions
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let temp_path = path.with_extension("tmp");

    fs::write(&temp_path, content)
        .map_err(|e| TandemError::IoError(format!("Failed to write temp file: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600)).map_err(|e| {
            TandemError::IoError(format!("Failed to set temp file permissions: {}", e))
        })?;
    }

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
    use crate::orchestrator::types::{
        BlackboardItem, BlackboardPatchOp, BlackboardPatchRecord, CheckpointSnapshot,
        OrchestratorConfig, RunStatus,
    };
    use serde_json::json;
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

    #[test]
    fn test_append_and_query_sequenced_run_events() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();
        store.create_run_dir("run_1").unwrap();

        for seq in 1..=5u64 {
            store
                .append_run_event(
                    "run_1",
                    &RunEventRecord {
                        event_id: format!("evt-{}", seq),
                        run_id: "run_1".to_string(),
                        seq,
                        ts_ms: seq * 1000,
                        event_type: "task_trace".to_string(),
                        status: RunStatus::Running,
                        step_id: Some("task-1".to_string()),
                        tenant_context: tandem_enterprise_contract::LocalImplicitTenant.into(),
                        authority_chain: None,
                        payload: json!({ "idx": seq }),
                    },
                )
                .unwrap();
        }

        assert_eq!(store.latest_run_event_seq("run_1").unwrap(), 5);

        let since_three = store.load_run_events("run_1", Some(3), None).unwrap();
        assert_eq!(since_three.len(), 2);
        assert_eq!(since_three[0].seq, 4);
        assert_eq!(since_three[1].seq, 5);

        let tail_two = store.load_run_events("run_1", None, Some(2)).unwrap();
        assert_eq!(tail_two.len(), 2);
        assert_eq!(tail_two[0].seq, 4);
        assert_eq!(tail_two[1].seq, 5);
    }

    #[test]
    fn test_blackboard_patch_append_and_materialization() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();
        store.create_run_dir("run_1").unwrap();

        store
            .append_blackboard_patch(
                "run_1",
                &BlackboardPatchRecord {
                    patch_id: "patch-1".to_string(),
                    run_id: "run_1".to_string(),
                    seq: 1,
                    ts_ms: 1_000,
                    op: BlackboardPatchOp::AddFact,
                    payload: serde_json::to_value(BlackboardItem {
                        id: "fact-1".to_string(),
                        ts_ms: 1_000,
                        text: "Task task-1 passed validation".to_string(),
                        step_id: Some("task-1".to_string()),
                        source_event_id: Some("evt-1".to_string()),
                    })
                    .unwrap(),
                },
            )
            .unwrap();

        store
            .append_blackboard_patch(
                "run_1",
                &BlackboardPatchRecord {
                    patch_id: "patch-2".to_string(),
                    run_id: "run_1".to_string(),
                    seq: 2,
                    ts_ms: 2_000,
                    op: BlackboardPatchOp::SetRollingSummary,
                    payload: json!("run is progressing"),
                },
            )
            .unwrap();

        let blackboard = store.load_blackboard("run_1").unwrap();
        assert_eq!(blackboard.revision, 2);
        assert_eq!(blackboard.facts.len(), 1);
        assert_eq!(blackboard.facts[0].id, "fact-1");
        assert_eq!(blackboard.summaries.rolling, "run is progressing");

        let patches = store
            .load_blackboard_patches("run_1", Some(1), Some(10))
            .unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].patch_id, "patch-2");
    }

    #[test]
    fn test_checkpoint_save_and_latest_load() {
        let temp = tempdir().unwrap();
        let store = OrchestratorStore::new(temp.path()).unwrap();
        store.create_run_dir("run_1").unwrap();

        let run = Run::new(
            "run_1".to_string(),
            "session_1".to_string(),
            "checkpoint objective".to_string(),
            OrchestratorConfig::default(),
        );
        let budget = run.budget.clone();
        let cp1 = CheckpointSnapshot {
            checkpoint_id: "cp-1".to_string(),
            run_id: "run_1".to_string(),
            seq: 10,
            ts_ms: 1_000,
            reason: "task_start".to_string(),
            run: run.clone(),
            budget: budget.clone(),
            task_sessions: std::collections::HashMap::new(),
        };
        let cp2 = CheckpointSnapshot {
            checkpoint_id: "cp-2".to_string(),
            run_id: "run_1".to_string(),
            seq: 22,
            ts_ms: 2_000,
            reason: "task_end".to_string(),
            run,
            budget,
            task_sessions: std::collections::HashMap::new(),
        };

        store.save_checkpoint("run_1", &cp1).unwrap();
        store.save_checkpoint("run_1", &cp2).unwrap();

        let latest = store.load_latest_checkpoint("run_1").unwrap().unwrap();
        assert_eq!(latest.checkpoint_id, "cp-2");
        assert_eq!(latest.seq, 22);
        assert_eq!(latest.reason, "task_end");
    }
}
