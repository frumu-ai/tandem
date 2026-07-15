// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::normalize_workspace_display_path;
use serde_json::{json, Value};
use std::path::PathBuf;

pub(crate) fn path_contains_wildcard_or_directory_placeholder(path: &str) -> bool {
    let trimmed = path.trim().trim_matches('`');
    trimmed.contains('*') || trimmed.contains('?') || trimmed.ends_with('/')
}

pub(crate) fn validate_path_array_hygiene(paths: &[String]) -> Option<String> {
    for path in paths {
        if path_contains_wildcard_or_directory_placeholder(path) {
            return Some(format!("path array contains non-concrete path: {}", path));
        }
    }
    None
}

pub(crate) fn path_looks_like_workspace_path(raw_path: &str) -> bool {
    let trimmed = raw_path.trim().trim_matches('`');
    !trimmed.is_empty()
        && !trimmed.starts_with("http://")
        && !trimmed.starts_with("https://")
        && (trimmed.contains('/') || trimmed.ends_with(".md") || trimmed.ends_with(".yaml"))
}

pub(crate) fn top_level_workspace_dir(path: &str) -> Option<String> {
    PathBuf::from(path)
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn workspace_relative_path_exists(workspace_root: &str, relative_path: &str) -> bool {
    let candidate = PathBuf::from(workspace_root).join(relative_path.trim_start_matches('/'));
    candidate.exists()
}

pub(crate) fn normalize_workspace_display_path_with_bases(
    workspace_root: &str,
    raw_path: &str,
    base_dirs: &[String],
    run_id: Option<&str>,
) -> Option<String> {
    let scoped_raw_path = run_id
        .and_then(|run_id| super::automation_run_scoped_output_path(run_id, raw_path))
        .unwrap_or_else(|| raw_path.to_string());
    if let Some(normalized) = normalize_workspace_display_path(workspace_root, &scoped_raw_path) {
        if workspace_relative_path_exists(workspace_root, &normalized) {
            return Some(normalized);
        }
    }
    if !path_looks_like_workspace_path(raw_path) {
        return None;
    }
    let trimmed = raw_path
        .trim()
        .trim_matches('`')
        .trim_start_matches("./")
        .trim_start_matches('/');
    let mut candidates = base_dirs
        .iter()
        .filter_map(|base_dir| {
            let candidate = format!("{}/{}", base_dir.trim_end_matches('/'), trimmed);
            normalize_workspace_display_path(workspace_root, &candidate)
                .filter(|normalized| workspace_relative_path_exists(workspace_root, normalized))
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return normalize_workspace_display_path(workspace_root, &scoped_raw_path);
    }
    candidates.sort();
    candidates.dedup();
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

pub(crate) fn normalize_workspace_path_annotation(
    workspace_root: &str,
    raw_path: &str,
    base_dirs: &[String],
    run_id: Option<&str>,
) -> Option<String> {
    if let Some((candidate, suffix)) = raw_path.split_once(" (") {
        return normalize_workspace_display_path_with_bases(
            workspace_root,
            candidate,
            base_dirs,
            run_id,
        )
        .map(|normalized| format!("{normalized} ({suffix}"));
    }
    if let Some((candidate, suffix)) = raw_path.split_once(": ") {
        return normalize_workspace_display_path_with_bases(
            workspace_root,
            candidate,
            base_dirs,
            run_id,
        )
        .map(|normalized| format!("{normalized}: {suffix}"));
    }
    normalize_workspace_display_path_with_bases(workspace_root, raw_path, base_dirs, run_id)
}

pub(crate) fn upstream_output_base_dirs(output: &Value, workspace_root: &str) -> Vec<String> {
    let mut bases = Vec::new();
    let path_arrays = [
        output
            .get("artifact_validation")
            .and_then(|value| value.get("read_paths")),
        output
            .get("artifact_validation")
            .and_then(|value| value.get("current_node_read_paths")),
        output
            .get("artifact_validation")
            .and_then(|value| value.get("discovered_relevant_paths")),
        output
            .get("artifact_validation")
            .and_then(|value| value.get("current_node_discovered_relevant_paths")),
    ];
    for rows in path_arrays.into_iter().flatten() {
        let Some(rows) = rows.as_array() else {
            continue;
        };
        for row in rows.iter().filter_map(Value::as_str) {
            let Some(normalized) = normalize_workspace_display_path(workspace_root, row) else {
                continue;
            };
            if let Some(parent) = PathBuf::from(&normalized)
                .parent()
                .and_then(|value| value.to_str())
            {
                let parent = parent.trim().trim_matches('/');
                if !parent.is_empty() {
                    bases.push(parent.to_string());
                }
            }
            if let Some(top_level) = top_level_workspace_dir(&normalized) {
                bases.push(top_level);
            }
        }
    }
    bases.sort();
    bases.dedup();
    bases
}

pub(crate) fn normalize_structured_handoff_field(
    workspace_root: &str,
    base_dirs: &[String],
    run_id: Option<&str>,
    key: &str,
    value: &mut Value,
) {
    let Some(rows) = value.as_array_mut() else {
        return;
    };
    for row in rows {
        match row {
            Value::String(raw) => {
                let normalized = match key {
                    "files_not_reviewed" | "skipped_paths_initial" => {
                        normalize_workspace_path_annotation(workspace_root, raw, base_dirs, run_id)
                    }
                    _ => normalize_workspace_display_path_with_bases(
                        workspace_root,
                        raw,
                        base_dirs,
                        run_id,
                    ),
                };
                if let Some(normalized) = normalized {
                    *raw = normalized;
                }
            }
            Value::Object(map) => {
                if let Some(Value::String(path)) = map.get_mut("path") {
                    if let Some(normalized) = normalize_workspace_display_path_with_bases(
                        workspace_root,
                        path,
                        base_dirs,
                        run_id,
                    ) {
                        *path = normalized;
                    }
                }
                if matches!(
                    key,
                    "citations_local" | "citations_external" | "sources_reviewed"
                ) {
                    if let Some(Value::String(source)) = map.get_mut("source") {
                        if let Some(normalized) = normalize_workspace_display_path_with_bases(
                            workspace_root,
                            source,
                            base_dirs,
                            run_id,
                        ) {
                            *source = normalized;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn normalize_upstream_research_output_paths(
    workspace_root: &str,
    run_id: Option<&str>,
    output: &Value,
) -> Value {
    let mut normalized = output.clone();
    let base_dirs = upstream_output_base_dirs(&normalized, workspace_root);
    let Some(content) = normalized.get_mut("content").and_then(Value::as_object_mut) else {
        return normalized;
    };
    if let Some(handoff) = content
        .get_mut("structured_handoff")
        .and_then(Value::as_object_mut)
    {
        for key in [
            "discovered_paths",
            "priority_paths",
            "skipped_paths_initial",
            "read_paths",
            "files_reviewed",
            "files_not_reviewed",
            "citations_local",
            "citations_external",
            "sources_reviewed",
            "source_material",
        ] {
            if let Some(value) = handoff.get_mut(key) {
                normalize_structured_handoff_field(workspace_root, &base_dirs, run_id, key, value);
            }
        }
    }
    if let Some(text) = content
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        if let Ok(mut parsed) = serde_json::from_str::<Value>(&text) {
            if let Some(map) = parsed.as_object_mut() {
                for key in [
                    "discovered_paths",
                    "priority_paths",
                    "skipped_paths_initial",
                    "read_paths",
                    "files_reviewed",
                    "files_not_reviewed",
                    "citations_local",
                    "citations_external",
                    "sources_reviewed",
                    "source_material",
                ] {
                    if let Some(value) = map.get_mut(key) {
                        normalize_structured_handoff_field(
                            workspace_root,
                            &base_dirs,
                            run_id,
                            key,
                            value,
                        );
                    }
                }
            }
            content.insert("text".to_string(), json!(parsed.to_string()));
        }
    }
    normalized
}
