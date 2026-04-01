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

impl TaskSourceKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::HumanIntent => "human_intent",
            Self::GitHubIssue => "github_issue",
            Self::GitHubProjectItem => "github_project_item",
            Self::LocalBoardItem => "local_board_item",
            Self::ManualPrompt => "manual_prompt",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRouteKind {
    CoderRun,
    WorkflowPreview,
    MissionPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskGroupingSignalKind {
    SourceKind,
    SourceRef,
    RepoSlug,
    WorkspaceRoot,
    ProjectName,
    ProjectColumn,
    Label,
    RelatedTask,
    ExplicitGroupingKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskGroupingSignal {
    pub kind: TaskGroupingSignalKind,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBoardItem {
    pub board_item_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_column: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub related_task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping_key: Option<String>,
}

impl TaskBoardItem {
    pub fn new(board_item_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            board_item_id: board_item_id.into(),
            title: title.into(),
            source_ref: None,
            description: None,
            repo_slug: None,
            workspace_root: None,
            project_name: None,
            project_column: None,
            acceptance_criteria: Vec::new(),
            labels: Vec::new(),
            related_task_ids: Vec::new(),
            grouping_key: None,
        }
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

    pub fn with_project_context(
        mut self,
        project_name: impl Into<String>,
        project_column: impl Into<String>,
    ) -> Self {
        self.project_name = Some(project_name.into());
        self.project_column = Some(project_column.into());
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_column: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskIntakePreview {
    pub task_id: String,
    pub title: String,
    pub source_kind: TaskSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_column: Option<String>,
    pub related_task_count: usize,
    pub acceptance_criteria_count: usize,
    pub is_grouped: bool,
    pub has_repo_binding: bool,
    pub grouping_signal_count: usize,
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
            project_name: None,
            project_column: None,
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

    pub fn from_board_item(
        board_item: &TaskBoardItem,
        source_kind: TaskSourceKind,
        preferred_route: TaskRouteKind,
    ) -> Self {
        Self::new(
            board_item.board_item_id.clone(),
            board_item.title.clone(),
            source_kind,
        )
        .with_source_ref(
            board_item
                .source_ref
                .clone()
                .unwrap_or_else(|| board_item.board_item_id.clone()),
        )
        .with_description(
            board_item
                .description
                .clone()
                .unwrap_or_else(|| board_item.title.clone()),
        )
        .with_project_context_option(
            board_item.project_name.clone(),
            board_item.project_column.clone(),
        )
        .with_acceptance_criteria(board_item.acceptance_criteria.clone())
        .with_labels(board_item.labels.clone())
        .with_related_task_ids(board_item.related_task_ids.clone())
        .with_repo_binding_from_option(
            board_item.repo_slug.clone(),
            board_item.workspace_root.clone(),
        )
        .with_grouping_key_option(board_item.grouping_key.clone())
        .with_preferred_route(preferred_route)
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

    pub fn with_repo_binding_from_option(
        mut self,
        repo_slug: Option<String>,
        workspace_root: Option<String>,
    ) -> Self {
        self.repo_slug = repo_slug;
        self.workspace_root = workspace_root;
        self
    }

    pub fn with_project_context(
        mut self,
        project_name: impl Into<String>,
        project_column: impl Into<String>,
    ) -> Self {
        self.project_name = Some(project_name.into());
        self.project_column = Some(project_column.into());
        self
    }

    pub fn with_project_context_option(
        mut self,
        project_name: Option<String>,
        project_column: Option<String>,
    ) -> Self {
        self.project_name = project_name;
        self.project_column = project_column;
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

    pub fn with_grouping_key_option(mut self, grouping_key: Option<String>) -> Self {
        self.grouping_key = grouping_key;
        self
    }

    pub fn with_preferred_route(mut self, preferred_route: TaskRouteKind) -> Self {
        self.preferred_route = Some(preferred_route);
        self
    }

    pub fn preview(&self) -> TaskIntakePreview {
        TaskIntakePreview {
            task_id: self.task_id.clone(),
            title: self.title.clone(),
            source_kind: self.source_kind.clone(),
            source_ref: self.source_ref.clone(),
            repo_slug: self.repo_slug.clone(),
            workspace_root: self.workspace_root.clone(),
            project_name: self.project_name.clone(),
            project_column: self.project_column.clone(),
            related_task_count: self.related_task_ids.len(),
            acceptance_criteria_count: self.acceptance_criteria.len(),
            is_grouped: self.is_grouped(),
            has_repo_binding: self.has_repo_binding(),
            grouping_signal_count: self.grouping_signals().len(),
            preferred_route: self.preferred_route,
        }
    }

    pub fn has_repo_binding(&self) -> bool {
        self.repo_slug.is_some() && self.workspace_root.is_some()
    }

    pub fn is_grouped(&self) -> bool {
        self.grouping_key.is_some() || !self.related_task_ids.is_empty()
    }

    pub fn grouping_signals(&self) -> Vec<TaskGroupingSignal> {
        let mut signals = Vec::new();
        signals.push(TaskGroupingSignal {
            kind: TaskGroupingSignalKind::SourceKind,
            value: self.source_kind.as_str().to_string(),
        });
        if let Some(source_ref) = self.source_ref.as_ref() {
            signals.push(TaskGroupingSignal {
                kind: TaskGroupingSignalKind::SourceRef,
                value: source_ref.clone(),
            });
        }
        if let Some(repo_slug) = self.repo_slug.as_ref() {
            signals.push(TaskGroupingSignal {
                kind: TaskGroupingSignalKind::RepoSlug,
                value: repo_slug.clone(),
            });
        }
        if let Some(workspace_root) = self.workspace_root.as_ref() {
            signals.push(TaskGroupingSignal {
                kind: TaskGroupingSignalKind::WorkspaceRoot,
                value: workspace_root.clone(),
            });
        }
        if let Some(project_name) = self.project_name.as_ref() {
            signals.push(TaskGroupingSignal {
                kind: TaskGroupingSignalKind::ProjectName,
                value: project_name.clone(),
            });
        }
        if let Some(project_column) = self.project_column.as_ref() {
            signals.push(TaskGroupingSignal {
                kind: TaskGroupingSignalKind::ProjectColumn,
                value: project_column.clone(),
            });
        }
        for label in &self.labels {
            signals.push(TaskGroupingSignal {
                kind: TaskGroupingSignalKind::Label,
                value: label.clone(),
            });
        }
        for related_task_id in &self.related_task_ids {
            signals.push(TaskGroupingSignal {
                kind: TaskGroupingSignalKind::RelatedTask,
                value: related_task_id.clone(),
            });
        }
        if let Some(grouping_key) = self.grouping_key.as_ref() {
            signals.push(TaskGroupingSignal {
                kind: TaskGroupingSignalKind::ExplicitGroupingKey,
                value: grouping_key.clone(),
            });
        }
        signals
    }
}

pub fn recommend_task_route(
    preview: &TaskIntakePreview,
    grouping_signals: &[TaskGroupingSignal],
) -> TaskRouteKind {
    let has_grouping_context = preview.is_grouped
        || grouping_signals.iter().any(|signal| {
            matches!(
                signal.kind,
                TaskGroupingSignalKind::ProjectName
                    | TaskGroupingSignalKind::ProjectColumn
                    | TaskGroupingSignalKind::RelatedTask
                    | TaskGroupingSignalKind::ExplicitGroupingKey
            )
        });

    if has_grouping_context {
        TaskRouteKind::MissionPreview
    } else if preview.has_repo_binding {
        TaskRouteKind::CoderRun
    } else {
        TaskRouteKind::WorkflowPreview
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

    #[test]
    fn preview_reports_task_shape_without_policy_decision() {
        let request = TaskIntakeRequest::single_task_coder_run(
            "task-3",
            "Normalize the intake",
            TaskSourceKind::HumanIntent,
        )
        .with_repo_binding("org/repo", "/workspace/repo")
        .with_description("Ship the intake contract");

        let preview = request.preview();

        assert_eq!(preview.task_id, "task-3");
        assert_eq!(preview.preferred_route, Some(TaskRouteKind::CoderRun));
        assert!(preview.has_repo_binding);
        assert!(!preview.is_grouped);
        assert_eq!(preview.related_task_count, 0);
        assert_eq!(preview.acceptance_criteria_count, 0);
    }

    #[test]
    fn from_board_item_preserves_board_shape() {
        let board_item = TaskBoardItem::new("board-17", "Ship the adapter")
            .with_source_ref("gh-project-item-17")
            .with_description("Adapter example")
            .with_repo_binding("org/repo", "/workspace/repo")
            .with_project_context("Release 2026", "In Progress")
            .with_acceptance_criteria(vec!["docs updated".to_string()])
            .with_labels(vec!["board".to_string(), "adapter".to_string()])
            .with_related_task_ids(vec!["task-a".to_string()])
            .with_grouping_key("release-2026-04");

        let request = TaskIntakeRequest::from_board_item(
            &board_item,
            TaskSourceKind::GitHubProjectItem,
            TaskRouteKind::CoderRun,
        );

        assert_eq!(request.task_id, "board-17");
        assert_eq!(request.source_ref.as_deref(), Some("gh-project-item-17"));
        assert_eq!(request.repo_slug.as_deref(), Some("org/repo"));
        assert_eq!(request.workspace_root.as_deref(), Some("/workspace/repo"));
        assert_eq!(request.project_name.as_deref(), Some("Release 2026"));
        assert_eq!(request.project_column.as_deref(), Some("In Progress"));
        assert_eq!(
            request.acceptance_criteria,
            vec!["docs updated".to_string()]
        );
        assert_eq!(
            request.labels,
            vec!["board".to_string(), "adapter".to_string()]
        );
        assert_eq!(request.related_task_ids, vec!["task-a".to_string()]);
        assert_eq!(request.grouping_key.as_deref(), Some("release-2026-04"));
        assert_eq!(request.preferred_route, Some(TaskRouteKind::CoderRun));
    }

    #[test]
    fn preview_serializes_stable_shape() {
        let request = TaskIntakeRequest::grouped_tasks_mission_preview(
            "task-4",
            "Grouped mission preview",
            TaskSourceKind::GitHubIssue,
            "sprint-12",
        )
        .with_repo_binding("org/repo", "/workspace/repo")
        .with_related_task_ids(vec!["task-5".to_string()]);

        let preview = request.preview();
        let value = serde_json::to_value(&preview).expect("serialize preview");
        let object = value.as_object().expect("preview object");

        assert_eq!(
            object.get("task_id").and_then(|v| v.as_str()),
            Some("task-4")
        );
        assert_eq!(
            object.get("preferred_route").and_then(|v| v.as_str()),
            Some("mission_preview")
        );
        assert_eq!(
            object.get("has_repo_binding").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            object.get("is_grouped").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            object.get("related_task_count").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(
            object
                .get("acceptance_criteria_count")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            object.get("grouping_signal_count").and_then(|v| v.as_u64()),
            Some(5)
        );
    }

    #[test]
    fn grouping_signals_collect_richer_context() {
        let request = TaskIntakeRequest::new(
            "task-5",
            "Grouped signal test",
            TaskSourceKind::GitHubProjectItem,
        )
        .with_source_ref("proj-item-5")
        .with_repo_binding("org/repo", "/workspace/repo")
        .with_project_context("Release 2026", "In Review")
        .with_labels(vec!["sprint".to_string(), "backend".to_string()])
        .with_related_task_ids(vec!["task-a".to_string(), "task-b".to_string()])
        .with_grouping_key("release-2026-04");

        let signals = request.grouping_signals();

        assert!(signals
            .iter()
            .any(|signal| signal.kind == TaskGroupingSignalKind::SourceKind));
        assert!(signals
            .iter()
            .any(|signal| signal.kind == TaskGroupingSignalKind::SourceRef));
        assert!(signals
            .iter()
            .any(|signal| signal.kind == TaskGroupingSignalKind::RepoSlug));
        assert!(signals
            .iter()
            .any(|signal| signal.kind == TaskGroupingSignalKind::WorkspaceRoot));
        assert!(signals
            .iter()
            .any(|signal| signal.kind == TaskGroupingSignalKind::ProjectName));
        assert!(signals
            .iter()
            .any(|signal| signal.kind == TaskGroupingSignalKind::ProjectColumn));
        assert!(
            signals
                .iter()
                .filter(|signal| signal.kind == TaskGroupingSignalKind::Label)
                .count()
                == 2
        );
        assert!(
            signals
                .iter()
                .filter(|signal| signal.kind == TaskGroupingSignalKind::RelatedTask)
                .count()
                == 2
        );
        assert!(signals
            .iter()
            .any(|signal| signal.kind == TaskGroupingSignalKind::ExplicitGroupingKey));
    }

    #[test]
    fn grouping_signals_include_project_context() {
        let request = TaskIntakeRequest::new(
            "task-7",
            "Project context",
            TaskSourceKind::GitHubProjectItem,
        )
        .with_repo_binding("org/repo", "/workspace/repo")
        .with_project_context("Release 2026", "Review");

        let signals = request.grouping_signals();
        assert!(signals
            .iter()
            .any(|signal| signal.kind == TaskGroupingSignalKind::ProjectName
                && signal.value == "Release 2026"));
        assert!(signals.iter().any(|signal| {
            signal.kind == TaskGroupingSignalKind::ProjectColumn && signal.value == "Review"
        }));
    }

    #[test]
    fn grouped_task_route_hint_stays_advisory() {
        let request = TaskIntakeRequest::new(
            "task-6",
            "Grouped override test",
            TaskSourceKind::GitHubProjectItem,
        )
        .with_grouping_key("sprint-13")
        .with_related_task_ids(vec!["task-a".to_string()])
        .with_preferred_route(TaskRouteKind::CoderRun);

        let preview = request.preview();

        assert_eq!(preview.preferred_route, Some(TaskRouteKind::CoderRun));
        assert!(preview.is_grouped);
        assert_eq!(preview.grouping_signal_count, 3);
    }

    #[test]
    fn recommend_task_route_prefers_mission_over_advisory_coder_route() {
        let request = TaskIntakeRequest::new(
            "task-10",
            "Clustered project slice",
            TaskSourceKind::GitHubProjectItem,
        )
        .with_repo_binding("org/repo", "/workspace/repo")
        .with_project_context("Release 2026", "In Review")
        .with_grouping_key("release-2026-04")
        .with_related_task_ids(vec!["task-a".to_string()])
        .with_preferred_route(TaskRouteKind::CoderRun);

        let preview = request.preview();
        let route = recommend_task_route(&preview, &request.grouping_signals());

        assert_eq!(preview.preferred_route, Some(TaskRouteKind::CoderRun));
        assert!(preview.is_grouped);
        assert_eq!(route, TaskRouteKind::MissionPreview);
    }

    #[test]
    fn recommend_task_route_falls_back_to_workflow_for_unbound_task() {
        let request = TaskIntakeRequest::single_task_coder_run(
            "task-11",
            "Draft the plan",
            TaskSourceKind::HumanIntent,
        );

        let preview = request.preview();
        let route = recommend_task_route(&preview, &request.grouping_signals());

        assert!(!preview.has_repo_binding);
        assert_eq!(route, TaskRouteKind::WorkflowPreview);
    }

    #[test]
    fn recommend_task_route_prefers_mission_for_grouped_project_context() {
        let request = TaskIntakeRequest::new(
            "task-8",
            "Clustered project slice",
            TaskSourceKind::GitHubProjectItem,
        )
        .with_repo_binding("org/repo", "/workspace/repo")
        .with_project_context("Release 2026", "In Review")
        .with_related_task_ids(vec!["task-a".to_string()]);

        let preview = request.preview();
        let route = recommend_task_route(&preview, &request.grouping_signals());

        assert_eq!(route, TaskRouteKind::MissionPreview);
    }

    #[test]
    fn recommend_task_route_prefers_coder_for_single_repo_bound_task() {
        let request = TaskIntakeRequest::single_task_coder_run(
            "task-9",
            "Patch the bug",
            TaskSourceKind::GitHubProjectItem,
        )
        .with_repo_binding("org/repo", "/workspace/repo");

        let preview = request.preview();
        let route = recommend_task_route(&preview, &request.grouping_signals());

        assert_eq!(route, TaskRouteKind::CoderRun);
    }
}
