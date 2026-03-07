use std::fs;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;

fn resolve_editor_command() -> Result<String, String> {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .map_err(|_| "neither VISUAL nor EDITOR is set".to_string())
        .and_then(|cmd| {
            if cmd.trim().is_empty() {
                Err("editor command is empty".to_string())
            } else {
                Ok(cmd)
            }
        })
}

pub async fn run_editor(seed: &str) -> Result<String, String> {
    let editor_cmd = resolve_editor_command()?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("tandem-tui-edit-{ts}.md"));
    fs::write(&path, seed).map_err(|err| format!("failed to write temp file: {err}"))?;
    let path_str = path.to_string_lossy().to_string();

    let status = if cfg!(windows) {
        let full = format!("{editor_cmd} \"{path_str}\"");
        Command::new("cmd")
            .args(["/C", &full])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await
    } else {
        let full = format!("{editor_cmd} \"{path_str}\"");
        Command::new("sh")
            .args(["-lc", &full])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await
    }
    .map_err(|err| format!("failed to launch editor: {err}"))?;

    if !status.success() {
        let _ = fs::remove_file(&path);
        return Err(format!("editor exited with status {}", status));
    }

    let contents =
        fs::read_to_string(&path).map_err(|err| format!("failed to read edits: {err}"))?;
    let _ = fs::remove_file(&path);
    Ok(contents)
}
