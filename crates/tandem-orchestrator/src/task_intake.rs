// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSourceKind {
    HumanIntent,
    GitHubIssue,
    GitHubProjectItem,
    LocalBoardItem,
    ManualPrompt,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRouteKind {
    CoderRun,
    WorkflowPreview,
    MissionPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskIntakeRequest {
    pub task_id: String,
    pub title: String,
    pub source_kind: TaskSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub related_task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_route: Option<TaskRouteKind>,
}

impl TaskIntakeRequest {
    pub fn new(
        task_id: impl Into<String>,
        title: impl Into<String>,
        source_kind: TaskSourceKind,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            title: title.into(),
            source_kind,
            source_ref: None,
            description: None,
            repo_slug: None,
            workspace_root: None,
            acceptance_criteria: Vec::new(),
            labels: Vec::new(),
            related_task_ids: Vec::new(),
            grouping_key: None,
            preferred_route: None,
        }
    }

    pub fn single_task_coder_run(
        task_id: impl Into<String>,
        title: impl Into<String>,
        source_kind: TaskSourceKind,
    ) -> Self {
        Self::new(task_id, title, source_kind).with_preferred_route(TaskRouteKind::CoderRun)
    }

    pub fn workflow_preview(
        task_id: impl Into<String>,
        title: impl Into<String>,
        source_kind: TaskSourceKind,
    ) -> Self {
        Self::new(task_id, title, source_kind).with_preferred_route(TaskRouteKind::WorkflowPreview)
    }

    pub fn grouped_tasks_mission_preview(
        task_id: impl Into<String>,
        title: impl Into<String>,
        source_kind: TaskSourceKind,
        grouping_key: impl Into<String>,
    ) -> Self {
        Self::new(task_id, title, source_kind)
            .with_grouping_key(grouping_key)
            .with_preferred_route(TaskRouteKind::MissionPreview)
    }

    pub fn with_source_ref(mut self, source_ref: impl Into<String>) -> Self {
        self.source_ref = Some(source_ref.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_repo_binding(
        mut self,
        repo_slug: impl Into<String>,
        workspace_root: impl Into<String>,
    ) -> Self {
        self.repo_slug = Some(repo_slug.into());
        self.workspace_root = Some(workspace_root.into());
        self
    }

    pub fn with_acceptance_criteria(mut self, acceptance_criteria: Vec<String>) -> Self {
        self.acceptance_criteria = acceptance_criteria;
        self
    }

    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = labels;
        self
    }

    pub fn with_related_task_ids(mut self, related_task_ids: Vec<String>) -> Self {
        self.related_task_ids = related_task_ids;
        self
    }

    pub fn with_grouping_key(mut self, grouping_key: impl Into<String>) -> Self {
        self.grouping_key = Some(grouping_key.into());
        self
    }

    pub fn with_preferred_route(mut self, preferred_route: TaskRouteKind) -> Self {
        self.preferred_route = Some(preferred_route);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_task_coder_run_constructor_sets_route() {
        let request = TaskIntakeRequest::single_task_coder_run(
            "task-1",
            "Fix the parser",
            TaskSourceKind::GitHubProjectItem,
        )
        .with_source_ref("proj-123")
        .with_repo_binding("org/repo", "/workspace/repo")
        .with_acceptance_criteria(vec!["tests pass".to_string()])
        .with_labels(vec!["bug".to_string()]);

        assert_eq!(request.task_id, "task-1");
        assert_eq!(request.preferred_route, Some(TaskRouteKind::CoderRun));
        assert_eq!(request.source_ref.as_deref(), Some("proj-123"));
        assert_eq!(request.repo_slug.as_deref(), Some("org/repo"));
        assert_eq!(request.workspace_root.as_deref(), Some("/workspace/repo"));
        assert_eq!(request.acceptance_criteria, vec!["tests pass".to_string()]);
        assert_eq!(request.labels, vec!["bug".to_string()]);
    }

    #[test]
    fn grouped_tasks_mission_preview_constructor_sets_grouping_key() {
        let request = TaskIntakeRequest::grouped_tasks_mission_preview(
            "task-group-1",
            "Sprint slice",
            TaskSourceKind::HumanIntent,
            "release-2026-04",
        )
        .with_related_task_ids(vec!["task-a".to_string(), "task-b".to_string()]);

        assert_eq!(request.preferred_route, Some(TaskRouteKind::MissionPreview));
        assert_eq!(request.grouping_key.as_deref(), Some("release-2026-04"));
        assert_eq!(
            request.related_task_ids,
            vec!["task-a".to_string(), "task-b".to_string()]
        );
    }

    #[test]
    fn workflow_preview_constructor_sets_preferred_route() {
        let request = TaskIntakeRequest::workflow_preview(
            "task-2",
            "Preview the workflow",
            TaskSourceKind::ManualPrompt,
        );

        assert_eq!(
            request.preferred_route,
            Some(TaskRouteKind::WorkflowPreview)
        );
    }
}
