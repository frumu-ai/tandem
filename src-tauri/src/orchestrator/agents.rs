// Orchestrator Sub-Agent Prompt Templates
// Defines prompts for Planner, Builder, Validator, and Researcher agents
// See: docs/orchestration_plan.md

use crate::orchestrator::types::{AgentRole, Task, ValidationResult};

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

Example:
```json
[
  {{
    "id": "task_1",
    "title": "Analyze existing code structure",
    "description": "Review the current implementation to understand the codebase",
    "dependencies": [],
    "acceptance_criteria": ["Identified key files", "Documented dependencies"]
  }},
  {{
    "id": "task_2",
    "title": "Implement feature X",
    "description": "Add the new feature based on analysis",
    "dependencies": ["task_1"],
    "acceptance_criteria": ["Feature works as specified", "No regressions"]
  }}
]
```

## Rules
1. Be CONCISE - no essays, just actionable tasks
2. Order tasks logically with proper dependencies
3. Each task should be achievable in one sub-agent call
4. Include clear acceptance criteria for validation
5. Maximum {max_tasks} tasks

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

## Rules
- Only modify files within the workspace
- Do not run dangerous commands (shell, install) unless absolutely necessary
- Be precise and minimal in your changes
- If you cannot complete the task, explain why

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
    pub fn parse_validation_result(output: &str) -> Option<ValidationResult> {
        // Try to extract JSON from the output
        let json_start = output.find('{')?;
        let json_end = output.rfind('}')?;
        let json_str = &output[json_start..=json_end];

        #[derive(serde::Deserialize)]
        struct RawResult {
            passed: bool,
            feedback: String,
            #[serde(default)]
            suggested_fixes: Vec<String>,
        }

        serde_json::from_str::<RawResult>(json_str)
            .ok()
            .map(|r| ValidationResult {
                passed: r.passed,
                feedback: r.feedback,
                suggested_fixes: r.suggested_fixes,
            })
    }

    /// Parse task list from planner output
    pub fn parse_task_list(output: &str) -> Option<Vec<ParsedTask>> {
        // Try to extract JSON array from the output
        let json_start = output.find('[')?;
        let json_end = output.rfind(']')?;
        let json_str = &output[json_start..=json_end];

        serde_json::from_str(json_str).ok()
    }
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
}

impl From<ParsedTask> for Task {
    fn from(parsed: ParsedTask) -> Self {
        Task {
            id: parsed.id,
            title: parsed.title,
            description: parsed.description,
            dependencies: parsed.dependencies,
            acceptance_criteria: parsed.acceptance_criteria,
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
}
