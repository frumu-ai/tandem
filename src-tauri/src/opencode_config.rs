//! OpenCode configuration helpers.
//!
//! Tandem uses OpenCode as a sidecar engine. To support MCP servers and plugins
//! without re-implementing runtime behavior, we manage OpenCode config files in
//! a round-trip-safe way (preserve unknown fields) with atomic-ish writes.

use crate::error::{Result, TandemError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OpenCodeConfigScope {
    Global,
    Project,
}

pub fn global_config_path() -> Result<PathBuf> {
    // Prefer an existing file to avoid splitting config across multiple locations.
    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1) Current Tandem path (legacy).
    if let Some(config_dir) = dirs::config_dir() {
        candidates.push(config_dir.join("opencode").join("config.json"));
    }

    // 2) OpenCode docs commonly refer to ~/.config/opencode/opencode.json(.c).
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config").join("opencode").join("opencode.json"));
        candidates.push(home.join(".config").join("opencode").join("opencode.jsonc"));
    }

    // 3) Additional reasonable fallback.
    if let Some(config_dir) = dirs::config_dir() {
        candidates.push(config_dir.join("opencode").join("opencode.json"));
    }

    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
        }
    }

    // No existing config: default to the legacy path so existing sidecar behavior
    // stays consistent, and because it's in the OS config directory.
    if let Some(config_dir) = dirs::config_dir() {
        return Ok(config_dir.join("opencode").join("config.json"));
    }

    // Last resort: ~/.config/opencode/opencode.json
    if let Some(home) = dirs::home_dir() {
        return Ok(home.join(".config").join("opencode").join("opencode.json"));
    }

    Err(TandemError::InvalidConfig(
        "Could not determine OpenCode global config path".to_string(),
    ))
}

pub fn project_config_path(workspace: &Path) -> PathBuf {
    // OpenCode supports `opencode.json`/`opencode.jsonc` in the project root.
    // We pick the first existing, otherwise default to opencode.json.
    let json = workspace.join("opencode.json");
    let jsonc = workspace.join("opencode.jsonc");
    if json.exists() {
        json
    } else if jsonc.exists() {
        jsonc
    } else {
        json
    }
}

pub fn get_config_path(scope: OpenCodeConfigScope, workspace: Option<&Path>) -> Result<PathBuf> {
    match scope {
        OpenCodeConfigScope::Global => global_config_path(),
        OpenCodeConfigScope::Project => {
            let ws = workspace.ok_or_else(|| {
                TandemError::InvalidConfig("No active workspace for project config".to_string())
            })?;
            Ok(project_config_path(ws))
        }
    }
}

pub fn read_config(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }

    let raw = fs::read_to_string(path).map_err(TandemError::Io)?;

    // Support basic JSONC by stripping comments when needed.
    // If stripping isn't necessary, serde_json will still parse fine.
    let stripped = strip_jsonc_comments(&raw);

    let mut v: Value = serde_json::from_str(&stripped).map_err(TandemError::Serialization)?;
    if !v.is_object() {
        // We only operate on object configs; normalize to object to avoid panics elsewhere.
        v = Value::Object(Map::new());
    }
    Ok(v)
}

pub fn write_config_atomic(path: &Path, value: &Value) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        TandemError::InvalidConfig(format!("Invalid OpenCode config path: {:?}", path))
    })?;
    fs::create_dir_all(parent).map_err(TandemError::Io)?;

    let json = serde_json::to_string_pretty(value).map_err(TandemError::Serialization)?;

    // Write to a temp file in the same directory then rename into place.
    // This is atomic on Unix; on Windows rename semantics may vary but is still
    // the best available without platform-specific APIs.
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.json");
    let tmp_path = parent.join(format!(".{}.tmp", file_name));

    {
        let mut f = fs::File::create(&tmp_path).map_err(TandemError::Io)?;
        f.write_all(json.as_bytes()).map_err(TandemError::Io)?;
        f.write_all(b"\n").map_err(TandemError::Io)?;
        f.sync_all().ok(); // best-effort
    }

    // Best effort rename into place (replace if necessary).
    match fs::rename(&tmp_path, path) {
        Ok(_) => Ok(()),
        Err(_) => {
            // Some environments fail if destination exists; try remove then rename.
            let _ = fs::remove_file(path);
            fs::rename(&tmp_path, path).map_err(TandemError::Io)?;
            Ok(())
        }
    }
}

pub fn update_config_at<F>(path: &Path, mutator: F) -> Result<Value>
where
    F: FnOnce(&mut Value) -> Result<()>,
{
    let mut cfg = read_config(path)?;
    mutator(&mut cfg)?;
    write_config_atomic(path, &cfg)?;
    Ok(cfg)
}

pub fn update_config<F>(
    scope: OpenCodeConfigScope,
    workspace: Option<&Path>,
    mutator: F,
) -> Result<Value>
where
    F: FnOnce(&mut Value) -> Result<()>,
{
    let path = get_config_path(scope, workspace)?;
    update_config_at(&path, mutator)
}

pub fn ensure_schema(cfg: &mut Value) {
    let obj = match cfg.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    obj.entry("$schema".to_string())
        .or_insert_with(|| Value::String("https://opencode.ai/config.json".to_string()));
}

pub fn set_provider_ollama_models(cfg: &mut Value, models: Value) {
    ensure_schema(cfg);

    let root = cfg.as_object_mut().unwrap();
    let provider = root
        .entry("provider".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    let provider_obj = provider.as_object_mut().unwrap();
    let ollama = provider_obj
        .entry("ollama".to_string())
        .or_insert_with(|| Value::Object(default_ollama_provider()));

    if let Some(ollama_obj) = ollama.as_object_mut() {
        // Only fill defaults if missing; preserve user overrides.
        ollama_obj
            .entry("npm".to_string())
            .or_insert_with(|| Value::String("@ai-sdk/openai-compatible".to_string()));
        ollama_obj
            .entry("name".to_string())
            .or_insert_with(|| Value::String("Ollama (Local)".to_string()));
        ollama_obj.entry("options".to_string()).or_insert_with(|| {
            let mut opt = Map::new();
            opt.insert(
                "baseURL".to_string(),
                Value::String("http://localhost:11434/v1".to_string()),
            );
            Value::Object(opt)
        });
        ollama_obj.insert("models".to_string(), models);
    } else {
        // If it's not an object for some reason, replace it.
        let mut o = default_ollama_provider();
        o.insert("models".to_string(), models);
        provider_obj.insert("ollama".to_string(), Value::Object(o));
    }
}

fn default_ollama_provider() -> Map<String, Value> {
    let mut o = Map::new();
    o.insert(
        "npm".to_string(),
        Value::String("@ai-sdk/openai-compatible".to_string()),
    );
    o.insert(
        "name".to_string(),
        Value::String("Ollama (Local)".to_string()),
    );
    let mut opt = Map::new();
    opt.insert(
        "baseURL".to_string(),
        Value::String("http://localhost:11434/v1".to_string()),
    );
    o.insert("options".to_string(), Value::Object(opt));
    o
}

/// Strip `//` and `/* */` comments from a JSONC string.
///
/// This is a minimal implementation intended to handle common JSONC configs.
fn strip_jsonc_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    let mut in_string = false;
    let mut escape = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }

        if c == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    // Line comment: consume until newline (preserve newline).
                    let _ = chars.next();
                    while let Some(nc) = chars.next() {
                        if nc == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    // Block comment: consume until closing */
                    let _ = chars.next();
                    let mut prev = '\0';
                    while let Some(nc) = chars.next() {
                        if prev == '*' && nc == '/' {
                            break;
                        }
                        prev = nc;
                    }
                    continue;
                }
                _ => {}
            }
        }

        out.push(c);
    }

    out
}
