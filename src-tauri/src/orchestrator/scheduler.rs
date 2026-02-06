// Orchestrator Task Scheduler
// DAG-based task scheduling with dependency resolution
// See: docs/orchestration_plan.md

use crate::orchestrator::types::{Task, TaskState};
use std::collections::{HashMap, HashSet};

// ============================================================================
// Task Scheduler
// ============================================================================

/// DAG-based task scheduler for orchestration
pub struct TaskScheduler;

impl TaskScheduler {
    /// Get the next runnable task (dependencies satisfied, not blocked)
    pub fn get_next_runnable(tasks: &[Task]) -> Option<&Task> {
        // Build a set of completed task IDs
        let completed: HashSet<&str> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Done)
            .map(|t| t.id.as_str())
            .collect();

        // Find first pending task with all deps satisfied
        tasks.iter().find(|task| {
            task.state == TaskState::Pending
                && task
                    .dependencies
                    .iter()
                    .all(|dep| completed.contains(dep.as_str()))
        })
    }

    /// Get all runnable tasks (for potential parallel execution)
    pub fn get_all_runnable(tasks: &[Task]) -> Vec<&Task> {
        let completed: HashSet<&str> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Done)
            .map(|t| t.id.as_str())
            .collect();

        tasks
            .iter()
            .filter(|task| {
                task.state == TaskState::Pending
                    && task
                        .dependencies
                        .iter()
                        .all(|dep| completed.contains(dep.as_str()))
            })
            .collect()
    }

    /// Check if all tasks are completed
    pub fn all_completed(tasks: &[Task]) -> bool {
        tasks.iter().all(|t| t.state == TaskState::Done)
    }

    /// Check if any task has failed
    pub fn any_failed(tasks: &[Task]) -> bool {
        tasks.iter().any(|t| t.state == TaskState::Failed)
    }

    /// Check if the task graph has any blocked tasks that can never complete
    pub fn has_deadlock(tasks: &[Task]) -> bool {
        let failed: HashSet<&str> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Failed)
            .map(|t| t.id.as_str())
            .collect();

        // A task is deadlocked if it depends on a failed task
        tasks.iter().any(|task| {
            task.state == TaskState::Pending
                && task
                    .dependencies
                    .iter()
                    .any(|dep| failed.contains(dep.as_str()))
        })
    }

    /// Detect cycles in task dependencies
    pub fn detect_cycle(tasks: &[Task]) -> Option<Vec<String>> {
        let task_map: HashMap<&str, &Task> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();

        for task in tasks {
            let mut visited = HashSet::new();
            let mut path = Vec::new();

            if Self::dfs_cycle(&task.id, &task_map, &mut visited, &mut path) {
                return Some(path);
            }
        }

        None
    }

    fn dfs_cycle(
        task_id: &str,
        task_map: &HashMap<&str, &Task>,
        visited: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> bool {
        if path.contains(&task_id.to_string()) {
            path.push(task_id.to_string());
            return true;
        }

        if visited.contains(task_id) {
            return false;
        }

        visited.insert(task_id.to_string());
        path.push(task_id.to_string());

        if let Some(task) = task_map.get(task_id) {
            for dep in &task.dependencies {
                if Self::dfs_cycle(dep, task_map, visited, path) {
                    return true;
                }
            }
        }

        path.pop();
        false
    }

    /// Validate task graph structure
    pub fn validate(tasks: &[Task]) -> Result<(), SchedulerError> {
        // Check for empty task list
        if tasks.is_empty() {
            return Err(SchedulerError::EmptyTaskList);
        }

        // Build task ID set
        let task_ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();

        // Check for duplicate IDs
        if task_ids.len() != tasks.len() {
            return Err(SchedulerError::DuplicateTaskId);
        }

        // Check for invalid dependencies
        for task in tasks {
            for dep in &task.dependencies {
                if !task_ids.contains(dep.as_str()) {
                    return Err(SchedulerError::InvalidDependency {
                        task_id: task.id.clone(),
                        dependency_id: dep.clone(),
                    });
                }
            }
        }

        // Check for cycles
        if let Some(cycle) = Self::detect_cycle(tasks) {
            return Err(SchedulerError::CycleDetected { path: cycle });
        }

        Ok(())
    }

    /// Update blocked tasks based on failed dependencies
    pub fn update_blocked_tasks(tasks: &mut [Task]) {
        let failed: HashSet<String> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Failed)
            .map(|t| t.id.clone())
            .collect();

        for task in tasks.iter_mut() {
            if task.state == TaskState::Pending {
                let has_failed_dep = task.dependencies.iter().any(|dep| failed.contains(dep));
                if has_failed_dep {
                    task.state = TaskState::Blocked;
                    task.error_message = Some("Blocked by failed dependency".to_string());
                }
            }
        }
    }

    /// Get task progress summary
    pub fn get_progress(tasks: &[Task]) -> TaskProgress {
        let mut progress = TaskProgress::default();

        for task in tasks {
            match task.state {
                TaskState::Pending => progress.pending += 1,
                TaskState::InProgress => progress.in_progress += 1,
                TaskState::Blocked => progress.blocked += 1,
                TaskState::Done => progress.done += 1,
                TaskState::Failed => progress.failed += 1,
            }
        }

        progress.total = tasks.len();
        progress
    }
}

/// Scheduler validation errors
#[derive(Debug, Clone)]
pub enum SchedulerError {
    EmptyTaskList,
    DuplicateTaskId,
    InvalidDependency {
        task_id: String,
        dependency_id: String,
    },
    CycleDetected {
        path: Vec<String>,
    },
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTaskList => write!(f, "Task list is empty"),
            Self::DuplicateTaskId => write!(f, "Duplicate task ID found"),
            Self::InvalidDependency {
                task_id,
                dependency_id,
            } => {
                write!(
                    f,
                    "Task '{}' has invalid dependency '{}'",
                    task_id, dependency_id
                )
            }
            Self::CycleDetected { path } => {
                write!(f, "Cycle detected in task graph: {}", path.join(" -> "))
            }
        }
    }
}

/// Task progress summary
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TaskProgress {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub blocked: usize,
    pub done: usize,
    pub failed: usize,
}

impl TaskProgress {
    pub fn completion_percentage(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.done as f64 / self.total as f64
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, deps: Vec<&str>, state: TaskState) -> Task {
        Task {
            id: id.to_string(),
            title: format!("Task {}", id),
            description: String::new(),
            dependencies: deps.into_iter().map(String::from).collect(),
            acceptance_criteria: Vec::new(),
            state,
            retry_count: 0,
            artifacts: Vec::new(),
            validation_result: None,
            error_message: None,
            session_id: None,
        }
    }

    #[test]
    fn test_get_next_runnable() {
        let tasks = vec![
            make_task("1", vec![], TaskState::Done),
            make_task("2", vec!["1"], TaskState::Pending),
            make_task("3", vec!["2"], TaskState::Pending),
        ];

        let next = TaskScheduler::get_next_runnable(&tasks);
        assert!(next.is_some());
        assert_eq!(next.unwrap().id, "2");
    }

    #[test]
    fn test_get_next_runnable_blocked() {
        let tasks = vec![
            make_task("1", vec![], TaskState::Pending),
            make_task("2", vec!["1"], TaskState::Pending),
        ];

        let next = TaskScheduler::get_next_runnable(&tasks);
        assert!(next.is_some());
        assert_eq!(next.unwrap().id, "1");
    }

    #[test]
    fn test_all_completed() {
        let tasks_incomplete = vec![
            make_task("1", vec![], TaskState::Done),
            make_task("2", vec![], TaskState::Pending),
        ];
        assert!(!TaskScheduler::all_completed(&tasks_incomplete));

        let tasks_complete = vec![
            make_task("1", vec![], TaskState::Done),
            make_task("2", vec![], TaskState::Done),
        ];
        assert!(TaskScheduler::all_completed(&tasks_complete));
    }

    #[test]
    fn test_detect_cycle() {
        // No cycle
        let tasks_no_cycle = vec![
            make_task("1", vec![], TaskState::Pending),
            make_task("2", vec!["1"], TaskState::Pending),
            make_task("3", vec!["2"], TaskState::Pending),
        ];
        assert!(TaskScheduler::detect_cycle(&tasks_no_cycle).is_none());

        // Cycle: 1 -> 2 -> 3 -> 1
        let tasks_cycle = vec![
            make_task("1", vec!["3"], TaskState::Pending),
            make_task("2", vec!["1"], TaskState::Pending),
            make_task("3", vec!["2"], TaskState::Pending),
        ];
        assert!(TaskScheduler::detect_cycle(&tasks_cycle).is_some());
    }

    #[test]
    fn test_validate() {
        // Valid
        let valid_tasks = vec![
            make_task("1", vec![], TaskState::Pending),
            make_task("2", vec!["1"], TaskState::Pending),
        ];
        assert!(TaskScheduler::validate(&valid_tasks).is_ok());

        // Empty
        let empty_tasks: Vec<Task> = vec![];
        assert!(matches!(
            TaskScheduler::validate(&empty_tasks),
            Err(SchedulerError::EmptyTaskList)
        ));

        // Invalid dependency
        let invalid_dep = vec![make_task("1", vec!["nonexistent"], TaskState::Pending)];
        assert!(matches!(
            TaskScheduler::validate(&invalid_dep),
            Err(SchedulerError::InvalidDependency { .. })
        ));
    }

    #[test]
    fn test_update_blocked_tasks() {
        let mut tasks = vec![
            make_task("1", vec![], TaskState::Failed),
            make_task("2", vec!["1"], TaskState::Pending),
            make_task("3", vec![], TaskState::Pending),
        ];

        TaskScheduler::update_blocked_tasks(&mut tasks);

        assert_eq!(tasks[1].state, TaskState::Blocked);
        assert_eq!(tasks[2].state, TaskState::Pending);
    }
}
