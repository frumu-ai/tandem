use crate::error::{Result, TandemError};
use crate::sidecar::PermissionRule;
use globset::{Glob, GlobSetBuilder};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModeBase {
    Immediate,
    Plan,
    Orchestrate,
    Coder,
    Ask,
    Explore,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModeSource {
    Builtin,
    User,
    Project,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModeScope {
    User,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ModeDefinition {
    pub id: String,
    pub label: String,
    pub base_mode: ModeBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_append: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_globs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ModeSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedMode {
    pub id: String,
    pub label: String,
    pub base_mode: ModeBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_append: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_globs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<bool>,
    pub source: ModeSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModeResolution {
    pub mode: ResolvedMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ModesFile {
    Array(Vec<ModeDefinition>),
    Wrapped { modes: Vec<ModeDefinition> },
}

static MODE_ID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z][a-z0-9-]*$").expect("valid mode id regex"));

static KNOWN_TOOLS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    HashSet::from([
        "ls",
        "list",
        "read",
        "search",
        "glob",
        "write",
        "write_file",
        "create_file",
        "delete",
        "delete_file",
        "edit",
        "patch",
        "replace",
        "bash",
        "shell",
        "cmd",
        "terminal",
        "run_command",
        "websearch",
        "webfetch",
        "todowrite",
        "new_task",
        "update_todo_list",
        "switch_mode",
        "run_slash_command",
    ])
});

fn canonical_tool_name(raw: &str) -> String {
    let cleaned = raw.trim().to_lowercase().replace('-', "_");
    match cleaned.as_str() {
        "update_todos" => "update_todo_list".to_string(),
        "todo_write" => "todowrite".to_string(),
        _ => cleaned,
    }
}

fn is_edit_tool(tool: &str) -> bool {
    matches!(
        canonical_tool_name(tool).as_str(),
        "write" | "write_file" | "create_file" | "delete" | "delete_file" | "edit" | "patch"
    )
}

pub fn tool_path_from_args(args: &serde_json::Value) -> Option<String> {
    args.get("filePath")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("absolute_path").and_then(|v| v.as_str()))
        .or_else(|| args.get("path").and_then(|v| v.as_str()))
        .or_else(|| args.get("file").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

impl ResolvedMode {
    pub fn sidecar_agent(&self) -> Option<String> {
        match self.base_mode {
            ModeBase::Immediate => None,
            ModeBase::Ask => Some("general".to_string()),
            ModeBase::Plan => Some("plan".to_string()),
            // Engine-native agent names are build/plan/explore/general.
            ModeBase::Orchestrate => Some("plan".to_string()),
            ModeBase::Coder => Some("build".to_string()),
            ModeBase::Explore => Some("explore".to_string()),
        }
    }
}

pub fn built_in_modes() -> Vec<ResolvedMode> {
    vec![
        ResolvedMode {
            id: "immediate".to_string(),
            label: "Immediate".to_string(),
            base_mode: ModeBase::Immediate,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: ModeSource::Builtin,
        },
        ResolvedMode {
            id: "plan".to_string(),
            label: "Plan".to_string(),
            base_mode: ModeBase::Plan,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: ModeSource::Builtin,
        },
        ResolvedMode {
            id: "orchestrate".to_string(),
            label: "Orchestrate".to_string(),
            base_mode: ModeBase::Orchestrate,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: ModeSource::Builtin,
        },
        ResolvedMode {
            id: "coder".to_string(),
            label: "Coder".to_string(),
            base_mode: ModeBase::Coder,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: ModeSource::Builtin,
        },
        ResolvedMode {
            id: "ask".to_string(),
            label: "Ask".to_string(),
            base_mode: ModeBase::Ask,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: ModeSource::Builtin,
        },
        ResolvedMode {
            id: "explore".to_string(),
            label: "Explore".to_string(),
            base_mode: ModeBase::Explore,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: ModeSource::Builtin,
        },
    ]
}

pub fn mode_id_from_legacy_agent(agent: Option<&str>) -> &'static str {
    match agent.unwrap_or_default() {
        "plan" => "plan",
        "orchestrate" => "orchestrate",
        "coder" => "coder",
        "explore" => "explore",
        "general" => "ask",
        _ => "immediate",
    }
}

pub fn validate_mode_definition(mode: &ModeDefinition) -> Result<()> {
    if !MODE_ID_RE.is_match(mode.id.trim()) {
        return Err(TandemError::ValidationError(format!(
            "Invalid mode id '{}'. Use kebab-case like 'safe-coder'.",
            mode.id
        )));
    }
    if mode.label.trim().is_empty() {
        return Err(TandemError::ValidationError(
            "Mode label cannot be empty".to_string(),
        ));
    }
    if let Some(icon) = &mode.icon {
        if !MODE_ID_RE.is_match(icon.trim()) {
            return Err(TandemError::ValidationError(format!(
                "Invalid mode icon '{}'. Use kebab-case like 'sparkles'.",
                icon
            )));
        }
    }

    if let Some(tools) = &mode.allowed_tools {
        for tool in tools {
            let canonical = canonical_tool_name(tool);
            if !KNOWN_TOOLS.contains(canonical.as_str()) {
                return Err(TandemError::ValidationError(format!(
                    "Unknown tool '{}' in allowed_tools",
                    tool
                )));
            }
        }
    }

    if let Some(globs) = &mode.edit_globs {
        for pattern in globs {
            Glob::new(pattern).map_err(|e| {
                TandemError::ValidationError(format!(
                    "Invalid edit_glob '{}' for mode '{}': {}",
                    pattern, mode.id, e
                ))
            })?;
        }
    }

    Ok(())
}

fn normalize_definition(mut mode: ModeDefinition) -> ModeDefinition {
    mode.id = mode.id.trim().to_string();
    mode.label = mode.label.trim().to_string();
    mode.source = None;
    mode.icon = mode
        .icon
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    mode.system_prompt_append = mode
        .system_prompt_append
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    mode.allowed_tools = mode.allowed_tools.map(|tools| {
        let mut seen = HashSet::new();
        tools
            .into_iter()
            .map(|t| canonical_tool_name(&t))
            .filter(|t| seen.insert(t.clone()))
            .collect::<Vec<_>>()
    });
    mode.edit_globs = mode.edit_globs.map(|patterns| {
        let mut seen = HashSet::new();
        patterns
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .filter(|p| seen.insert(p.clone()))
            .collect::<Vec<_>>()
    });
    mode
}

fn to_resolved(mode: ModeDefinition, source: ModeSource) -> ResolvedMode {
    ResolvedMode {
        id: mode.id,
        label: mode.label,
        base_mode: mode.base_mode,
        icon: mode.icon,
        system_prompt_append: mode.system_prompt_append,
        allowed_tools: mode.allowed_tools,
        edit_globs: mode.edit_globs,
        auto_approve: mode.auto_approve,
        source,
    }
}

fn user_modes_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_config_dir().map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to resolve app config dir: {}", e))
    })?;
    fs::create_dir_all(&dir)?;
    Ok(dir.join("modes.json"))
}

fn project_modes_path(workspace: Option<&Path>) -> Option<PathBuf> {
    workspace.map(|ws| ws.join(".tandem").join("modes.json"))
}

fn read_mode_file(path: &Path) -> Result<Vec<ModeDefinition>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed: ModesFile = serde_json::from_str(&raw).map_err(|e| {
        TandemError::InvalidConfig(format!(
            "Failed to parse modes file '{}': {}",
            path.display(),
            e
        ))
    })?;
    Ok(match parsed {
        ModesFile::Array(modes) => modes,
        ModesFile::Wrapped { modes } => modes,
    })
}

fn write_mode_file(path: &Path, modes: &[ModeDefinition]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(modes)?;
    fs::write(path, json)?;
    Ok(())
}

fn validate_and_normalize_many(modes: Vec<ModeDefinition>) -> Vec<ModeDefinition> {
    let mut output = Vec::new();
    for mode in modes {
        let normalized = normalize_definition(mode);
        match validate_mode_definition(&normalized) {
            Ok(()) => output.push(normalized),
            Err(e) => {
                tracing::warn!("Dropping invalid mode '{}': {}", normalized.id, e);
            }
        }
    }
    output
}

fn merge_modes(
    builtins: Vec<ResolvedMode>,
    user_modes: Vec<ModeDefinition>,
    project_modes: Vec<ModeDefinition>,
) -> Vec<ResolvedMode> {
    let mut merged: HashMap<String, ResolvedMode> = HashMap::new();

    for mode in builtins {
        merged.insert(mode.id.clone(), mode);
    }
    for mode in user_modes {
        let resolved = to_resolved(mode, ModeSource::User);
        merged.insert(resolved.id.clone(), resolved);
    }
    for mode in project_modes {
        let resolved = to_resolved(mode, ModeSource::Project);
        merged.insert(resolved.id.clone(), resolved);
    }

    let mut values: Vec<ResolvedMode> = merged.into_values().collect();
    values.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    values
}

pub fn list_modes(app: &AppHandle, workspace: Option<&Path>) -> Result<Vec<ResolvedMode>> {
    let user_path = user_modes_path(app)?;
    let project_path = project_modes_path(workspace);

    let user_modes = validate_and_normalize_many(read_mode_file(&user_path)?);
    let project_modes = if let Some(path) = project_path.as_ref() {
        validate_and_normalize_many(read_mode_file(path)?)
    } else {
        Vec::new()
    };

    Ok(merge_modes(built_in_modes(), user_modes, project_modes))
}

pub fn resolve_mode_for_request(
    app: &AppHandle,
    workspace: Option<&Path>,
    mode_id: Option<&str>,
    legacy_agent: Option<&str>,
) -> Result<ModeResolution> {
    let resolved_modes = list_modes(app, workspace)?;
    let requested = mode_id
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| mode_id_from_legacy_agent(legacy_agent).to_string());

    if let Some(mode) = resolved_modes.into_iter().find(|m| m.id == requested) {
        return Ok(ModeResolution {
            mode,
            fallback_reason: None,
        });
    }

    let fallback = list_modes(app, workspace)?
        .into_iter()
        .find(|m| m.id == "ask")
        .or_else(|| built_in_modes().into_iter().find(|m| m.id == "ask"))
        .ok_or_else(|| {
            TandemError::InvalidConfig("Missing builtin fallback mode 'ask'".to_string())
        })?;

    Ok(ModeResolution {
        mode: fallback,
        fallback_reason: Some(format!(
            "Mode '{}' was not found. Falling back to 'ask'.",
            requested
        )),
    })
}

pub fn is_tool_allowed(mode: &ResolvedMode, tool: &str) -> bool {
    let Some(allowed) = &mode.allowed_tools else {
        return true;
    };
    let requested = canonical_tool_name(tool);
    allowed.iter().any(|t| t == &requested)
}

pub fn is_edit_path_allowed(mode: &ResolvedMode, workspace: &Path, path: &Path) -> bool {
    let Some(edit_globs) = &mode.edit_globs else {
        return true;
    };

    let relative = path.strip_prefix(workspace).unwrap_or(path);
    let rel = relative.to_string_lossy().replace('\\', "/");

    let mut builder = GlobSetBuilder::new();
    for pattern in edit_globs {
        let Ok(glob) = Glob::new(pattern) else {
            return false;
        };
        builder.add(glob);
    }
    let Ok(set) = builder.build() else {
        return false;
    };
    set.is_match(rel)
}

pub fn mode_allows_tool_execution(
    mode: &ResolvedMode,
    workspace: Option<&Path>,
    tool: &str,
    args: &serde_json::Value,
) -> Result<()> {
    if !is_tool_allowed(mode, tool) {
        return Err(TandemError::PermissionDenied(format!(
            "Tool '{}' is not allowed in mode '{}'",
            tool, mode.label
        )));
    }

    if is_edit_tool(tool) {
        if let Some(workspace_path) = workspace {
            if let Some(path_str) = tool_path_from_args(args) {
                let path = PathBuf::from(path_str);
                if !is_edit_path_allowed(mode, workspace_path, &path) {
                    return Err(TandemError::PermissionDenied(format!(
                        "Path is blocked by edit_globs for mode '{}'",
                        mode.label
                    )));
                }
            }
        }
    }

    Ok(())
}

fn mode_has_any_tool(mode: &ResolvedMode, tools: &[&str]) -> bool {
    let Some(allowed) = &mode.allowed_tools else {
        return true;
    };
    tools
        .iter()
        .map(|t| canonical_tool_name(t))
        .any(|t| allowed.contains(&t))
}

pub fn build_permission_rules(mode: &ResolvedMode) -> Vec<PermissionRule> {
    let mut rules = Vec::new();
    if mode_has_any_tool(mode, &["ls", "list", "glob", "search"]) {
        rules.push(PermissionRule {
            permission: "ls".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }
    if mode_has_any_tool(mode, &["read"]) {
        rules.push(PermissionRule {
            permission: "read".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }
    if mode_has_any_tool(mode, &["todowrite", "new_task", "update_todo_list"]) {
        rules.push(PermissionRule {
            permission: "todowrite".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
        rules.push(PermissionRule {
            permission: "todo_write".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }
    if mode_has_any_tool(mode, &["websearch"]) {
        rules.push(PermissionRule {
            permission: "websearch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }
    if mode_has_any_tool(mode, &["webfetch", "webfetch_document"]) {
        rules.push(PermissionRule {
            permission: "webfetch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }
    if mode_has_any_tool(mode, &["webfetch_document"]) {
        rules.push(PermissionRule {
            permission: "webfetch_document".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }
    rules
}

fn scope_path(app: &AppHandle, workspace: Option<&Path>, scope: ModeScope) -> Result<PathBuf> {
    match scope {
        ModeScope::User => user_modes_path(app),
        ModeScope::Project => project_modes_path(workspace).ok_or_else(|| {
            TandemError::InvalidConfig(
                "Cannot manage project modes without an active workspace".to_string(),
            )
        }),
    }
}

pub fn upsert_mode(
    app: &AppHandle,
    workspace: Option<&Path>,
    scope: ModeScope,
    mode: ModeDefinition,
) -> Result<()> {
    let path = scope_path(app, workspace, scope)?;
    let normalized = normalize_definition(mode);
    validate_mode_definition(&normalized)?;

    let mut modes = read_mode_file(&path)?;
    modes.retain(|m| m.id != normalized.id);
    modes.push(normalized);
    write_mode_file(&path, &modes)
}

pub fn delete_mode(
    app: &AppHandle,
    workspace: Option<&Path>,
    scope: ModeScope,
    id: &str,
) -> Result<()> {
    let path = scope_path(app, workspace, scope)?;
    let mut modes = read_mode_file(&path)?;
    modes.retain(|m| m.id != id);
    write_mode_file(&path, &modes)
}

pub fn export_modes(app: &AppHandle, workspace: Option<&Path>, scope: ModeScope) -> Result<String> {
    let path = scope_path(app, workspace, scope)?;
    let modes = validate_and_normalize_many(read_mode_file(&path)?);
    serde_json::to_string_pretty(&modes).map_err(TandemError::from)
}

pub fn import_modes(
    app: &AppHandle,
    workspace: Option<&Path>,
    scope: ModeScope,
    json: &str,
) -> Result<()> {
    let path = scope_path(app, workspace, scope)?;
    let parsed: ModesFile = serde_json::from_str(json)
        .map_err(|e| TandemError::InvalidConfig(format!("Invalid import JSON for modes: {}", e)))?;
    let modes = match parsed {
        ModesFile::Array(modes) => modes,
        ModesFile::Wrapped { modes } => modes,
    };

    let mut normalized = Vec::new();
    let mut seen_ids = HashSet::new();
    for mode in modes {
        let mode = normalize_definition(mode);
        validate_mode_definition(&mode)?;
        if !seen_ids.insert(mode.id.clone()) {
            return Err(TandemError::ValidationError(format!(
                "Duplicate mode id '{}' in import payload",
                mode.id
            )));
        }
        normalized.push(mode);
    }

    write_mode_file(&path, &normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_mode_id() {
        let mode = ModeDefinition {
            id: "Bad Id".to_string(),
            label: "x".to_string(),
            base_mode: ModeBase::Ask,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: None,
        };
        assert!(validate_mode_definition(&mode).is_err());
    }

    #[test]
    fn merge_precedence_prefers_project_then_user_then_builtin() {
        let builtins = built_in_modes();
        let user = vec![ModeDefinition {
            id: "ask".to_string(),
            label: "Ask User".to_string(),
            base_mode: ModeBase::Ask,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: None,
        }];
        let project = vec![ModeDefinition {
            id: "ask".to_string(),
            label: "Ask Project".to_string(),
            base_mode: ModeBase::Ask,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: None,
        }];

        let merged = merge_modes(builtins, user, project);
        let ask = merged
            .into_iter()
            .find(|m| m.id == "ask")
            .expect("ask should exist");
        assert_eq!(ask.label, "Ask Project");
        assert_eq!(ask.source, ModeSource::Project);
    }

    #[test]
    fn tool_allowlist_enforced() {
        let mode = ResolvedMode {
            id: "safe".to_string(),
            label: "Safe".to_string(),
            base_mode: ModeBase::Ask,
            icon: None,
            system_prompt_append: None,
            allowed_tools: Some(vec!["read".to_string()]),
            edit_globs: None,
            auto_approve: None,
            source: ModeSource::User,
        };
        assert!(is_tool_allowed(&mode, "read"));
        assert!(!is_tool_allowed(&mode, "write"));
    }
}
