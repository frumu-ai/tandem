use super::*;
#[cfg(target_os = "linux")]
use std::ffi::OsString;
#[cfg(target_os = "linux")]
use std::sync::{Mutex, OnceLock};

#[cfg(target_os = "linux")]
fn shell_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(target_os = "linux")]
struct EnvRestore {
    name: &'static str,
    previous: Option<OsString>,
}

#[cfg(target_os = "linux")]
impl EnvRestore {
    fn clear(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        std::env::remove_var(name);
        Self { name, previous }
    }
}

#[cfg(target_os = "linux")]
impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

#[cfg(target_os = "linux")]
fn bwrap_available() -> bool {
    std::process::Command::new("bwrap")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

#[cfg(target_os = "linux")]
fn command_permission_blocked(stderr: &str) -> bool {
    stderr.contains("No permissions")
        || stderr.contains("Operation not permitted")
        || stderr.contains("Creating new namespace failed")
}

#[cfg(target_os = "linux")]
#[test]
fn linux_bwrap_argv_matches_sandbox_policy_snapshot() {
    let workspace = tempfile::tempdir().expect("workspace");
    let root = workspace
        .path()
        .canonicalize()
        .expect("canonical workspace");
    let root_text = root.to_string_lossy().to_string();
    let fake_bwrap = root.join("fake-bwrap");
    let args = json!({
        "__workspace_root": root_text,
        "__effective_cwd": root.to_string_lossy().to_string(),
    });

    let plan = build_bwrap_shell_command_with_bwrap("printf ok", &args, fake_bwrap.clone());
    let ShellCommandPlan::Execute(plan) = plan else {
        panic!("expected executable bwrap plan");
    };
    let argv = plan.args_for_test();
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string());

    assert_eq!(
        plan.program_for_test(),
        fake_bwrap.to_string_lossy().to_string()
    );
    assert_eq!(plan.sandbox_mode_for_test(), "bubblewrap");
    assert_eq!(
        argv,
        vec![
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
            &root_text,
            &root_text,
            "--chdir",
            &root_text,
            "--setenv",
            "PATH",
            &path,
            "--setenv",
            "TMPDIR",
            "/tmp",
            "--setenv",
            "HOME",
            &root_text,
            "--",
            "/bin/sh",
            "-lc",
            "printf ok",
        ]
    );
    assert!(
        argv.iter().any(|arg| arg == "--unshare-all"),
        "Linux shell sandbox must unshare the network namespace by default"
    );
    assert!(
        !argv.iter().any(|arg| arg == "--share-net"),
        "Linux shell sandbox must not opt back into host network access"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn linux_bwrap_sandbox_blocks_outside_writes_and_allows_workspace_writes() {
    let _guard = shell_env_lock().lock().expect("shell env lock");
    let _unsafe_restore = EnvRestore::clear("TANDEM_UNSAFE_UNSANDBOXED_SHELL");
    if !bwrap_available() {
        eprintln!("skipping bwrap sandbox integration: bwrap is not available");
        return;
    }

    let workspace = tempfile::tempdir().expect("workspace");
    let root = workspace
        .path()
        .canonicalize()
        .expect("canonical workspace");
    let command = "\
printf workspace-ok > allowed.txt
if printf denied > /etc/tandem-shell-sandbox-denied 2>/tmp/tandem-denied.err; then
  echo ETC_WRITE_UNEXPECTED
else
  echo ETC_WRITE_DENIED
fi
cat allowed.txt
";

    let result = BashTool
        .execute(json!({
            "command": command,
            "timeout_ms": 10_000,
            "__workspace_root": root.to_string_lossy().to_string(),
            "__effective_cwd": root.to_string_lossy().to_string(),
        }))
        .await
        .expect("bash tool result");
    let stderr = result
        .metadata
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if command_permission_blocked(stderr) {
        eprintln!("skipping bwrap sandbox integration: bwrap cannot create namespaces here");
        return;
    }

    assert_eq!(result.metadata["shell_sandbox"], json!("bubblewrap"));
    assert_eq!(result.metadata["exit_code"], json!(0));
    assert!(
        result.output.contains("ETC_WRITE_DENIED"),
        "outside write must fail; output: {}\nstderr: {}",
        result.output,
        stderr
    );
    assert!(
        result.output.contains("workspace-ok"),
        "workspace write must succeed; output: {}",
        result.output
    );
    assert_eq!(
        std::fs::read_to_string(root.join("allowed.txt")).expect("allowed file"),
        "workspace-ok"
    );
}

#[cfg(unix)]
#[test]
fn unavailable_posix_shell_sandbox_fails_closed_without_explicit_opt_out() {
    let blocked = build_unavailable_posix_shell_command("echo ok", false);
    let ShellCommandPlan::Blocked(result) = blocked else {
        panic!("sandbox-unavailable POSIX shells must fail closed by default");
    };
    assert_eq!(
        result.metadata["guardrail_reason"],
        json!("os_shell_sandbox_unavailable")
    );

    let execute = build_unavailable_posix_shell_command("echo ok", true);
    let ShellCommandPlan::Execute(plan) = execute else {
        panic!("explicit unsafe opt-out should build an unsandboxed POSIX shell plan");
    };
    assert_eq!(plan.sandbox_mode_for_test(), "unsafe_unsandboxed");
}

#[test]
fn windows_shell_translation_and_rejection_matrix_is_stable() {
    let cases = [
        ("ls -la", Some("Get-ChildItem -Force"), None),
        (
            "find src -type f -name \"*.rs\"",
            Some("Get-ChildItem -Path 'src' -Recurse -File -Filter '*.rs'"),
            None,
        ),
        (
            "sed -n '1,5p' README.md",
            None,
            Some("unix_command_untranslatable"),
        ),
        (
            "bash -lc 'echo hi'",
            None,
            Some("unix_command_untranslatable"),
        ),
        ("cat README.md", None, None),
    ];

    for (raw, expected_translation, expected_guardrail) in cases {
        assert_eq!(
            translate_windows_shell_command(raw).as_deref(),
            expected_translation,
            "translation for `{raw}`"
        );
        assert_eq!(
            windows_guardrail_reason(raw),
            expected_guardrail,
            "guardrail reason for `{raw}`"
        );
    }
}
