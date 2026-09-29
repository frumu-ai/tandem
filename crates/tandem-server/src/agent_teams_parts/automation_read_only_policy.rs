// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn automation_tool_target_paths(tool: &str, args: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    match tool {
        "write" | "edit" => {
            if let Some(path) = args.get("path").and_then(Value::as_str) {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    paths.push(trimmed.to_string());
                }
            }
        }
        "apply_patch" => {
            let patch = args
                .get("patchText")
                .and_then(Value::as_str)
                .or_else(|| args.as_str());
            if let Some(patch) = patch {
                for line in patch.lines() {
                    for prefix in [
                        "*** Update File: ",
                        "*** Delete File: ",
                        "*** Add File: ",
                        "*** Move to: ",
                    ] {
                        if let Some(path) = line.strip_prefix(prefix) {
                            let trimmed = path.trim();
                            if !trimmed.is_empty() {
                                paths.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    paths.sort();
    paths.dedup();
    paths
}

fn automation_path_references_read_only_source_of_truth(
    path: &str,
    read_only_names: &std::collections::HashSet<String>,
    workspace_root: Option<&str>,
) -> bool {
    let trimmed = path.trim().trim_matches('`');
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if read_only_names.contains(&lowered) {
        return true;
    }
    if let Some(filename) = std::path::Path::new(trimmed)
        .file_name()
        .and_then(|value| value.to_str())
    {
        if read_only_names.contains(&filename.to_ascii_lowercase()) {
            return true;
        }
    }
    workspace_root
        .and_then(|root| {
            crate::app::state::automation::normalize_workspace_display_path(root, trimmed)
        })
        .is_some_and(|normalized| {
            let normalized_lower = normalized.to_ascii_lowercase();
            if read_only_names.contains(&normalized_lower) {
                return true;
            }
            std::path::Path::new(&normalized)
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|filename| read_only_names.contains(&filename.to_ascii_lowercase()))
        })
}

async fn evaluate_automation_read_only_write_deny(
    state: &AppState,
    session_id: &str,
    tool: &str,
    args: &Value,
) -> Option<String> {
    if !matches!(tool, "write" | "edit" | "apply_patch") {
        return None;
    }
    let run_id = state
        .automation_v2_session_runs
        .read()
        .await
        .get(session_id)
        .cloned()?;
    let run = state.get_automation_v2_run(&run_id).await?;
    let automation = run.automation_snapshot?;
    let read_only_names =
        crate::app::state::automation::enforcement::automation_read_only_source_of_truth_name_variants_for_automation(
            &automation,
        );
    if read_only_names.is_empty() {
        return None;
    }
    let workspace_root = args
        .get("__workspace_root")
        .and_then(Value::as_str)
        .or(automation.workspace_root.as_deref());
    let blocked_paths = automation_tool_target_paths(tool, args)
        .into_iter()
        .filter(|path| {
            automation_path_references_read_only_source_of_truth(
                path,
                &read_only_names,
                workspace_root,
            )
        })
        .collect::<Vec<_>>();
    if blocked_paths.is_empty() {
        None
    } else {
        Some(format!(
            "write denied for automation `{}` (run `{}`): read-only source-of-truth file(s) cannot be mutated: {}",
            automation.automation_id,
            run_id,
            blocked_paths.join(", ")
        ))
    }
}
