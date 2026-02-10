use crate::python_env;
use std::path::Path;

/// Returns Some(denial_message) if the operation should be blocked by the strict
/// workspace venv Python policy; otherwise None.
///
/// Applies only to terminal-like tools that may execute arbitrary shell commands.
pub fn python_policy_violation(
    workspace: &Path,
    tool_name: &str,
    args: &serde_json::Value,
) -> Option<String> {
    let is_terminal_tool = matches!(
        tool_name,
        "bash" | "shell" | "run_command" | "terminal" | "cmd"
    );
    if !is_terminal_tool {
        return None;
    }

    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("cmd").and_then(|v| v.as_str()))
        .or_else(|| args.get("script").and_then(|v| v.as_str()));

    command.and_then(|c| python_env::check_terminal_command_policy(workspace, c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    fn make_ws() -> PathBuf {
        let ws =
            std::env::temp_dir().join(format!("tandem_tool_policy_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(ws.join(".opencode")).unwrap();
        ws
    }

    #[test]
    fn blocks_python_in_terminal_tool() {
        let ws = make_ws();
        let args = json!({ "command": "python -c \"print(1)\"" });
        assert!(python_policy_violation(&ws, "bash", &args).is_some());
    }

    #[test]
    fn allows_non_terminal_tool() {
        let ws = make_ws();
        let args = json!({ "command": "python -c \"print(1)\"" });
        assert!(python_policy_violation(&ws, "write", &args).is_none());
    }

    #[test]
    fn blocks_pip_install() {
        let ws = make_ws();
        let args = json!({ "cmd": "pip install pandas" });
        assert!(python_policy_violation(&ws, "shell", &args).is_some());
    }

    #[test]
    fn allows_venv_python_binary() {
        let ws = make_ws();

        // Create a fake venv python path expected by policy.
        let venv_python = if cfg!(windows) {
            ws.join(".opencode")
                .join(".venv")
                .join("Scripts")
                .join("python.exe")
        } else {
            ws.join(".opencode")
                .join(".venv")
                .join("bin")
                .join("python3")
        };
        fs::create_dir_all(venv_python.parent().unwrap()).unwrap();
        fs::write(&venv_python, b"").unwrap();

        let args = json!({ "script": format!("\"{}\" -c \"print(1)\"", venv_python.display()) });
        assert!(python_policy_violation(&ws, "terminal", &args).is_none());
    }

    #[test]
    fn ignores_missing_command() {
        let ws = make_ws();
        let args = json!({ "nope": "nothing" });
        assert!(python_policy_violation(&ws, "bash", &args).is_none());
    }
}
