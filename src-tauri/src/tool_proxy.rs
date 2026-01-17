// Tandem Tool Proxy
// Intercepts, validates, and journals all file/system operations
// This module will be used for tool approval UI in the future

use crate::error::{Result, TandemError};
use crate::state::AppState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Journal entry for tracking operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub status: OperationStatus,
    pub before_state: Option<FileSnapshot>,
    pub after_state: Option<FileSnapshot>,
    pub user_approved: bool,
}

/// Operation status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    PendingApproval,
    Approved,
    Denied,
    Completed,
    RolledBack,
    Failed,
}

/// Snapshot of a file's state for undo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub content: Option<String>,
    pub exists: bool,
    pub is_directory: bool,
}

/// Undo action that can restore previous state
#[derive(Debug, Clone)]
pub struct UndoAction {
    pub journal_entry_id: String,
    pub snapshot: FileSnapshot,
}

impl UndoAction {
    pub fn revert(&self) -> Result<()> {
        let path = Path::new(&self.snapshot.path);

        if self.snapshot.exists {
            if let Some(content) = &self.snapshot.content {
                fs::write(path, content).map_err(TandemError::Io)?;
                tracing::info!("Reverted file: {}", self.snapshot.path);
            }
        } else {
            // File didn't exist before, delete it
            if path.exists() {
                fs::remove_file(path).map_err(TandemError::Io)?;
                tracing::info!("Deleted file (undo create): {}", self.snapshot.path);
            }
        }

        Ok(())
    }
}

/// Operation journal for tracking and undoing AI actions
pub struct OperationJournal {
    entries: RwLock<VecDeque<JournalEntry>>,
    undo_stack: RwLock<Vec<UndoAction>>,
    max_entries: usize,
}

impl OperationJournal {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: RwLock::new(VecDeque::new()),
            undo_stack: RwLock::new(Vec::new()),
            max_entries,
        }
    }

    pub fn record(&self, entry: JournalEntry, undo_action: Option<UndoAction>) {
        let mut entries = self.entries.write().unwrap();

        // Remove oldest entries if we exceed max
        while entries.len() >= self.max_entries {
            entries.pop_front();
        }

        entries.push_back(entry);

        if let Some(action) = undo_action {
            let mut undo_stack = self.undo_stack.write().unwrap();
            undo_stack.push(action);
        }
    }

    pub fn undo_last(&self) -> Result<Option<String>> {
        let mut undo_stack = self.undo_stack.write().unwrap();

        if let Some(action) = undo_stack.pop() {
            let entry_id = action.journal_entry_id.clone();
            action.revert()?;
            Ok(Some(entry_id))
        } else {
            Ok(None)
        }
    }

    pub fn get_recent_entries(&self, count: usize) -> Vec<JournalEntry> {
        let entries = self.entries.read().unwrap();
        entries.iter().rev().take(count).cloned().collect()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.read().unwrap().is_empty()
    }
}

/// Tool proxy for validating and journaling operations
pub struct ToolProxy {
    app_state: Arc<AppState>,
    journal: Arc<OperationJournal>,
}

impl ToolProxy {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            journal: Arc::new(OperationJournal::new(100)),
        }
    }

    /// Validate that a path is within the allowed workspace
    pub fn validate_path(&self, path: &str) -> Result<PathBuf> {
        let path_buf = PathBuf::from(path);

        // Resolve to absolute path
        let absolute_path = if path_buf.is_absolute() {
            path_buf
        } else {
            let workspace = self.app_state.workspace_path.read().unwrap();
            if let Some(workspace_path) = workspace.as_ref() {
                workspace_path.join(&path_buf)
            } else {
                return Err(TandemError::PathNotAllowed(
                    "No workspace configured".to_string(),
                ));
            }
        };

        // Canonicalize to resolve any .. or symlinks
        let canonical = absolute_path
            .canonicalize()
            .unwrap_or(absolute_path.clone());

        // Check if path is allowed
        if !self.app_state.is_path_allowed(&canonical) {
            return Err(TandemError::PathNotAllowed(format!(
                "Access to path '{}' is not allowed.",
                path
            )));
        }

        Ok(canonical)
    }

    /// Get the operation journal
    pub fn journal(&self) -> &Arc<OperationJournal> {
        &self.journal
    }

    /// Check if undo is available
    pub fn can_undo(&self) -> bool {
        self.journal.can_undo()
    }

    /// Undo the last operation
    pub fn undo_last(&self) -> Result<Option<String>> {
        self.journal.undo_last()
    }

    /// Get recent operations
    pub fn get_recent_operations(&self, count: usize) -> Vec<JournalEntry> {
        self.journal.get_recent_entries(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_journal() {
        let journal = OperationJournal::new(10);

        let entry = JournalEntry {
            id: "test-1".to_string(),
            timestamp: Utc::now(),
            tool_name: "write_file".to_string(),
            args: serde_json::json!({"path": "test.txt"}),
            status: OperationStatus::Completed,
            before_state: None,
            after_state: None,
            user_approved: true,
        };

        journal.record(entry, None);

        let entries = journal.get_recent_entries(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "test-1");
    }
}
