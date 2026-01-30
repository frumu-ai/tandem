// Ralph Loop Storage
// Handles persistence of state, history, and context to workspace-local files

use crate::error::{Result, TandemError};
use crate::ralph::types::{IterationRecord, RalphState};
use std::fs;
use std::path::{Path, PathBuf};

pub struct RalphStorage {
    base_path: PathBuf,
}

impl RalphStorage {
    pub fn new(workspace_path: &Path) -> Self {
        let base_path = workspace_path.join(".opencode/tandem/ralph");
        fs::create_dir_all(&base_path).ok();
        Self { base_path }
    }

    // =========================================================================
    // State File (state.json)
    // =========================================================================

    pub fn save_state(&self, state: &RalphState) -> Result<()> {
        let path = self.base_path.join("state.json");
        let json =
            serde_json::to_string_pretty(state).map_err(|e| TandemError::Serialization(e))?;
        fs::write(&path, json).map_err(|e| TandemError::Io(e))?;
        Ok(())
    }

    pub fn load_state(&self) -> Result<Option<RalphState>> {
        let path = self.base_path.join("state.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).map_err(|e| TandemError::Io(e))?;
        let state = serde_json::from_str(&content).map_err(|e| TandemError::Serialization(e))?;
        Ok(Some(state))
    }

    pub fn clear_state(&self) -> Result<()> {
        let path = self.base_path.join("state.json");
        if path.exists() {
            fs::remove_file(&path).map_err(|e| TandemError::Io(e))?;
        }
        Ok(())
    }

    // =========================================================================
    // History File (history.json)
    // =========================================================================

    pub fn append_history(&self, record: &IterationRecord) -> Result<()> {
        let path = self.base_path.join("history.json");

        // Load existing history
        let mut history = if path.exists() {
            let content = fs::read_to_string(&path).map_err(|e| TandemError::Io(e))?;
            serde_json::from_str(&content).map_err(|e| TandemError::Serialization(e))?
        } else {
            Vec::new()
        };

        // Append new record
        history.push(record.clone());

        // Cap to last 50 iterations
        const MAX_HISTORY: usize = 50;
        if history.len() > MAX_HISTORY {
            let skip_count = history.len() - MAX_HISTORY;
            history = history.into_iter().skip(skip_count).collect();
        }

        // Save back
        let json =
            serde_json::to_string_pretty(&history).map_err(|e| TandemError::Serialization(e))?;
        fs::write(&path, json).map_err(|e| TandemError::Io(e))?;

        Ok(())
    }

    pub fn load_history(&self, limit: usize) -> Result<Vec<IterationRecord>> {
        let path = self.base_path.join("history.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path).map_err(|e| TandemError::Io(e))?;
        let history: Vec<IterationRecord> =
            serde_json::from_str(&content).map_err(|e| TandemError::Serialization(e))?;

        // Return last N records
        let start = history.len().saturating_sub(limit);
        Ok(history.into_iter().skip(start).collect())
    }

    pub fn clear_history(&self) -> Result<()> {
        let path = self.base_path.join("history.json");
        if path.exists() {
            fs::remove_file(&path).map_err(|e| TandemError::Io(e))?;
        }
        Ok(())
    }

    // =========================================================================
    // Context File (context.md)
    // =========================================================================

    pub fn save_context(&self, context: &str) -> Result<()> {
        let path = self.base_path.join("context.md");
        fs::write(&path, context).map_err(|e| TandemError::Io(e))?;
        Ok(())
    }

    pub fn load_context(&self) -> Result<Option<String>> {
        let path = self.base_path.join("context.md");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).map_err(|e| TandemError::Io(e))?;
        Ok(Some(content))
    }

    pub fn clear_context(&self) -> Result<()> {
        let path = self.base_path.join("context.md");
        if path.exists() {
            fs::remove_file(&path).map_err(|e| TandemError::Io(e))?;
        }
        Ok(())
    }

    pub fn has_context(&self) -> bool {
        self.base_path.join("context.md").exists()
    }

    // =========================================================================
    // Summary File (summary.md) - Optional human-friendly summary
    // =========================================================================

    pub fn save_summary(&self, summary: &str) -> Result<()> {
        let path = self.base_path.join("summary.md");
        fs::write(&path, summary).map_err(|e| TandemError::Io(e))?;
        Ok(())
    }

    pub fn load_summary(&self) -> Result<Option<String>> {
        let path = self.base_path.join("summary.md");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).map_err(|e| TandemError::Io(e))?;
        Ok(Some(content))
    }

    // =========================================================================
    // Utility
    // =========================================================================

    pub fn get_storage_path(&self) -> &Path {
        &self.base_path
    }

    pub fn cleanup(&self) -> Result<()> {
        self.clear_state()?;
        self.clear_history()?;
        self.clear_context()?;
        let summary_path = self.base_path.join("summary.md");
        if summary_path.exists() {
            fs::remove_file(&summary_path).map_err(|e| TandemError::Io(e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ralph::types::{RalphConfig, RalphRunStatus};

    fn create_test_storage() -> (RalphStorage, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = RalphStorage::new(temp_dir.path());
        (storage, temp_dir)
    }

    #[test]
    fn test_save_and_load_state() {
        let (storage, _temp) = create_test_storage();

        let state = RalphState::new(
            "run-123".to_string(),
            "session-456".to_string(),
            "Test prompt".to_string(),
            RalphConfig::default(),
        );

        storage.save_state(&state).unwrap();
        let loaded = storage.load_state().unwrap().unwrap();

        assert_eq!(loaded.run_id, state.run_id);
        assert_eq!(loaded.session_id, state.session_id);
        assert_eq!(loaded.prompt, state.prompt);
    }

    #[test]
    fn test_append_and_load_history() {
        let (storage, _temp) = create_test_storage();

        let record1 = IterationRecord {
            iteration: 1,
            started_at: chrono::Utc::now(),
            ended_at: chrono::Utc::now(),
            duration_ms: 1000,
            completion_detected: false,
            tools_used: std::collections::HashMap::new(),
            files_modified: vec!["file1.txt".to_string()],
            errors: vec![],
            context_injected: None,
        };

        let record2 = IterationRecord {
            iteration: 2,
            started_at: chrono::Utc::now(),
            ended_at: chrono::Utc::now(),
            duration_ms: 2000,
            completion_detected: true,
            tools_used: std::collections::HashMap::new(),
            files_modified: vec!["file2.txt".to_string()],
            errors: vec![],
            context_injected: Some("Additional context".to_string()),
        };

        storage.append_history(&record1).unwrap();
        storage.append_history(&record2).unwrap();

        let history = storage.load_history(10).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].iteration, 1);
        assert_eq!(history[1].iteration, 2);
    }

    #[test]
    fn test_context_operations() {
        let (storage, _temp) = create_test_storage();

        assert!(!storage.has_context());
        assert!(storage.load_context().unwrap().is_none());

        storage.save_context("Test context").unwrap();
        assert!(storage.has_context());
        assert_eq!(storage.load_context().unwrap().unwrap(), "Test context");

        storage.clear_context().unwrap();
        assert!(!storage.has_context());
    }
}
