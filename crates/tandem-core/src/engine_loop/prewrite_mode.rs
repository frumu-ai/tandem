use serde_json::{json, Value};
use tandem_types::{PrewriteCoverageMode, PrewriteRequirements, ToolMode};

use super::prewrite_gate::describe_unmet_prewrite_requirements_for_prompt;
use super::{
    infer_required_output_target_path_from_text, is_terminal_tool_error_reason,
    is_workspace_write_tool, normalize_tool_name,
};

pub(super) const REQUIRED_TOOL_MODE_UNSATISFIED_REASON: &str = "TOOL_MODE_REQUIRED_NOT_SATISFIED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequiredToolFailureKind {
    NoToolCallEmitted,
    ToolCallParseFailed,
    ToolCallInvalidArgs,
    WriteArgsEmptyFromProvider,
    WriteArgsUnparseableFromProvider,
    ToolCallRejectedByPolicy,
    ToolCallExecutedNonProductive,
    WriteRequiredNotSatisfied,
    #[allow(dead_code)]
    PrewriteRequirementsExhausted,
}

impl RequiredToolFailureKind {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::NoToolCallEmitted => "NO_TOOL_CALL_EMITTED",
            Self::ToolCallParseFailed => "TOOL_CALL_PARSE_FAILED",
            Self::ToolCallInvalidArgs => "TOOL_CALL_INVALID_ARGS",
            Self::WriteArgsEmptyFromProvider => "WRITE_ARGS_EMPTY_FROM_PROVIDER",
            Self::WriteArgsUnparseableFromProvider => "WRITE_ARGS_UNPARSEABLE_FROM_PROVIDER",
            Self::ToolCallRejectedByPolicy => "TOOL_CALL_REJECTED_BY_POLICY",
            Self::ToolCallExecutedNonProductive => "TOOL_CALL_EXECUTED_NON_PRODUCTIVE",
            Self::WriteRequiredNotSatisfied => "WRITE_REQUIRED_NOT_SATISFIED",
            Self::PrewriteRequirementsExhausted => "PREWRITE_REQUIREMENTS_EXHAUSTED",
        }
    }
}

pub(super) fn required_tool_mode_unsatisfied_completion(reason: RequiredToolFailureKind) -> String {
    format!(
        "{REQUIRED_TOOL_MODE_UNSATISFIED_REASON}: {}: tool_mode=required but the model ended without executing a productive tool call.",
        reason.code()
    )
}

#[allow(dead_code)]
pub(super) fn prewrite_requirements_exhausted_completion(
    unmet_codes: &[&'static str],
    repair_attempt: usize,
    repair_attempts_remaining: usize,
) -> String {
    let unmet = if unmet_codes.is_empty() {
        "none".to_string()
    } else {
        unmet_codes.join(", ")
    };
    format!(
        "TOOL_MODE_REQUIRED_NOT_SATISFIED: PREWRITE_REQUIREMENTS_EXHAUSTED: unmet prewrite requirements: {unmet}\n\n{{\"status\":\"blocked\",\"reason\":\"prewrite requirements exhausted before final artifact validation\",\"failureCode\":\"PREWRITE_REQUIREMENTS_EXHAUSTED\",\"repairAttempt\":{},\"repairAttemptsRemaining\":{},\"repairExhausted\":true,\"unmetRequirements\":{:?}}}",
        repair_attempt,
        repair_attempts_remaining,
        unmet_codes,
    )
}

pub(super) fn prewrite_repair_event_payload(
    repair_attempt: usize,
    repair_attempts_remaining: usize,
    unmet_codes: &[&'static str],
    repair_exhausted: bool,
) -> Value {
    json!({
        "repairAttempt": repair_attempt,
        "repairAttemptsRemaining": repair_attempts_remaining,
        "unmetRequirements": unmet_codes,
        "repairActive": repair_attempt > 0 && !repair_exhausted,
        "repairExhausted": repair_exhausted,
    })
}

pub(super) fn build_required_tool_retry_context(
    offered_tool_preview: &str,
    previous_reason: RequiredToolFailureKind,
) -> String {
    let offered = offered_tool_preview.trim();
    let available_tools = if offered.is_empty() {
        "Use one of the tools offered in this turn before you produce final text.".to_string()
    } else {
        format!("Use one of these offered tools before you produce final text: {offered}.")
    };
    let execution_instruction = if previous_reason
        == RequiredToolFailureKind::WriteRequiredNotSatisfied
    {
        "Inspection is complete; now create or modify workspace files with write, edit, or apply_patch.".to_string()
    } else if is_write_invalid_args_failure_kind(previous_reason) {
        "Previous tool call arguments were invalid. If you use write, include both `path` and the full `content`. If inspection is already complete, use write, edit, or apply_patch now.".to_string()
    } else {
        available_tools
    };
    format!(
        "Tool access is mandatory for this request. Previous attempt failed with {}. Execute at least one valid offered tool call before any final text. {}",
        previous_reason.code(),
        execution_instruction
    )
}

pub(super) fn looks_like_code_target_path(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.replace('\\', "/");
    let file_name = normalized
        .rsplit('/')
        .next()
        .unwrap_or(normalized.as_str())
        .to_ascii_lowercase();
    if matches!(
        file_name.as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "pnpm-lock.yaml"
            | "package-lock.json"
            | "yarn.lock"
            | "makefile"
            | "dockerfile"
            | ".gitignore"
            | ".editorconfig"
            | "tsconfig.json"
            | "pyproject.toml"
            | "requirements.txt"
    ) {
        return true;
    }
    let extension = file_name.rsplit('.').next().unwrap_or_default();
    matches!(
        extension,
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "kt"
            | "kts"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "swift"
            | "scala"
            | "sh"
            | "bash"
            | "zsh"
            | "toml"
            | "yaml"
            | "yml"
            | "json"
    )
}

pub(super) fn infer_code_workflow_from_text(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    if lowered.contains("code agent contract")
        || lowered.contains("inspect -> patch -> apply -> test -> repair")
        || lowered.contains("task kind: `code_change`")
        || lowered.contains("task kind: code_change")
        || lowered.contains("output contract kind: code_patch")
        || lowered.contains("verification expectation:")
        || lowered.contains("verification command:")
    {
        return true;
    }
    infer_required_output_target_path_from_text(text)
        .is_some_and(|path| looks_like_code_target_path(&path))
}

pub(super) fn infer_verification_command_from_text(text: &str) -> Option<String> {
    for marker in ["Verification expectation:", "verification expectation:"] {
        let Some(start) = text.find(marker) else {
            continue;
        };
        let remainder = text[start + marker.len()..].trim_start();
        let line = remainder.lines().next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let cleaned = line
            .trim_matches('`')
            .trim_end_matches('.')
            .trim()
            .to_string();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

pub(super) fn build_required_tool_retry_context_for_task(
    offered_tool_preview: &str,
    previous_reason: RequiredToolFailureKind,
    latest_user_text: &str,
) -> String {
    let mut prompt = build_required_tool_retry_context(offered_tool_preview, previous_reason);
    if !infer_code_workflow_from_text(latest_user_text) {
        return prompt;
    }
    let output_target = infer_required_output_target_path_from_text(latest_user_text)
        .unwrap_or_else(|| "the declared source target".to_string());
    let verification = infer_verification_command_from_text(latest_user_text)
        .unwrap_or_else(|| "run the declared verification command with `bash`".to_string());
    prompt.push(' ');
    prompt.push_str(
        "This is a code workflow: follow inspect -> patch -> apply -> test -> repair before finalizing.",
    );
    prompt.push(' ');
    prompt.push_str(&format!(
        "Patch `{output_target}` using `apply_patch` (or `edit` for local edits); use `write` only when creating a brand-new file."
    ));
    prompt.push(' ');
    prompt.push_str(&format!(
        "After patching, run verification with `bash` (`{verification}`). If verification fails, repair the smallest root cause and re-run verification."
    ));
    prompt
}

pub(super) fn is_write_invalid_args_failure_kind(reason: RequiredToolFailureKind) -> bool {
    matches!(
        reason,
        RequiredToolFailureKind::ToolCallInvalidArgs
            | RequiredToolFailureKind::WriteArgsEmptyFromProvider
            | RequiredToolFailureKind::WriteArgsUnparseableFromProvider
    )
}

pub(super) fn should_retry_nonproductive_required_tool_cycle(
    requested_write_required: bool,
    write_tool_attempted_in_cycle: bool,
    progress_made_in_cycle: bool,
    required_tool_retry_count: usize,
) -> bool {
    if write_tool_attempted_in_cycle {
        return required_tool_retry_count == 0 && !requested_write_required;
    }
    if progress_made_in_cycle {
        return required_tool_retry_count < 2;
    }
    required_tool_retry_count == 0 && (!requested_write_required || !write_tool_attempted_in_cycle)
}

pub(super) fn build_write_required_retry_context(
    offered_tool_preview: &str,
    previous_reason: RequiredToolFailureKind,
    latest_user_text: &str,
    prewrite_requirements: &PrewriteRequirements,
    workspace_inspection_satisfied: bool,
    concrete_read_satisfied: bool,
    web_research_satisfied: bool,
    successful_web_research_satisfied: bool,
) -> String {
    let mut prompt = build_required_tool_retry_context_for_task(
        offered_tool_preview,
        previous_reason,
        latest_user_text,
    );
    let unmet = describe_unmet_prewrite_requirements_for_prompt(
        prewrite_requirements,
        workspace_inspection_satisfied,
        concrete_read_satisfied,
        web_research_satisfied,
        successful_web_research_satisfied,
    );
    if !unmet.is_empty() {
        prompt.push(' ');
        prompt.push_str(&format!(
            "Before the final write, you still need to {}.",
            unmet.join(" and ")
        ));
    }
    if let Some(path) = infer_required_output_target_path_from_text(latest_user_text) {
        prompt.push(' ');
        prompt.push_str(&format!(
            "The required output target for this task is `{path}`. Write or update that file now."
        ));
        prompt.push(' ');
        prompt.push_str(
            "Your next response must be a `write` tool call for that file, not a prose-only reply.",
        );
        prompt.push(' ');
        prompt.push_str(
            "You have already gathered research in this session. Now write the output file using the information from your previous tool calls. You may re-read a specific file if needed for accuracy.",
        );
    }
    prompt
}

pub(super) fn build_prewrite_repair_retry_context(
    offered_tool_preview: &str,
    previous_reason: RequiredToolFailureKind,
    latest_user_text: &str,
    prewrite_requirements: &PrewriteRequirements,
    workspace_inspection_satisfied: bool,
    concrete_read_satisfied: bool,
    web_research_satisfied: bool,
    successful_web_research_satisfied: bool,
) -> String {
    let mut prompt = build_required_tool_retry_context_for_task(
        offered_tool_preview,
        previous_reason,
        latest_user_text,
    );
    let unmet = describe_unmet_prewrite_requirements_for_prompt(
        prewrite_requirements,
        workspace_inspection_satisfied,
        concrete_read_satisfied,
        web_research_satisfied,
        successful_web_research_satisfied,
    );
    if !unmet.is_empty() {
        prompt.push(' ');
        prompt.push_str(&format!(
            "Before the final write, you still need to {}.",
            unmet.join(" and ")
        ));
    }
    let mut repair_notes = Vec::new();
    if prewrite_requirements.concrete_read_required && !concrete_read_satisfied {
        repair_notes.push(
            "This task requires concrete `read` calls on relevant workspace files before you can write the output. Call `read` now on the files you discovered.",
        );
    }
    if prewrite_requirements.successful_web_research_required && !successful_web_research_satisfied
    {
        repair_notes.push(
            "Timed out or empty websearch attempts do not satisfy external-research requirements; call `websearch` with a concrete query now.",
        );
    }
    if !matches!(
        prewrite_requirements.coverage_mode,
        PrewriteCoverageMode::None
    ) {
        repair_notes.push(
            "Every path listed under `Files reviewed` must have been actually read in this run, and any relevant discovered file you did not read must appear under `Files not reviewed` with a reason.",
        );
    }
    if !repair_notes.is_empty() {
        prompt.push(' ');
        prompt.push_str("Do not skip this step. ");
        prompt.push_str(&repair_notes.join(" "));
    }
    if let Some(path) = infer_required_output_target_path_from_text(latest_user_text) {
        if infer_code_workflow_from_text(latest_user_text) {
            prompt.push(' ');
            prompt.push_str(&format!(
                "Use `read` to confirm the concrete code context, then patch `{path}` with `apply_patch` or `edit` and run verification before finalizing."
            ));
            prompt.push(' ');
            prompt.push_str(
                "Do not return a prose-only completion before patch + verification steps run.",
            );
        } else {
            prompt.push(' ');
            prompt.push_str(&format!(
                "Use `read` and `websearch` now to gather evidence, then write the artifact to `{path}`."
            ));
            prompt.push(' ');
            prompt.push_str(&format!(
                "Do not declare the output blocked while `read` and `websearch` remain available. Call them now."
            ));
        }
    }
    prompt
}

pub(super) fn build_prewrite_waived_write_context(
    latest_user_text: &str,
    unmet_codes: &[&'static str],
) -> String {
    let mut prompt = String::from(
        "Research prerequisites could not be fully satisfied after multiple repair attempts. \
         You must still write the output file using whatever information you have gathered so far. \
         Do not write a blocked or placeholder file. Write the best possible output with the evidence available.",
    );
    if !unmet_codes.is_empty() {
        prompt.push_str(&format!(
            " (Unmet prerequisites waived: {}.)",
            unmet_codes.join(", ")
        ));
    }
    if let Some(path) = infer_required_output_target_path_from_text(latest_user_text) {
        prompt.push_str(&format!(
            " The required output file is `{path}`. Call the `write` tool now to create it."
        ));
    }
    prompt
}

pub(super) fn build_empty_completion_retry_context(
    offered_tool_preview: &str,
    latest_user_text: &str,
    prewrite_requirements: &PrewriteRequirements,
    workspace_inspection_satisfied: bool,
    concrete_read_satisfied: bool,
    web_research_satisfied: bool,
    successful_web_research_satisfied: bool,
) -> String {
    let mut prompt = String::from(
        "You already used tools in this session, but returned no final output. Do not stop now.",
    );
    let unmet = describe_unmet_prewrite_requirements_for_prompt(
        prewrite_requirements,
        workspace_inspection_satisfied,
        concrete_read_satisfied,
        web_research_satisfied,
        successful_web_research_satisfied,
    );
    if !unmet.is_empty() {
        prompt.push(' ');
        prompt.push_str(&format!(
            "You still need to {} before the final write.",
            unmet.join(" and ")
        ));
        prompt.push(' ');
        prompt.push_str(&build_required_tool_retry_context_for_task(
            offered_tool_preview,
            RequiredToolFailureKind::WriteRequiredNotSatisfied,
            latest_user_text,
        ));
    }
    if let Some(path) = infer_required_output_target_path_from_text(latest_user_text) {
        prompt.push(' ');
        prompt.push_str(&format!("The required output target is `{path}`."));
        if unmet.is_empty() {
            prompt.push(' ');
            prompt.push_str(
                "Your next response must be a `write` tool call for that file, not a prose-only reply.",
            );
        } else {
            prompt.push(' ');
            prompt.push_str(
                "After completing the missing requirement, immediately write that file instead of ending with prose.",
            );
        }
    }
    prompt
}

pub(super) fn synthesize_artifact_write_completion_from_tool_state(
    latest_user_text: &str,
    prewrite_satisfied: bool,
    prewrite_gate_waived: bool,
) -> String {
    let target = infer_required_output_target_path_from_text(latest_user_text)
        .unwrap_or_else(|| "the declared output artifact".to_string());
    let mut completion = format!("Completed the requested tool actions and wrote `{target}`.");
    if prewrite_gate_waived && !prewrite_satisfied {
        completion.push_str(
            "\n\nRuntime validation will decide whether the artifact can be accepted because some evidence requirements were waived in-run.",
        );
    } else {
        completion
            .push_str("\n\nRuntime validation will verify the artifact and finalize node status.");
    }
    completion.push_str("\n\n{\"status\":\"completed\"}");
    completion
}

pub(super) fn should_generate_post_tool_final_narrative(
    requested_tool_mode: ToolMode,
    productive_tool_calls_total: usize,
) -> bool {
    !matches!(requested_tool_mode, ToolMode::Required) || productive_tool_calls_total > 0
}

pub(super) fn is_workspace_inspection_tool(tool_name: &str) -> bool {
    matches!(
        normalize_tool_name(tool_name).as_str(),
        "glob" | "read" | "grep" | "search" | "codesearch" | "ls" | "list"
    )
}

pub(super) fn is_web_research_tool(tool_name: &str) -> bool {
    matches!(
        normalize_tool_name(tool_name).as_str(),
        "websearch" | "webfetch" | "webfetch_html"
    )
}

pub(super) fn tool_matches_unmet_prewrite_repair_requirement(
    tool_name: &str,
    unmet_codes: &[&str],
) -> bool {
    if is_workspace_write_tool(tool_name) {
        return false;
    }
    let normalized = normalize_tool_name(tool_name);
    let needs_workspace_inspection = unmet_codes.contains(&"workspace_inspection_required");
    let needs_concrete_read =
        unmet_codes.contains(&"concrete_read_required") || unmet_codes.contains(&"coverage_mode");
    let needs_web_research = unmet_codes.iter().any(|code| {
        matches!(
            *code,
            "web_research_required" | "successful_web_research_required"
        )
    });
    (needs_concrete_read && (normalized == "read" || normalized == "glob"))
        || (needs_workspace_inspection && is_workspace_inspection_tool(&normalized))
        || (needs_web_research && is_web_research_tool(&normalized))
}

pub(super) fn invalid_tool_args_retry_max_attempts() -> usize {
    2
}

/// When `TANDEM_PREWRITE_GATE_STRICT=true`, the engine refuses to waive the prewrite
/// evidence gate even after exhausting repair retries. Instead of proceeding with
/// an unverified write, it holds the gate and emits `prewrite.gate.strict_mode.blocked`.
pub(super) fn prewrite_gate_strict_mode() -> bool {
    std::env::var("TANDEM_PREWRITE_GATE_STRICT")
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub fn prewrite_repair_retry_max_attempts() -> usize {
    5
}

pub(super) fn build_invalid_tool_args_retry_context_from_outputs(
    outputs: &[String],
    previous_attempts: usize,
) -> Option<String> {
    if outputs
        .iter()
        .any(|output| output.contains("BASH_COMMAND_MISSING"))
    {
        let emphasis = if previous_attempts > 0 {
            "You already tried `bash` without a valid command. Do not repeat an empty bash call."
        } else {
            "If you use `bash`, include a full non-empty command string."
        };
        return Some(format!(
            "Previous bash tool call was invalid because it did not include the required `command` field. {emphasis} Good examples: `pwd`, `ls -la`, `find docs -maxdepth 2 -type f`, or `rg -n \"workflow\" docs src`. Prefer `ls`, `glob`, `search`, and `read` for repository inspection when they are sufficient."
        ));
    }
    if outputs
        .iter()
        .any(|output| output.contains("WEBSEARCH_QUERY_MISSING"))
    {
        return Some("Previous websearch tool call was invalid because it did not include a query. If you use `websearch`, include a specific non-empty search query.".to_string());
    }
    if outputs
        .iter()
        .any(|output| output.contains("WEBFETCH_URL_MISSING"))
    {
        return Some(
            "Previous webfetch tool call was invalid because it did not include a URL. If you use `webfetch`, include a full absolute `url`.".to_string(),
        );
    }
    if outputs
        .iter()
        .any(|output| output.contains("FILE_PATH_MISSING"))
    {
        return Some(
            "Previous file tool call was invalid because it did not include a `path`. If you use `read`, `write`, or `edit`, include the exact workspace-relative file path.".to_string(),
        );
    }
    if outputs
        .iter()
        .any(|output| output.contains("WRITE_CONTENT_MISSING"))
    {
        return Some(
            "Previous write tool call was invalid because it did not include `content`. If you use `write`, include both `path` and the full `content`.".to_string(),
        );
    }
    None
}

pub(super) fn looks_like_unparsed_tool_payload(output: &str) -> bool {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("\"tool_calls\"")
        || lower.contains("\"function_call\"")
        || lower.contains("\"function\":{")
        || lower.contains("\"type\":\"tool_call\"")
        || lower.contains("\"type\":\"function_call\"")
        || lower.contains("\"type\":\"tool_use\"")
}

pub(super) fn is_policy_rejection_output(output: &str) -> bool {
    let lower = output.trim().to_ascii_lowercase();
    lower.contains("call skipped")
        || lower.contains("authorization required")
        || lower.contains("not allowed")
        || lower.contains("permission denied")
}

pub(super) fn classify_required_tool_failure(
    outputs: &[String],
    saw_tool_call_candidate: bool,
    accepted_tool_calls: usize,
    parse_failed: bool,
    rejected_by_policy: bool,
) -> RequiredToolFailureKind {
    if parse_failed {
        return RequiredToolFailureKind::ToolCallParseFailed;
    }
    if !saw_tool_call_candidate {
        return RequiredToolFailureKind::NoToolCallEmitted;
    }
    if accepted_tool_calls == 0 || rejected_by_policy {
        return RequiredToolFailureKind::ToolCallRejectedByPolicy;
    }
    if outputs
        .iter()
        .any(|output| output.contains("WRITE_ARGS_EMPTY_FROM_PROVIDER"))
    {
        return RequiredToolFailureKind::WriteArgsEmptyFromProvider;
    }
    if outputs
        .iter()
        .any(|output| output.contains("WRITE_ARGS_UNPARSEABLE_FROM_PROVIDER"))
    {
        return RequiredToolFailureKind::WriteArgsUnparseableFromProvider;
    }
    if outputs
        .iter()
        .any(|output| is_terminal_tool_error_reason(output))
    {
        return RequiredToolFailureKind::ToolCallInvalidArgs;
    }
    if outputs
        .iter()
        .any(|output| is_policy_rejection_output(output))
    {
        return RequiredToolFailureKind::ToolCallRejectedByPolicy;
    }
    RequiredToolFailureKind::ToolCallExecutedNonProductive
}
