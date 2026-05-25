// Tandem Tool Proxy
// Intercepts, validates, and journals all file/system operations
// This module will be used for tool approval UI in the future

use crate::error::{Result, TandemError};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

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
    Staged,
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
    pub message_id: Option<String>,
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

    /// Undo all recorded file changes for a specific OpenCode message ID.
    /// Returns the list of file paths that were reverted.
    pub fn undo_for_message(&self, message_id: &str) -> Result<Vec<String>> {
        let mut reverted_paths: Vec<String> = Vec::new();
        let mut undo_stack = self.undo_stack.write().unwrap();

        tracing::info!(
            "[undo_for_message] Looking for message_id='{}' in {} undo actions",
            message_id,
            undo_stack.len()
        );

        // Log all undo actions for debugging
        for (i, action) in undo_stack.iter().enumerate() {
            tracing::info!(
                "[undo_for_message] Stack[{}]: message_id={:?}, path={}",
                i,
                action.message_id,
                action.snapshot.path
            );
        }

        // Walk from the end so we undo in reverse chronological order.
        let mut idx: isize = (undo_stack.len() as isize) - 1;
        while idx >= 0 {
            let i = idx as usize;
            let matches = undo_stack
                .get(i)
                .and_then(|a| a.message_id.as_deref())
                .map(|mid| mid == message_id)
                .unwrap_or(false);

            if matches {
                let action = undo_stack.remove(i);
                tracing::info!(
                    "[undo_for_message] Reverting file: {}",
                    action.snapshot.path
                );
                action.revert()?;
                reverted_paths.push(action.snapshot.path.clone());
            }

            idx -= 1;
        }

        Ok(reverted_paths)
    }

    pub fn get_recent_entries(&self, count: usize) -> Vec<JournalEntry> {
        let entries = self.entries.read().unwrap();
        entries.iter().rev().take(count).cloned().collect()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.read().unwrap().is_empty()
    }
}

/// Staged operation waiting for batch execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedOperation {
    pub id: String,
    pub request_id: String,
    pub session_id: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub before_snapshot: Option<FileSnapshot>,
    pub proposed_content: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub description: String,
    pub message_id: Option<String>,
}

/// Store for staged operations
pub struct StagingStore {
    operations: RwLock<Vec<StagedOperation>>,
}

impl StagingStore {
    pub fn new() -> Self {
        Self {
            operations: RwLock::new(Vec::new()),
        }
    }

    /// Stage a new operation
    pub fn stage(&self, operation: StagedOperation) {
        let mut ops = self.operations.write().unwrap();
        ops.push(operation);
        tracing::info!(
            "Staged operation: {} (total: {})",
            ops.last().unwrap().id,
            ops.len()
        );
    }

    /// Get all staged operations
    pub fn get_all(&self) -> Vec<StagedOperation> {
        let ops = self.operations.read().unwrap();
        ops.clone()
    }

    /// Remove a specific staged operation by ID
    pub fn remove(&self, id: &str) -> Option<StagedOperation> {
        let mut ops = self.operations.write().unwrap();
        ops.iter()
            .position(|op| op.id == id)
            .map(|pos| ops.remove(pos))
    }

    /// Clear all staged operations
    pub fn clear(&self) -> Vec<StagedOperation> {
        let mut ops = self.operations.write().unwrap();
        let cleared = ops.clone();
        ops.clear();
        tracing::info!("Cleared {} staged operations", cleared.len());
        cleared
    }

    /// Get count of staged operations
    pub fn count(&self) -> usize {
        let ops = self.operations.read().unwrap();
        ops.len()
    }

    /// Check if any operations are staged (reserved for future use)
    #[allow(dead_code)]
    pub fn has_staged(&self) -> bool {
        !self.operations.read().unwrap().is_empty()
    }
}

/// Tool proxy for validating and journaling operations (reserved for future use)
#[allow(dead_code)]
pub struct ToolProxy {
    _app_state: Arc<AppState>,
    journal: Arc<OperationJournal>,
}

impl ToolProxy {
    #[allow(dead_code)]
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self {
            _app_state: app_state,
            journal: Arc::new(OperationJournal::new(100)),
        }
    }

    /// Get the operation journal
    #[allow(dead_code)]
    pub fn journal(&self) -> &Arc<OperationJournal> {
        &self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

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
