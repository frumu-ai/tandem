// Orchestrator Sub-Agent Prompt Templates
// Defines prompts for Planner, Builder, Validator, and Researcher agents
// See: docs/orchestration_plan.md

use crate::orchestrator::types::{normalize_role_key, Task, TaskGate, ValidationResult};
use std::collections::HashSet;

// ============================================================================
// Prompt Templates
// ============================================================================

/// Prompt builder for sub-agents
pub struct AgentPrompts;

impl AgentPrompts {
    /// Build prompt for Planner agent
    pub fn build_planner_prompt(
        objective: &str,
        workspace_summary: &str,
        constraints: &PlannerConstraints,
    ) -> String {
        format!(
            r#"You are a Planning Agent for a multi-agent orchestration system.

## Your Task
Create a task plan to accomplish the following objective:

{objective}

## Workspace Context
{workspace_summary}

## Constraints
- Maximum tasks: {max_tasks}
- Available tools: read_file, write_file, search, apply_patch
- Research enabled: {research_enabled}

## Output Format
You MUST output a valid JSON array of tasks. Each task must have:
- "id": unique identifier (e.g., "task_1", "task_2")
- "title": short descriptive title
- "description": detailed task description
- "dependencies": array of task IDs that must complete first (can be empty)
- "acceptance_criteria": array of specific criteria to verify completion
- "assigned_role": orchestrator/delegator/worker/watcher/reviewer/tester (default worker)
- Optional "template_id": role template hint
- Optional "gate": "review" or "test"

Example:
```json
[
  {{
    "id": "task_1",
    "title": "Analyze existing code structure",
    "description": "Review the current implementation to understand the codebase",
    "dependencies": [],
    "acceptance_criteria": ["Identified key files", "Documented dependencies"],
    "assigned_role": "orchestrator"
  }},
  {{
    "id": "task_2",
    "title": "Implement feature X",
    "description": "Add the new feature based on analysis",
    "dependencies": ["task_1"],
    "acceptance_criteria": ["Feature works as specified", "No regressions"],
    "assigned_role": "worker"
  }}
]
```

## Rules
1. Be CONCISE - no essays, just actionable tasks
2. Maximize safe parallelism:
   - Prefer independent tasks with empty dependencies when possible
   - Add dependencies ONLY when strictly required by task outputs
   - Avoid over-serializing work into unnecessary chains
3. Order tasks logically with proper dependencies
4. Each task should be achievable in one sub-agent call
5. Include clear acceptance criteria for validation
6. Maximum {max_tasks} tasks
7. If workspace context indicates sparse/empty files, create research-first and scaffold-first tasks
   (avoid repeated local shell discovery loops; use web research and produce concrete starter artifacts)
8. The first wave of work should include multiple runnable tasks whenever objective scope allows it.
   Only final synthesis/integration tasks should depend on many prior tasks.

Output ONLY the JSON array, no other text."#,
            objective = objective,
            workspace_summary = workspace_summary,
            max_tasks = constraints.max_tasks,
            research_enabled = constraints.research_enabled,
        )
    }

    /// Build prompt for Builder agent
    pub fn build_builder_prompt(
        task: &Task,
        file_context: &str,
        previous_output: Option<&str>,
    ) -> String {
        let previous_section = previous_output
            .map(|o| format!("\n## Previous Attempt Output\n{}\n", o))
            .unwrap_or_default();

        format!(
            r#"You are a Builder Agent for a multi-agent orchestration system.

## Your Task
{title}

{description}

## Acceptance Criteria
{criteria}

## Relevant Files
{file_context}
{previous_section}
## Output Requirements
1. Make the necessary code changes to complete this task
2. Write a brief note explaining what you did
3. Include verification hints for the validator
4. If the task/criteria mention creating or updating files, you MUST use `write`, `edit`, or `apply_patch`
   and ensure the target file exists in the workspace before finishing.

## Rules
- Only modify files within the workspace
- Do not run dangerous commands (shell, install) unless absolutely necessary
- Be precise and minimal in your changes
- If you cannot complete the task, explain why
- If workspace context is sparse/empty, avoid repeated shell discovery loops and pivot to research + initial scaffold artifacts
- Before claiming the workspace is empty or that templates/prior work are missing, you MUST gather concrete evidence with tools:
  - Run `glob` with pattern `**/*` (or `*` as fallback) and summarize results.
  - Cite at least a few observed paths (or explicitly report that no non-metadata files were found).
  - Do not rely on assumptions without tool-produced evidence.
- Tool arguments MUST be valid JSON objects. Never emit empty `{{}}` for file tools.
- For `read`/`write`, always include a non-empty string `path` field.
- For `write`, always include a non-empty string `content` field.
- If your tool call arguments are malformed, the task will fail immediately.

Complete this task now."#,
            title = task.title,
            description = task.description,
            criteria = task
                .acceptance_criteria
                .iter()
                .map(|c| format!("- {}", c))
                .collect::<Vec<_>>()
                .join("\n"),
            file_context = file_context,
            previous_section = previous_section,
        )
    }

    /// Build prompt for Validator agent
    pub fn build_validator_prompt(
        task: &Task,
        changes_diff: &str,
        build_output: Option<&str>,
    ) -> String {
        let build_section = build_output
            .map(|o| format!("\n## Build/Test Output\n```\n{}\n```\n", o))
            .unwrap_or_default();

        format!(
            r#"You are a Validator Agent for a multi-agent orchestration system.

## Task Being Validated
{title}

{description}

## Acceptance Criteria
{criteria}

## Changes Made
```diff
{diff}
```
{build_section}
## Your Job
Evaluate whether the changes satisfy ALL acceptance criteria.

## Output Format
You MUST output a JSON object with:
- "passed": true or false
- "feedback": explanation of your evaluation
- "suggested_fixes": array of specific fixes needed (empty if passed)

Example (passed):
```json
{{
  "passed": true,
  "feedback": "All acceptance criteria are met. The implementation is correct and complete.",
  "suggested_fixes": []
}}
```

Example (failed):
```json
{{
  "passed": false,
  "feedback": "The feature is partially implemented but missing error handling.",
  "suggested_fixes": ["Add try-catch around the API call", "Handle null response case"]
}}
```

Be strict but fair. Output ONLY the JSON object."#,
            title = task.title,
            description = task.description,
            criteria = task
                .acceptance_criteria
                .iter()
                .map(|c| format!("- {}", c))
                .collect::<Vec<_>>()
                .join("\n"),
            diff = changes_diff,
            build_section = build_section,
        )
    }

    /// Build prompt for Researcher agent
    pub fn build_researcher_prompt(question: &str, constraints: &ResearcherConstraints) -> String {
        format!(
            r#"You are a Researcher Agent for a multi-agent orchestration system.

## Research Question
{question}

## Constraints
- Maximum sources: {max_sources}
- Prohibited domains: {prohibited}

## Output Requirements
You must produce two outputs:

### 1. sources.json
A JSON array of sources consulted:
```json
[
  {{
    "url": "https://example.com/article",
    "title": "Article Title",
    "relevance": "Why this source is relevant"
  }}
]
```

### 2. fact_cards.md
A markdown document with key findings:
```markdown
## Key Finding 1
Summary of finding with citation [1]

## Key Finding 2
Summary of finding with citation [2]

---
## References
[1] Source title - URL
[2] Source title - URL
```

## Rules
1. Only use reputable sources
2. Always cite your sources
3. Deduplicate information
4. Stay within the source limit
5. Be factual and objective

Begin your research now."#,
            question = question,
            max_sources = constraints.max_sources,
            prohibited = constraints
                .prohibited_domains
                .iter()
                .map(|d| format!("- {}", d))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// Parse validation result from agent output
    pub fn parse_validation_result_strict(
        output: &str,
    ) -> std::result::Result<ValidationResult, String> {
        #[derive(serde::Deserialize)]
        struct RawResult {
            passed: bool,
            #[serde(default)]
            feedback: String,
            #[serde(default)]
            suggested_fixes: Vec<String>,
        }

        if let Ok(parsed) = serde_json::from_str::<RawResult>(output) {
            return Ok(ValidationResult {
                passed: parsed.passed,
                feedback: if parsed.feedback.trim().is_empty() {
                    if parsed.passed {
                        "Validation passed".to_string()
                    } else {
                        "Validation failed".to_string()
                    }
                } else {
                    parsed.feedback
                },
                suggested_fixes: parsed.suggested_fixes,
            });
        }

        for candidate in validation_json_candidates(output) {
            if let Ok(parsed) = serde_json::from_str::<RawResult>(&candidate) {
                return Ok(ValidationResult {
                    passed: parsed.passed,
                    feedback: if parsed.feedback.trim().is_empty() {
                        if parsed.passed {
                            "Validation passed".to_string()
                        } else {
                            "Validation failed".to_string()
                        }
                    } else {
                        parsed.feedback
                    },
                    suggested_fixes: parsed.suggested_fixes,
                });
            }
        }

        Err("validator response did not match required JSON schema".to_string())
    }

    pub fn parse_validation_result_fallback(output: &str) -> Option<ValidationResult> {
        let lower = output.to_lowercase();
        let inferred = if lower.contains("\"passed\": true") || lower.contains("passed: true") {
            Some(true)
        } else if lower.contains("\"passed\": false") || lower.contains("passed: false") {
            Some(false)
        } else if lower.contains("all acceptance criteria are met")
            || lower.contains("meets all acceptance criteria")
            || lower.contains("criteria are satisfied")
        {
            Some(true)
        } else if lower.contains("acceptance criteria not met")
            || lower.contains("does not meet")
            || lower.contains("missing")
            || lower.contains("not satisfied")
        {
            Some(false)
        } else if lower.contains("passed") && !lower.contains("not passed") {
            Some(true)
        } else if lower.contains("failed") && !lower.contains("not failed") {
            Some(false)
        } else {
            None
        };

        inferred.map(|passed| ValidationResult {
            passed,
            feedback: output
                .lines()
                .take(6)
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string(),
            suggested_fixes: Vec::new(),
        })
    }

    pub fn parse_validation_result(output: &str) -> Option<ValidationResult> {
        Self::parse_validation_result_strict(output)
            .ok()
            .or_else(|| Self::parse_validation_result_fallback(output))
    }

    /// Parse task list from planner output
    pub fn parse_task_list_strict(output: &str) -> std::result::Result<Vec<ParsedTask>, String> {
        let tasks = parse_tasks_from_json(output).ok_or_else(|| {
            "planner response did not contain valid JSON task payload".to_string()
        })?;
        let normalized = normalize_and_validate_parsed_tasks(tasks)?;
        if normalized.is_empty() {
            return Err("planner produced an empty task list".to_string());
        }
        Ok(normalized)
    }

    pub fn parse_task_list_fallback(output: &str) -> Option<Vec<ParsedTask>> {
        let markdown_tasks = parse_tasks_from_markdown(output);
        if markdown_tasks.is_empty() {
            return None;
        }
        normalize_and_validate_parsed_tasks(markdown_tasks).ok()
    }

    pub fn parse_task_list(output: &str) -> Option<Vec<ParsedTask>> {
        Self::parse_task_list_strict(output)
            .ok()
            .or_else(|| Self::parse_task_list_fallback(output))
    }
}

fn parse_tasks_from_json(output: &str) -> Option<Vec<ParsedTask>> {
    #[derive(serde::Deserialize)]
    struct WrappedTasks {
        tasks: Vec<ParsedTask>,
    }
    #[derive(serde::Deserialize)]
    struct WrappedPlan {
        plan: Vec<ParsedTask>,
    }
    #[derive(serde::Deserialize)]
    struct WrappedSteps {
        steps: Vec<ParsedTask>,
    }
    #[derive(serde::Deserialize)]
    struct WrappedItems {
        items: Vec<ParsedTask>,
    }
    #[derive(serde::Deserialize)]
    struct WrappedTaskList {
        task_list: Vec<ParsedTask>,
    }

    if let Ok(tasks) = serde_json::from_str::<Vec<ParsedTask>>(output) {
        return Some(tasks);
    }
    if let Ok(wrapped) = serde_json::from_str::<WrappedTasks>(output) {
        return Some(wrapped.tasks);
    }
    if let Ok(wrapped) = serde_json::from_str::<WrappedPlan>(output) {
        return Some(wrapped.plan);
    }
    if let Ok(wrapped) = serde_json::from_str::<WrappedSteps>(output) {
        return Some(wrapped.steps);
    }
    if let Ok(wrapped) = serde_json::from_str::<WrappedItems>(output) {
        return Some(wrapped.items);
    }
    if let Ok(wrapped) = serde_json::from_str::<WrappedTaskList>(output) {
        return Some(wrapped.task_list);
    }

    for candidate in json_candidates(output) {
        if let Ok(tasks) = serde_json::from_str::<Vec<ParsedTask>>(&candidate) {
            return Some(tasks);
        }
        if let Ok(wrapped) = serde_json::from_str::<WrappedTasks>(&candidate) {
            return Some(wrapped.tasks);
        }
        if let Ok(wrapped) = serde_json::from_str::<WrappedPlan>(&candidate) {
            return Some(wrapped.plan);
        }
        if let Ok(wrapped) = serde_json::from_str::<WrappedSteps>(&candidate) {
            return Some(wrapped.steps);
        }
        if let Ok(wrapped) = serde_json::from_str::<WrappedItems>(&candidate) {
            return Some(wrapped.items);
        }
        if let Ok(wrapped) = serde_json::from_str::<WrappedTaskList>(&candidate) {
            return Some(wrapped.task_list);
        }
    }

    None
}

fn json_candidates(output: &str) -> Vec<String> {
    let mut out = Vec::new();

    for marker in ["```json", "```JSON", "```"] {
        if let Some(start) = output.find(marker) {
            let after = &output[start + marker.len()..];
            if let Some(end) = after.find("```") {
                let block = after[..end].trim();
                if !block.is_empty() {
                    out.push(block.to_string());
                }
            }
        }
    }

    if let (Some(start), Some(end)) = (output.find('['), output.rfind(']')) {
        if start <= end {
            out.push(output[start..=end].to_string());
        }
    }

    if let (Some(start), Some(end)) = (output.find('{'), output.rfind('}')) {
        if start <= end {
            out.push(output[start..=end].to_string());
        }
    }

    out
}

fn validation_json_candidates(output: &str) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(start) = output.find("```json") {
        let after = &output[start + "```json".len()..];
        if let Some(end) = after.find("```") {
            out.push(after[..end].trim().to_string());
        }
    }

    if let (Some(start), Some(end)) = (output.find('{'), output.rfind('}')) {
        if start <= end {
            out.push(output[start..=end].to_string());
        }
    }

    out
}

fn parse_tasks_from_markdown(output: &str) -> Vec<ParsedTask> {
    let mut tasks = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let candidate = trimmed
            .trim_start_matches("- ")
            .trim_start_matches("* ")
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
            .trim();

        let (id, title) = if let Some((id_part, rest)) = candidate.split_once(':') {
            let parsed_id = id_part.trim().to_string();
            let parsed_title = rest.trim().to_string();
            if parsed_id.is_empty() || parsed_title.is_empty() {
                continue;
            }
            (parsed_id, parsed_title)
        } else {
            // Plain checklist fallback without explicit IDs.
            let parsed_title = candidate.trim().to_string();
            if parsed_title.len() < 4 {
                continue;
            }
            (String::new(), parsed_title)
        };

        tasks.push(ParsedTask {
            id,
            title: title.clone(),
            description: title,
            dependencies: Vec::new(),
            acceptance_criteria: vec!["Task completed successfully".to_string()],
            assigned_role: Some("worker".to_string()),
            template_id: None,
            gate: None,
        });
    }

    tasks
}

fn normalize_and_validate_parsed_tasks(
    tasks: Vec<ParsedTask>,
) -> std::result::Result<Vec<ParsedTask>, String> {
    if tasks.is_empty() {
        return Ok(Vec::new());
    }

    let mut seen_ids = HashSet::<String>::new();
    let mut normalized = Vec::new();

    for (idx, mut task) in tasks.into_iter().enumerate() {
        task.id = task.id.trim().to_string();
        task.title = task.title.trim().to_string();
        task.description = task.description.trim().to_string();
        task.dependencies = task
            .dependencies
            .into_iter()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .collect();
        task.acceptance_criteria = task
            .acceptance_criteria
            .into_iter()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        task.assigned_role = task
            .assigned_role
            .map(|r| normalize_role_key(&r))
            .or_else(|| Some("worker".to_string()));
        task.template_id = task
            .template_id
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        task.gate = task
            .gate
            .map(|v| v.trim().to_lowercase())
            .filter(|v| v == "review" || v == "test");

        if task.id.is_empty() {
            task.id = format!("task_{}", idx + 1);
        }
        if task.title.is_empty() {
            return Err(format!("task {} has empty title", idx + 1));
        }
        if task.description.is_empty() {
            return Err(format!("task {} has empty description", idx + 1));
        }
        if task.acceptance_criteria.is_empty() {
            task.acceptance_criteria = vec!["Task completed successfully".to_string()];
        }

        let base_id = task.id.clone();
        let mut unique_id = base_id.clone();
        let mut suffix = 2usize;
        while seen_ids.contains(&unique_id) {
            unique_id = format!("{}_{}", base_id, suffix);
            suffix += 1;
        }
        task.id = unique_id.clone();
        seen_ids.insert(unique_id);
        normalized.push(task);
    }

    Ok(normalized)
}

// ============================================================================
// Constraint Types
// ============================================================================

/// Constraints for the Planner agent
pub struct PlannerConstraints {
    pub max_tasks: usize,
    pub research_enabled: bool,
}

impl Default for PlannerConstraints {
    fn default() -> Self {
        Self {
            max_tasks: 12,
            research_enabled: false,
        }
    }
}

/// Constraints for the Researcher agent
pub struct ResearcherConstraints {
    pub max_sources: usize,
    pub prohibited_domains: Vec<String>,
}

impl Default for ResearcherConstraints {
    fn default() -> Self {
        Self {
            max_sources: 30,
            prohibited_domains: Vec::new(),
        }
    }
}

/// Parsed task from planner output
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ParsedTask {
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub assigned_role: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub gate: Option<String>,
}

impl From<ParsedTask> for Task {
    fn from(parsed: ParsedTask) -> Self {
        let assigned_role = normalize_role_key(parsed.assigned_role.as_deref().unwrap_or("worker"));
        let gate =
            parsed
                .gate
                .as_deref()
                .and_then(|value| match value.trim().to_lowercase().as_str() {
                    "review" => Some(TaskGate::Review),
                    "test" => Some(TaskGate::Test),
                    _ => None,
                });
        Task {
            id: parsed.id,
            title: parsed.title,
            description: parsed.description,
            dependencies: parsed.dependencies,
            acceptance_criteria: parsed.acceptance_criteria,
            assigned_role,
            template_id: parsed.template_id.filter(|v| !v.trim().is_empty()),
            gate,
            state: crate::orchestrator::types::TaskState::Pending,
            retry_count: 0,
            artifacts: Vec::new(),
            validation_result: None,
            error_message: None,
            session_id: None,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_validation_result_passed() {
        let output = r#"
Here is my evaluation:
{
  "passed": true,
  "feedback": "All criteria met",
  "suggested_fixes": []
}
"#;
        let result = AgentPrompts::parse_validation_result(output);
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.passed);
        assert_eq!(result.feedback, "All criteria met");
    }

    #[test]
    fn test_parse_validation_result_failed() {
        let output = r#"{"passed": false, "feedback": "Missing feature", "suggested_fixes": ["Add X", "Fix Y"]}"#;
        let result = AgentPrompts::parse_validation_result(output);
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(!result.passed);
        assert_eq!(result.suggested_fixes.len(), 2);
    }

    #[test]
    fn test_parse_task_list() {
        let output = r#"
Here is the plan:
[
  {"id": "1", "title": "Task 1", "description": "Do thing 1", "dependencies": [], "acceptance_criteria": ["Done"]},
  {"id": "2", "title": "Task 2", "description": "Do thing 2", "dependencies": ["1"], "acceptance_criteria": ["Done"]}
]
"#;
        let tasks = AgentPrompts::parse_task_list(output);
        assert!(tasks.is_some());
        let tasks = tasks.unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[1].dependencies, vec!["1"]);
    }

    #[test]
    fn test_parse_task_list_wrapped_json() {
        let output = r#"{
  "tasks": [
    {"id":"task_1","title":"Analyze","description":"Analyze code","dependencies":[],"acceptance_criteria":["done"]}
  ]
}"#;
        let tasks = AgentPrompts::parse_task_list(output).expect("tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "task_1");
    }

    #[test]
    fn test_parse_task_list_markdown_fallback() {
        let output = r#"
- task_1: Analyze existing codebase
- task_2: Implement feature
"#;
        let tasks = AgentPrompts::parse_task_list(output).expect("tasks");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "task_1");
    }

    #[test]
    fn test_parse_task_list_wrapped_plan_key() {
        let output = r#"{
  "plan": [
    {"id":"task_1","title":"Analyze","description":"Analyze code","dependencies":[],"acceptance_criteria":["done"]}
  ]
}"#;
        let tasks = AgentPrompts::parse_task_list(output).expect("tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "task_1");
    }

    #[test]
    fn test_parse_task_list_markdown_plain_checklist() {
        let output = r#"
- Analyze existing code structure
- Implement feature X
"#;
        let tasks = AgentPrompts::parse_task_list(output).expect("tasks");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "task_1");
        assert_eq!(tasks[0].title, "Analyze existing code structure");
    }

    #[test]
    fn test_parse_task_list_strict_rejects_markdown() {
        let output = r#"
- Analyze existing code structure
- Implement feature X
"#;
        let tasks = AgentPrompts::parse_task_list_strict(output);
        assert!(tasks.is_err());
    }

    #[test]
    fn test_parse_task_list_strict_dedupes_ids() {
        let output = r#"
[
  {"id":"task_1","title":"A","description":"Desc A","dependencies":[],"acceptance_criteria":["done"]},
  {"id":"task_1","title":"B","description":"Desc B","dependencies":[],"acceptance_criteria":["done"]}
]
"#;
        let tasks = AgentPrompts::parse_task_list_strict(output).expect("tasks");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "task_1");
        assert_eq!(tasks[1].id, "task_1_2");
    }

    #[test]
    fn test_parse_validation_result_strict_rejects_prose() {
        let output = "Looks good overall, passed.";
        let parsed = AgentPrompts::parse_validation_result_strict(output);
        assert!(parsed.is_err());
    }
}
