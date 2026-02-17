use crate::error::{Result, TandemError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonCandidate {
    /// "py" | "python" | "python3"
    pub kind: String,
    /// e.g. "Python 3.12.5"
    pub version: String,
    /// Command vector for invocation, e.g. ["py","-3"] or ["python3"]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonStatus {
    pub found: bool,
    pub candidates: Vec<PythonCandidate>,
    pub workspace_path: Option<String>,
    pub venv_root: Option<String>,
    pub venv_python: Option<String>,
    pub venv_exists: bool,
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonInstallResult {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonConfig {
    pub venv_root: String,
    pub selected_python_kind: Option<String>,
    pub created_at_ms: Option<u64>,
    pub last_checked_ms: Option<u64>,
}

fn now_ms() -> u64 {
    // Avoid pulling chrono into this module.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn venv_root_for_workspace(workspace: &Path) -> PathBuf {
    workspace.join(".tandem").join(".venv")
}

pub fn legacy_venv_root_for_workspace(workspace: &Path) -> PathBuf {
    workspace.join(".opencode").join(".venv")
}

pub fn effective_venv_root_for_workspace(workspace: &Path) -> PathBuf {
    let canonical = venv_root_for_workspace(workspace);
    if canonical.exists() {
        return canonical;
    }
    let legacy = legacy_venv_root_for_workspace(workspace);
    if legacy.exists() {
        return legacy;
    }
    canonical
}

pub fn venv_python_for_root(venv_root: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_root.join("Scripts").join("python.exe")
    } else {
        // Many venvs include python3; some only python.
        let py3 = venv_root.join("bin").join("python3");
        if py3.exists() {
            py3
        } else {
            venv_root.join("bin").join("python")
        }
    }
}

pub fn python_config_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".tandem")
        .join("tandem")
        .join("python")
        .join("config.json")
}

fn run_capture(cmd: &mut Command) -> Result<(i32, String, String)> {
    let out = cmd
        .output()
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to run command: {}", e)))?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok((code, stdout, stderr))
}

fn probe_version(command: &[String]) -> Option<String> {
    let (exe, args) = command.split_first()?;
    let mut cmd = Command::new(exe);
    cmd.args(args).arg("--version");

    // Python sometimes prints version to stderr.
    let out = cmd.output().ok()?;
    let s = if !out.stdout.is_empty() {
        String::from_utf8_lossy(&out.stdout).to_string()
    } else {
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn discover_python_candidates() -> Vec<PythonCandidate> {
    let mut candidates: Vec<(String, Vec<String>)> = Vec::new();

    if cfg!(windows) {
        candidates.push(("py".to_string(), vec!["py".to_string(), "-3".to_string()]));
        candidates.push(("python".to_string(), vec!["python".to_string()]));
        candidates.push(("python3".to_string(), vec!["python3".to_string()]));
    } else {
        candidates.push(("python3".to_string(), vec!["python3".to_string()]));
        candidates.push(("python".to_string(), vec!["python".to_string()]));
    }

    let mut out = Vec::new();
    for (kind, command) in candidates {
        if let Some(version) = probe_version(&command) {
            out.push(PythonCandidate {
                kind,
                version,
                command,
            });
        }
    }
    out
}

pub fn read_python_config(workspace: &Path) -> Option<PythonConfig> {
    let canonical = python_config_path(workspace);
    if let Ok(bytes) = fs::read(&canonical) {
        return serde_json::from_slice(&bytes).ok();
    }
    let legacy = workspace
        .join(".opencode")
        .join("tandem")
        .join("python")
        .join("config.json");
    let bytes = fs::read(legacy).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn write_python_config(workspace: &Path, cfg: &PythonConfig) -> Result<()> {
    let p = python_config_path(workspace);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(TandemError::Io)?;
    }
    let s = serde_json::to_string_pretty(cfg).map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to serialize python config: {}", e))
    })?;
    fs::write(p, s).map_err(TandemError::Io)?;
    Ok(())
}

pub fn get_status(workspace: Option<&Path>) -> PythonStatus {
    let candidates = discover_python_candidates();
    let found = !candidates.is_empty();

    let (venv_root, venv_python, venv_exists, config_path) = if let Some(ws) = workspace {
        let venv_root = effective_venv_root_for_workspace(ws);
        let venv_python = venv_python_for_root(&venv_root);
        (
            Some(venv_root.to_string_lossy().to_string()),
            Some(venv_python.to_string_lossy().to_string()),
            venv_root.exists(),
            Some(python_config_path(ws).to_string_lossy().to_string()),
        )
    } else {
        (None, None, false, None)
    };

    PythonStatus {
        found,
        candidates,
        workspace_path: workspace.map(|p| p.to_string_lossy().to_string()),
        venv_root,
        venv_python,
        venv_exists,
        config_path,
    }
}

pub fn create_venv(workspace: &Path, selected_kind: Option<String>) -> Result<PythonStatus> {
    let mut candidates = discover_python_candidates();
    if candidates.is_empty() {
        return Err(TandemError::InvalidConfig(
            "Python was not found. Install Python 3 and re-try.".to_string(),
        ));
    }

    // Choose candidate by kind if provided; else the first.
    let chosen = if let Some(kind) = selected_kind.as_deref() {
        candidates
            .iter()
            .find(|c| c.kind == kind)
            .cloned()
            .unwrap_or_else(|| candidates[0].clone())
    } else {
        candidates[0].clone()
    };

    let venv_root = venv_root_for_workspace(workspace);
    fs::create_dir_all(venv_root.parent().unwrap_or(workspace)).map_err(TandemError::Io)?;

    let (exe, base_args) = chosen
        .command
        .split_first()
        .ok_or_else(|| TandemError::InvalidConfig("Invalid python candidate".to_string()))?;

    let mut cmd = Command::new(exe);
    cmd.args(base_args)
        .arg("-m")
        .arg("venv")
        .arg(&venv_root)
        .current_dir(workspace);

    let (code, stdout, stderr) = run_capture(&mut cmd)?;
    if code != 0 {
        return Err(TandemError::InvalidConfig(format!(
            "Failed to create venv (exit {}): {}\n{}",
            code,
            stdout.trim(),
            stderr.trim()
        )));
    }

    let venv_python = venv_python_for_root(&venv_root);
    if !venv_python.exists() {
        return Err(TandemError::InvalidConfig(format!(
            "Venv created but python not found at: {}",
            venv_python.display()
        )));
    }

    // Best-effort ensure pip.
    let mut ensure = Command::new(&venv_python);
    ensure
        .arg("-m")
        .arg("ensurepip")
        .arg("--upgrade")
        .current_dir(workspace);
    let _ = ensure.output();

    // Record config for the workspace.
    let cfg = PythonConfig {
        venv_root: ".tandem/.venv".to_string(),
        selected_python_kind: Some(chosen.kind),
        created_at_ms: Some(now_ms()),
        last_checked_ms: Some(now_ms()),
    };
    let _ = write_python_config(workspace, &cfg);

    Ok(get_status(Some(workspace)))
}

pub fn install_requirements(
    workspace: &Path,
    requirements_path: &Path,
) -> Result<PythonInstallResult> {
    let venv_root = effective_venv_root_for_workspace(workspace);
    if !venv_root.exists() {
        return Err(TandemError::InvalidConfig(
            "No workspace venv found. Create `.tandem/.venv` first.".to_string(),
        ));
    }

    let venv_python = venv_python_for_root(&venv_root);
    if !venv_python.exists() {
        return Err(TandemError::InvalidConfig(format!(
            "Venv python not found at: {}",
            venv_python.display()
        )));
    }

    let mut cmd = Command::new(&venv_python);
    cmd.arg("-m")
        .arg("pip")
        .arg("install")
        .arg("-r")
        .arg(requirements_path)
        .current_dir(workspace);

    let (code, stdout, stderr) = run_capture(&mut cmd)?;

    Ok(PythonInstallResult {
        ok: code == 0,
        exit_code: Some(code),
        stdout: truncate_text(stdout, 80_000),
        stderr: truncate_text(stderr, 80_000),
    })
}

fn truncate_text(s: String, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut out = String::with_capacity(max_chars + 64);
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            break;
        }
        out.push(ch);
    }
    out.push_str("\n...[truncated]...\n");
    out
}

// -----------------------------
// Enforcement / policy helpers
// -----------------------------

fn tokenize_command(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;

    for ch in input.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else {
                    cur.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch.is_whitespace() {
                    if !cur.is_empty() {
                        tokens.push(cur.clone());
                        cur.clear();
                    }
                } else {
                    cur.push(ch);
                }
            }
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

fn normalize_path_token(ws: &Path, tok: &str) -> PathBuf {
    let t = tok.trim().trim_matches('"').trim_matches('\'');
    let p = PathBuf::from(t);
    if p.is_absolute() {
        p
    } else {
        ws.join(p)
    }
}

fn is_inside(parent: &Path, child: &Path) -> bool {
    // Best-effort normalization without requiring the child to exist.
    child.starts_with(parent)
}

/// Enforce "no global python/pip" policy for AI terminal commands.
/// Returns Some(error_message) if blocked; None if allowed.
pub fn check_terminal_command_policy(workspace: &Path, command: &str) -> Option<String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    let tokens = tokenize_command(trimmed);
    if tokens.is_empty() {
        return None;
    }

    let venv_root = venv_root_for_workspace(workspace);
    let legacy_venv_root = legacy_venv_root_for_workspace(workspace);
    let venv_python = venv_python_for_root(&venv_root);
    let legacy_venv_python = venv_python_for_root(&legacy_venv_root);
    let venv_pip = if cfg!(windows) {
        venv_root.join("Scripts").join("pip.exe")
    } else {
        venv_root.join("bin").join("pip")
    };
    let legacy_venv_pip = if cfg!(windows) {
        legacy_venv_root.join("Scripts").join("pip.exe")
    } else {
        legacy_venv_root.join("bin").join("pip")
    };

    let lc = trimmed.to_lowercase();

    // Allow version checks outside venv.
    if tokens.len() >= 2 {
        let t0 = tokens[0].to_lowercase();
        let t1 = tokens[1].to_lowercase();
        if (t0 == "python" || t0 == "python3" || t0 == "py") && t1 == "--version" {
            return None;
        }
    }

    // Allow venv creation targeting the allowed workspace venv.
    // Patterns:
    // - python -m venv .tandem/.venv
    // - py -3 -m venv .tandem/.venv
    let t0 = tokens[0].to_lowercase();
    if t0 == "python" || t0 == "python3" || t0 == "py" {
        let mut idx = 1usize;
        if t0 == "py" && tokens.get(1).map(|s| s == "-3").unwrap_or(false) {
            idx += 1;
        }
        if tokens.get(idx).map(|s| s == "-m").unwrap_or(false)
            && tokens.get(idx + 1).map(|s| s == "venv").unwrap_or(false)
            && tokens.get(idx + 2).is_some()
        {
            let target = normalize_path_token(workspace, &tokens[idx + 2]);
            if target == venv_root || target == legacy_venv_root {
                return None;
            }
        }
    }

    // Allow if the command explicitly uses the venv python/pip binaries.
    let resolved0 = normalize_path_token(workspace, &tokens[0]);
    if resolved0 == venv_python
        || resolved0 == venv_pip
        || resolved0 == legacy_venv_python
        || resolved0 == legacy_venv_pip
        || is_inside(&venv_root, &resolved0)
        || is_inside(&legacy_venv_root, &resolved0)
    {
        return None;
    }

    // Block any pip installs unless explicitly routed through venv python/pip.
    if lc.contains("pip install")
        || lc.contains("pip3 install")
        || lc.contains("python -m pip install")
    {
        return Some(format!(
            "Global Python/pip installs are blocked. Use the workspace venv at `.tandem/.venv`.\n\nRecommended:\n- Create venv: `python -m venv .tandem/.venv`\n- Install: `\"{}\" -m pip install -r requirements.txt`\n\nOr open the in-app Python setup wizard.",
            venv_python.to_string_lossy()
        ));
    }

    // Block running python outside venv (scripts, -c, -m, etc).
    if t0 == "python" || t0 == "python3" || t0 == "py" {
        return Some(
            "Running Python outside the workspace venv is blocked. Create the venv at `.tandem/.venv` and run Python via the venv interpreter, or use the in-app Python setup wizard.".to_string(),
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_allows_version_checks() {
        let ws = PathBuf::from("C:\\tmp\\ws");
        assert!(check_terminal_command_policy(&ws, "python --version").is_none());
        assert!(check_terminal_command_policy(&ws, "python3 --version").is_none());
        assert!(check_terminal_command_policy(&ws, "py --version").is_none());
    }

    #[test]
    fn policy_allows_venv_creation_target() {
        let ws = PathBuf::from("/tmp/ws");
        assert!(check_terminal_command_policy(&ws, "python -m venv .tandem/.venv").is_none());
        assert!(check_terminal_command_policy(&ws, "py -3 -m venv .tandem/.venv").is_none());
    }

    #[test]
    fn policy_blocks_global_pip_install() {
        let ws = PathBuf::from("/tmp/ws");
        assert!(check_terminal_command_policy(&ws, "pip install pandas").is_some());
        assert!(check_terminal_command_policy(&ws, "python -m pip install pandas").is_some());
    }
}
