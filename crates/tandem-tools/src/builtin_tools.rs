use super::*;

pub(crate) fn trimmed_non_empty_str(value: Option<&Value>) -> Option<&str> {
    let text = value.and_then(Value::as_str)?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub(crate) fn is_document_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "pdf" | "docx" | "pptx" | "xlsx" | "xls" | "ods" | "xlsb" | "rtf"
            )
        })
        .unwrap_or(false)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct BashTool;

pub(crate) struct ShellExecutionPlan {
    command: Command,
    translated_command: Option<String>,
    os_guardrail_applied: bool,
    guardrail_reason: Option<String>,
    sandbox_mode: String,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ShellCommandPlan {
    Execute(ShellExecutionPlan),
    Blocked(ToolResult),
}

fn bool_env_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

fn sandbox_blocked_result(reason: &str) -> ShellCommandPlan {
    ShellCommandPlan::Blocked(ToolResult {
        output: format!("Shell command blocked by sandbox policy: {reason}"),
        metadata: json!({
            "blocked": true,
            "shell_sandbox": "blocked",
            "guardrail_reason": reason,
        }),
    })
}

#[cfg(unix)]
fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
pub(crate) fn prepare_shell_workspace(
    args: &Value,
) -> Result<(PathBuf, PathBuf), ShellCommandPlan> {
    let workspace_root = workspace_root_from_args(args)
        .ok_or_else(|| sandbox_blocked_result("missing_workspace_root"))?;
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|_| sandbox_blocked_result("workspace_root_not_canonical"))?;
    let effective_cwd = effective_cwd_from_args(args);
    let effective_cwd = effective_cwd
        .canonicalize()
        .map_err(|_| sandbox_blocked_result("effective_cwd_not_canonical"))?;
    if !is_within_workspace_root(&effective_cwd, &workspace_root) {
        return Err(sandbox_blocked_result("effective_cwd_outside_workspace"));
    }
    Ok((workspace_root, effective_cwd))
}

#[cfg(unix)]
fn build_unsandboxed_posix_shell_command(raw_cmd: &str) -> ShellExecutionPlan {
    let mut command = Command::new("sh");
    command.args(["-lc", raw_cmd]);
    ShellExecutionPlan {
        command,
        translated_command: None,
        os_guardrail_applied: false,
        guardrail_reason: Some("unsafe_unsandboxed_shell_opt_out".to_string()),
        sandbox_mode: "unsafe_unsandboxed".to_string(),
    }
}

#[cfg(all(unix, target_os = "linux"))]
fn build_bwrap_shell_command(raw_cmd: &str, args: &Value) -> ShellCommandPlan {
    if bool_env_enabled("TANDEM_UNSAFE_UNSANDBOXED_SHELL") {
        return ShellCommandPlan::Execute(build_unsandboxed_posix_shell_command(raw_cmd));
    }

    let (workspace_root, effective_cwd) = match prepare_shell_workspace(args) {
        Ok(paths) => paths,
        Err(blocked) => return blocked,
    };
    let bwrap = match find_executable_on_path("bwrap") {
        Some(path) => path,
        None => return sandbox_blocked_result("bubblewrap_not_available"),
    };
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string());

    let mut command = Command::new(bwrap);
    command.env_clear();
    command.args([
        "--die-with-parent",
        "--unshare-all",
        "--new-session",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--ro-bind",
        "/bin",
        "/bin",
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind-try",
        "/lib",
        "/lib",
        "--ro-bind-try",
        "/lib64",
        "/lib64",
        "--ro-bind-try",
        "/etc/alternatives",
        "/etc/alternatives",
        "--bind",
    ]);
    command.arg(&workspace_root);
    command.arg(&workspace_root);
    command.arg("--chdir");
    command.arg(&effective_cwd);
    command.args(["--setenv", "PATH", &path, "--setenv", "TMPDIR", "/tmp"]);
    command.arg("--setenv");
    command.arg("HOME");
    command.arg(&workspace_root);
    command.args(["--", "/bin/sh", "-lc", raw_cmd]);

    ShellCommandPlan::Execute(ShellExecutionPlan {
        command,
        translated_command: None,
        os_guardrail_applied: false,
        guardrail_reason: None,
        sandbox_mode: "bubblewrap".to_string(),
    })
}

#[cfg(all(unix, not(target_os = "linux")))]
fn build_platform_shell_command(raw_cmd: &str, _args: &Value) -> ShellCommandPlan {
    if bool_env_enabled("TANDEM_UNSAFE_UNSANDBOXED_SHELL") {
        return ShellCommandPlan::Execute(build_unsandboxed_posix_shell_command(raw_cmd));
    }
    sandbox_blocked_result("os_shell_sandbox_unavailable")
}

#[cfg(all(unix, target_os = "linux"))]
fn build_platform_shell_command(raw_cmd: &str, args: &Value) -> ShellCommandPlan {
    build_bwrap_shell_command(raw_cmd, args)
}

fn bash_timeout_ms(args: &Value) -> u64 {
    let from_args = args
        .get("timeout_ms")
        .and_then(|value| value.as_u64())
        .filter(|value| *value >= 1_000);
    let from_env = std::env::var("TANDEM_BASH_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value >= 1_000);
    from_args.or(from_env).unwrap_or(30_000)
}

fn shell_metadata(
    translated_command: Option<&str>,
    os_guardrail_applied: bool,
    guardrail_reason: Option<&str>,
    sandbox_mode: &str,
    stderr: String,
) -> Value {
    let mut metadata = json!({
        "stderr": stderr,
        "os_guardrail_applied": os_guardrail_applied,
        "shell_sandbox": sandbox_mode,
    });
    if let Some(obj) = metadata.as_object_mut() {
        if let Some(translated) = translated_command {
            obj.insert(
                "translated_command".to_string(),
                Value::String(translated.to_string()),
            );
        }
        if let Some(reason) = guardrail_reason {
            obj.insert(
                "guardrail_reason".to_string(),
                Value::String(reason.to_string()),
            );
        }
    }
    metadata
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_guardrail_reason(raw_cmd: &str) -> Option<&'static str> {
    let trimmed = raw_cmd.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    let unix_only_prefixes = [
        "awk ", "sed ", "xargs ", "chmod ", "chown ", "sudo ", "apt ", "apt-get ", "yum ", "dnf ",
        "brew ", "zsh ", "bash ", "sh ", "uname", "pwd",
    ];
    if unix_only_prefixes
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return Some("unix_command_untranslatable");
    }
    if trimmed.contains("/dev/null") || trimmed.contains("~/.") {
        return Some("posix_path_pattern");
    }
    None
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn quote_powershell_single(input: &str) -> String {
    format!("'{}'", input.replace('\'', "''"))
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn normalize_shell_token(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn translate_windows_find_command(trimmed: &str) -> Option<String> {
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() || !tokens[0].eq_ignore_ascii_case("find") {
        return None;
    }

    let mut idx = 1usize;
    let mut path = ".".to_string();
    let mut file_only = false;
    let mut patterns: Vec<String> = Vec::new();

    if idx < tokens.len() && !tokens[idx].starts_with('-') {
        path = normalize_shell_token(tokens[idx]);
        idx += 1;
    }

    while idx < tokens.len() {
        let token = tokens[idx].to_ascii_lowercase();
        match token.as_str() {
            "-type" => {
                if idx + 1 < tokens.len() && tokens[idx + 1].eq_ignore_ascii_case("f") {
                    file_only = true;
                }
                idx += 2;
            }
            "-name" => {
                if idx + 1 < tokens.len() {
                    let pattern = normalize_shell_token(tokens[idx + 1]);
                    if !pattern.is_empty() {
                        patterns.push(pattern);
                    }
                }
                idx += 2;
            }
            "-o" | "-or" | "(" | ")" => {
                idx += 1;
            }
            _ => {
                idx += 1;
            }
        }
    }

    let mut translated = format!("Get-ChildItem -Path {}", quote_powershell_single(&path));
    translated.push_str(" -Recurse");
    if file_only {
        translated.push_str(" -File");
    }

    if patterns.len() == 1 {
        translated.push_str(" -Filter ");
        translated.push_str(&quote_powershell_single(&patterns[0]));
    } else if patterns.len() > 1 {
        translated.push_str(" -Include ");
        let include_list = patterns
            .iter()
            .map(|p| quote_powershell_single(p))
            .collect::<Vec<_>>()
            .join(",");
        translated.push_str(&include_list);
    }

    Some(translated)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn translate_windows_ls_command(trimmed: &str) -> Option<String> {
    let mut force = false;
    let mut paths: Vec<&str> = Vec::new();
    for token in trimmed.split_whitespace().skip(1) {
        if token.starts_with('-') {
            let flags = token.trim_start_matches('-').to_ascii_lowercase();
            if flags.contains('a') {
                force = true;
            }
            continue;
        }
        paths.push(token);
    }

    let mut translated = String::from("Get-ChildItem");
    if force {
        translated.push_str(" -Force");
    }
    if !paths.is_empty() {
        translated.push_str(" -Path ");
        translated.push_str(&quote_powershell_single(&paths.join(" ")));
    }
    Some(translated)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn translate_windows_shell_command(raw_cmd: &str) -> Option<String> {
    let trimmed = raw_cmd.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("ls") {
        return translate_windows_ls_command(trimmed);
    }
    if lowered.starts_with("find ") {
        return translate_windows_find_command(trimmed);
    }
    None
}

fn build_shell_command(raw_cmd: &str, args: &Value) -> ShellCommandPlan {
    #[cfg(windows)]
    {
        let reason = windows_guardrail_reason(raw_cmd);
        let translated = translate_windows_shell_command(raw_cmd);
        let translated_applied = translated.is_some();
        if let Some(reason) = reason {
            if translated.is_none() {
                return ShellCommandPlan::Blocked(ToolResult {
                    output: format!(
                        "Shell command blocked on Windows ({reason}). Use cross-platform tools (`read`, `glob`, `grep`) or PowerShell-native syntax."
                    ),
                    metadata: json!({
                        "os_guardrail_applied": true,
                        "guardrail_reason": reason,
                        "blocked": true,
                        "shell_sandbox": "windows_guardrail"
                    }),
                });
            }
        }
        let effective = translated.clone().unwrap_or_else(|| raw_cmd.to_string());
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-Command", &effective]);
        return ShellCommandPlan::Execute(ShellExecutionPlan {
            command,
            translated_command: translated,
            os_guardrail_applied: reason.is_some() || translated_applied,
            guardrail_reason: reason.map(str::to_string),
            sandbox_mode: "windows_guardrail".to_string(),
        });
    }

    #[cfg(not(windows))]
    {
        build_platform_shell_command(raw_cmd, args)
    }
}

async fn run_bash_command(
    cmd: &str,
    args: &Value,
    cancel: Option<CancellationToken>,
) -> anyhow::Result<ToolResult> {
    let shell = match build_shell_command(cmd, args) {
        ShellCommandPlan::Execute(plan) => plan,
        ShellCommandPlan::Blocked(result) => return Ok(result),
    };

    let ShellExecutionPlan {
        mut command,
        translated_command,
        os_guardrail_applied,
        guardrail_reason,
        sandbox_mode,
    } = shell;
    let effective_cwd = effective_cwd_from_args(args);
    command.current_dir(&effective_cwd);
    if let Some(env) = args.get("env").and_then(|v| v.as_object()) {
        for (k, v) in env {
            if let Some(value) = v.as_str() {
                command.env(k, value);
            }
        }
    }
    let timeout_ms = bash_timeout_ms(args);

    if let Some(cancel) = cancel {
        let timeout = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms));
        tokio::pin!(timeout);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let status = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Ok(ToolResult {
                    output: "command cancelled".to_string(),
                    metadata: json!({"cancelled": true}),
                });
            }
            _ = &mut timeout => {
                let _ = child.kill().await;
                return Ok(ToolResult {
                    output: format!("command timed out after {} ms", timeout_ms),
                    metadata: json!({"timeout": true, "timeout_ms": timeout_ms}),
                });
            }
            result = child.wait() => result?
        };
        let stdout = match child.stdout.take() {
            Some(mut handle) => {
                use tokio::io::AsyncReadExt;
                let mut buf = Vec::new();
                let _ = handle.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).to_string()
            }
            None => String::new(),
        };
        let stderr = match child.stderr.take() {
            Some(mut handle) => {
                use tokio::io::AsyncReadExt;
                let mut buf = Vec::new();
                let _ = handle.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).to_string()
            }
            None => String::new(),
        };
        let mut metadata = shell_metadata(
            translated_command.as_deref(),
            os_guardrail_applied,
            guardrail_reason.as_deref(),
            &sandbox_mode,
            stderr,
        );
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert("exit_code".to_string(), json!(status.code()));
            obj.insert(
                "effective_cwd".to_string(),
                Value::String(effective_cwd.to_string_lossy().to_string()),
            );
            if let Some(workspace_root) = workspace_root_from_args(args) {
                obj.insert(
                    "workspace_root".to_string(),
                    Value::String(workspace_root.to_string_lossy().to_string()),
                );
            }
        }
        return Ok(ToolResult {
            output: if stdout.is_empty() {
                format!("command exited: {}", status)
            } else {
                stdout
            },
            metadata,
        });
    }

    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let output = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        command.output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("BASH_TIMEOUT_MS_EXCEEDED({timeout_ms})"))??;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut metadata = shell_metadata(
        translated_command.as_deref(),
        os_guardrail_applied,
        guardrail_reason.as_deref(),
        &sandbox_mode,
        stderr,
    );
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert(
            "effective_cwd".to_string(),
            Value::String(effective_cwd.to_string_lossy().to_string()),
        );
        if let Some(workspace_root) = workspace_root_from_args(args) {
            obj.insert(
                "workspace_root".to_string(),
                Value::String(workspace_root.to_string_lossy().to_string()),
            );
        }
    }
    Ok(ToolResult {
        output: String::from_utf8_lossy(&output.stdout).to_string(),
        metadata,
    })
}

#[async_trait]
impl Tool for BashTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "bash",
            "Run shell command",
            json!({
                "type":"object",
                "properties":{
                    "command":{"type":"string"},
                    "timeout_ms":{"type":"integer","minimum":1000}
                },
                "required":["command"]
            }),
            shell_execution_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let cmd = args["command"].as_str().unwrap_or("").trim();
        if cmd.is_empty() {
            anyhow::bail!("BASH_COMMAND_MISSING");
        }
        run_bash_command(cmd, &args, None).await
    }

    async fn execute_with_cancel(
        &self,
        args: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let cmd = args["command"].as_str().unwrap_or("").trim();
        if cmd.is_empty() {
            anyhow::bail!("BASH_COMMAND_MISSING");
        }
        run_bash_command(cmd, &args, Some(cancel)).await
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct ReadTool;

fn document_tool_metadata(path: &str, _path_buf: &PathBuf, format: &str) -> Value {
    json!({
        "path": path,
        "type": "document",
        "format": format
    })
}

fn document_limits_from_args(args: &Value) -> tandem_document::ExtractLimits {
    let mut limits = tandem_document::ExtractLimits::default();
    if let Some(max_size) = args["max_size"].as_u64() {
        limits.max_file_bytes = max_size;
    }
    if let Some(max_chars) = args["max_chars"].as_u64() {
        limits.max_output_chars = max_chars as usize;
    }
    limits
}

#[async_trait]
impl Tool for ReadTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "read",
            "Read file contents, including plain text and common documents (PDF, DOCX, PPTX, spreadsheets, RTF).",
            json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to read"
                    },
                    "max_size": {
                        "type": "integer",
                        "description": "Maximum file size in bytes (default: 25MB)"
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum output characters (default: 200,000)"
                    }
                },
                "required": ["path"]
            }),
            workspace_read_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let path = args["path"].as_str().unwrap_or("").trim();
        let Some(mut path_buf) = resolve_tool_path(path, &args) else {
            return Ok(sandbox_path_denied_result(path, &args));
        };

        let metadata = match fs::metadata(&path_buf).await {
            Ok(meta) => meta,
            Err(first_err) => {
                if let Some(recovered) = resolve_read_path_fallback(path, &args) {
                    path_buf = recovered;
                    match fs::metadata(&path_buf).await {
                        Ok(meta) => meta,
                        Err(err) => {
                            return Ok(ToolResult {
                                output: format!("read failed: {}", err),
                                metadata: json!({
                                    "ok": false,
                                    "reason": "path_not_found",
                                    "path": path,
                                    "resolved_path": path_buf.to_string_lossy(),
                                    "error": err.to_string()
                                }),
                            });
                        }
                    }
                } else {
                    return Ok(ToolResult {
                        output: format!("read failed: {}", first_err),
                        metadata: json!({
                            "ok": false,
                            "reason": "path_not_found",
                            "path": path,
                            "error": first_err.to_string()
                        }),
                    });
                }
            }
        };
        if metadata.is_dir() {
            return Ok(ToolResult {
                output: format!(
                    "read failed: `{}` is a directory. Use `glob` to enumerate files, then `read` a concrete file path.",
                    path
                ),
                metadata: json!({
                    "ok": false,
                    "reason": "path_is_directory",
                    "path": path
                }),
            });
        }

        if is_document_file(&path_buf) {
            let limits = document_limits_from_args(&args);
            let format = path_buf
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("unknown")
                .to_ascii_lowercase();
            return match tandem_document::extract_file_text(&path_buf, limits) {
                Ok(text) => Ok(ToolResult {
                    output: text,
                    metadata: document_tool_metadata(path, &path_buf, &format),
                }),
                Err(err) => Ok(ToolResult {
                    output: format!("Failed to extract document text: {}", err),
                    metadata: json!({"path": path, "error": true}),
                }),
            };
        }

        let data = match fs::read_to_string(&path_buf).await {
            Ok(data) => data,
            Err(err) => {
                return Ok(ToolResult {
                    output: format!("read failed: {}", err),
                    metadata: json!({
                        "ok": false,
                        "reason": "read_text_failed",
                        "path": path_buf.to_string_lossy(),
                        "error": err.to_string()
                    }),
                });
            }
        };
        Ok(ToolResult {
            output: data,
            metadata: json!({"path": path_buf.to_string_lossy(), "type": "text"}),
        })
    }
}
