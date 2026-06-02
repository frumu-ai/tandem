use super::super::truncate_text;
use super::types::AutomationVerificationStep;
use super::*;
use serde_json::{json, Value};
use tandem_types::{MessagePart, Session};

pub(crate) fn automation_node_verification_state(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "verification_state")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_verification_command(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "verification_command")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn infer_verification_kind(command: &str) -> String {
    let lowered = command.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return "verify".to_string();
    }
    if lowered.starts_with("build:")
        || lowered.contains(" cargo build")
        || lowered.starts_with("cargo build")
        || lowered.contains(" npm run build")
        || lowered.starts_with("npm run build")
        || lowered.contains(" pnpm build")
        || lowered.starts_with("pnpm build")
        || lowered.contains(" yarn build")
        || lowered.starts_with("yarn build")
        || lowered.contains(" tsc")
        || lowered.starts_with("tsc")
        || lowered.starts_with("cargo check")
        || lowered.contains(" cargo check")
    {
        return "build".to_string();
    }
    if lowered.starts_with("test:")
        || lowered.contains(" cargo test")
        || lowered.starts_with("cargo test")
        || lowered.contains(" pytest")
        || lowered.starts_with("pytest")
        || lowered.contains(" npm test")
        || lowered.starts_with("npm test")
        || lowered.contains(" pnpm test")
        || lowered.starts_with("pnpm test")
        || lowered.contains(" yarn test")
        || lowered.starts_with("yarn test")
        || lowered.contains(" go test")
        || lowered.starts_with("go test")
    {
        return "test".to_string();
    }
    if lowered.starts_with("lint:")
        || lowered.contains(" clippy")
        || lowered.starts_with("cargo clippy")
        || lowered.contains(" eslint")
        || lowered.starts_with("eslint")
        || lowered.contains(" ruff")
        || lowered.starts_with("ruff")
        || lowered.contains(" shellcheck")
        || lowered.starts_with("shellcheck")
        || lowered.contains(" fmt --check")
        || lowered.contains(" format")
        || lowered.contains(" lint")
    {
        return "lint".to_string();
    }
    "verify".to_string()
}

pub(crate) fn split_verification_commands(raw: &str) -> Vec<String> {
    let mut commands = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        for chunk in trimmed.split("&&") {
            for piece in chunk.split(';') {
                let candidate = piece.trim();
                if candidate.is_empty() {
                    continue;
                }
                commands.push(candidate.to_string());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    commands
        .into_iter()
        .filter(|value| seen.insert(value.to_ascii_lowercase()))
        .collect()
}

fn normalized_command_tokens(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(|ch: char| matches!(ch, '\'' | '"' | '`'))
                .to_ascii_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn verification_command_matches(executed: &str, expected: &str) -> bool {
    let expected_tokens = normalized_command_tokens(expected);
    if expected_tokens.is_empty() {
        return false;
    }
    split_verification_commands(executed)
        .into_iter()
        .any(|segment| {
            let executed_tokens = normalized_command_tokens(&segment);
            executed_tokens.len() >= expected_tokens.len()
                && executed_tokens[..expected_tokens.len()] == expected_tokens[..]
        })
}

pub(crate) fn automation_node_verification_plan(
    node: &AutomationFlowNode,
) -> Vec<AutomationVerificationStep> {
    if let Some(items) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("verification_plan"))
        .and_then(Value::as_array)
    {
        let mut plan = Vec::new();
        for item in items {
            let (kind, command) = if let Some(obj) = item.as_object() {
                let command = obj
                    .get("command")
                    .or_else(|| obj.get("value"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let kind = obj
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_ascii_lowercase);
                (kind, command)
            } else {
                (
                    None,
                    item.as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                )
            };
            let Some(command) = command else {
                continue;
            };
            plan.push(AutomationVerificationStep {
                kind: kind.unwrap_or_else(|| infer_verification_kind(&command)),
                command,
            });
        }
        if !plan.is_empty() {
            return plan;
        }
    }
    automation_node_verification_command(node)
        .map(|raw| {
            split_verification_commands(&raw)
                .into_iter()
                .map(|command| AutomationVerificationStep {
                    kind: infer_verification_kind(&command),
                    command,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn session_verification_summary(node: &AutomationFlowNode, session: &Session) -> Value {
    let verification_plan = automation_node_verification_plan(node);
    let Some(expected_command) = automation_node_verification_command(node) else {
        return json!({
            "verification_expected": false,
            "verification_command": Value::Null,
            "verification_plan": [],
            "verification_results": [],
            "verification_outcome": Value::Null,
            "verification_total": 0,
            "verification_completed": 0,
            "verification_passed_count": 0,
            "verification_failed_count": 0,
            "verification_ran": false,
            "verification_failed": false,
            "latest_verification_command": Value::Null,
            "latest_verification_failure": Value::Null,
        });
    };
    let verification_plan = if verification_plan.is_empty() {
        vec![AutomationVerificationStep {
            kind: infer_verification_kind(&expected_command),
            command: expected_command.clone(),
        }]
    } else {
        verification_plan
    };
    let mut verification_results = verification_plan
        .iter()
        .map(|step| {
            json!({
                "kind": step.kind,
                "command": step.command,
                "ran": false,
                "failed": false,
                "failure": Value::Null,
                "latest_command": Value::Null,
            })
        })
        .collect::<Vec<_>>();
    let mut verification_ran = false;
    let mut verification_failed = false;
    let mut latest_verification_command = None::<String>;
    let mut latest_verification_failure = None::<String>;
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } = part
            else {
                continue;
            };
            if tool.trim().to_ascii_lowercase().replace('-', "_") != "bash" {
                continue;
            }
            let Some(command) = args.get("command").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let failure = if let Some(error) = error
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(error.to_string())
            } else {
                let metadata = result
                    .as_ref()
                    .and_then(|value| value.get("metadata"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let exit_code = metadata.get("exit_code").and_then(Value::as_i64);
                let timed_out = metadata
                    .get("timeout")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let cancelled = metadata
                    .get("cancelled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let stderr = metadata
                    .get("stderr")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                if timed_out {
                    Some(format!("verification command timed out: {}", command))
                } else if cancelled {
                    Some(format!("verification command was cancelled: {}", command))
                } else if exit_code.is_some_and(|code| code != 0) {
                    Some(
                        stderr
                            .filter(|value| !value.is_empty())
                            .map(|value| {
                                format!(
                                    "verification command failed with exit code {}: {}",
                                    exit_code.unwrap_or_default(),
                                    truncate_text(&value, 240)
                                )
                            })
                            .unwrap_or_else(|| {
                                format!(
                                    "verification command failed with exit code {}: {}",
                                    exit_code.unwrap_or_default(),
                                    command
                                )
                            }),
                    )
                } else {
                    None
                }
            };
            for result in &mut verification_results {
                let Some(expected) = result.get("command").and_then(Value::as_str) else {
                    continue;
                };
                if !verification_command_matches(command, expected) {
                    continue;
                }
                verification_ran = true;
                latest_verification_command = Some(command.to_string());
                if let Some(object) = result.as_object_mut() {
                    object.insert("ran".to_string(), json!(true));
                    object.insert("latest_command".to_string(), json!(command.to_string()));
                    if let Some(failure_text) = failure.clone() {
                        verification_failed = true;
                        latest_verification_failure = Some(failure_text.clone());
                        object.insert("failed".to_string(), json!(true));
                        object.insert("failure".to_string(), json!(failure_text));
                    }
                }
            }
        }
    }
    let verification_completed = verification_results
        .iter()
        .filter(|value| value.get("ran").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let verification_failed_count = verification_results
        .iter()
        .filter(|value| {
            value
                .get("failed")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let verification_passed_count = verification_results
        .iter()
        .filter(|value| {
            value.get("ran").and_then(Value::as_bool).unwrap_or(false)
                && !value
                    .get("failed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .count();
    let verification_total = verification_results.len();
    let verification_outcome = if verification_total == 0 {
        None
    } else if verification_failed_count > 0 {
        Some("failed")
    } else if verification_completed == 0 {
        Some("missing")
    } else if verification_completed < verification_total {
        Some("partial")
    } else {
        Some("passed")
    };
    json!({
        "verification_expected": true,
        "verification_command": expected_command,
        "verification_plan": verification_plan
            .iter()
            .map(|step| json!({"kind": step.kind, "command": step.command}))
            .collect::<Vec<_>>(),
        "verification_results": verification_results,
        "verification_outcome": verification_outcome,
        "verification_total": verification_total,
        "verification_completed": verification_completed,
        "verification_passed_count": verification_passed_count,
        "verification_failed_count": verification_failed_count,
        "verification_ran": verification_ran,
        "verification_failed": verification_failed,
        "latest_verification_command": latest_verification_command,
        "latest_verification_failure": latest_verification_failure,
    })
}
