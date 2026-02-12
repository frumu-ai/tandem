//! Tandem engine configuration helpers.
//!
//! Manages global and project config in a round-trip-safe way and keeps
//! compatibility with legacy OpenCode config locations.

use crate::error::{Result, TandemError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TandemConfigScope {
    Global,
    Project,
}

pub fn global_config_path() -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(config_dir) = dirs::config_dir() {
        candidates.push(config_dir.join("tandem").join("config.json"));
        candidates.push(config_dir.join("tandem").join("config.jsonc"));
    }

    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config").join("tandem").join("config.json"));
        candidates.push(home.join(".config").join("tandem").join("config.jsonc"));
    }

    // Legacy OpenCode fallbacks for migration compatibility.
    if let Some(config_dir) = dirs::config_dir() {
        candidates.push(config_dir.join("opencode").join("config.json"));
        candidates.push(config_dir.join("opencode").join("opencode.json"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config").join("opencode").join("opencode.json"));
        candidates.push(home.join(".config").join("opencode").join("opencode.jsonc"));
    }

    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
        }
    }

    if let Some(config_dir) = dirs::config_dir() {
        return Ok(config_dir.join("tandem").join("config.json"));
    }
    if let Some(home) = dirs::home_dir() {
        return Ok(home.join(".config").join("tandem").join("config.json"));
    }

    Err(TandemError::InvalidConfig(
        "Could not determine Tandem global config path".to_string(),
    ))
}

pub fn project_config_path(workspace: &Path) -> PathBuf {
    let tandem_json = workspace.join(".tandem").join("config.json");
    let tandem_jsonc = workspace.join(".tandem").join("config.jsonc");
    let legacy_json = workspace.join("opencode.json");
    let legacy_jsonc = workspace.join("opencode.jsonc");

    if tandem_json.exists() {
        tandem_json
    } else if tandem_jsonc.exists() {
        tandem_jsonc
    } else if legacy_json.exists() {
        legacy_json
    } else if legacy_jsonc.exists() {
        legacy_jsonc
    } else {
        tandem_json
    }
}

pub fn get_config_path(scope: TandemConfigScope, workspace: Option<&Path>) -> Result<PathBuf> {
    match scope {
        TandemConfigScope::Global => global_config_path(),
        TandemConfigScope::Project => {
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
    let stripped = strip_jsonc_comments(&raw);
    let mut v: Value = serde_json::from_str(&stripped).map_err(TandemError::Serialization)?;
    if !v.is_object() {
        v = Value::Object(Map::new());
    }
    Ok(v)
}

pub fn write_config_atomic(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| TandemError::InvalidConfig(format!("Invalid config path: {:?}", path)))?;
    fs::create_dir_all(parent).map_err(TandemError::Io)?;

    let json = serde_json::to_string_pretty(value).map_err(TandemError::Serialization)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.json");
    let tmp_path = parent.join(format!(".{}.tmp", file_name));

    {
        let mut f = fs::File::create(&tmp_path).map_err(TandemError::Io)?;
        f.write_all(json.as_bytes()).map_err(TandemError::Io)?;
        f.write_all(b"\n").map_err(TandemError::Io)?;
        f.sync_all().ok();
    }

    if fs::rename(&tmp_path, path).is_ok() {
        return Ok(());
    }

    let backup_path = parent.join(format!(".{}.bak", file_name));
    if backup_path.exists() {
        let _ = fs::remove_file(&backup_path);
    }

    if path.exists() {
        fs::rename(path, &backup_path).map_err(TandemError::Io)?;
    }

    match fs::rename(&tmp_path, path) {
        Ok(_) => {
            if backup_path.exists() {
                let _ = fs::remove_file(&backup_path);
            }
            Ok(())
        }
        Err(e) => {
            if backup_path.exists() && !path.exists() {
                let _ = fs::rename(&backup_path, path);
            }
            let _ = fs::remove_file(&tmp_path);
            Err(TandemError::Io(e))
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
    scope: TandemConfigScope,
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
    let Some(obj) = cfg.as_object_mut() else {
        return;
    };
    obj.entry("$schema".to_string())
        .or_insert_with(|| Value::String("https://tandem.frumu.ai/config.json".to_string()));
}

pub fn set_provider_ollama_models(cfg: &mut Value, models: Value) {
    ensure_schema(cfg);
    let root = cfg.as_object_mut().expect("config must be object");
    let provider = root
        .entry("provider".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let provider_obj = provider.as_object_mut().expect("provider must be object");
    let ollama = provider_obj
        .entry("ollama".to_string())
        .or_insert_with(|| Value::Object(default_ollama_provider()));

    if let Some(ollama_obj) = ollama.as_object_mut() {
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
                    let _ = chars.next();
                    for nc in chars.by_ref() {
                        if nc == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    let _ = chars.next();
                    let mut prev = '\0';
                    for nc in chars.by_ref() {
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
