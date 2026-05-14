use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use regex::Regex;

const REDACTED_SECRET: &str = "[redacted-secret]";
const REDACTED_PATH: &str = "[redacted-path]";

static DEFAULT_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
static CONFIGURED_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

pub fn redact_outbound(text: &str, workspace_root: &Path) -> String {
    if text.is_empty() {
        return String::new();
    }

    let root = normalize_path(workspace_root);
    let mut output = String::with_capacity(text.len());
    for (idx, line) in text.lines().enumerate() {
        if idx > 0 {
            output.push('\n');
        }
        let secret_redacted = redact_secret_line(line);
        output.push_str(&redact_paths(&secret_redacted, root.as_deref()));
    }
    if text.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn redact_secret_line(line: &str) -> String {
    let mut redacted = line.to_string();
    for pattern in default_patterns()
        .iter()
        .chain(configured_patterns().iter())
    {
        redacted = pattern.replace_all(&redacted, REDACTED_SECRET).to_string();
    }
    redacted
}

fn redact_paths(line: &str, workspace_root: Option<&Path>) -> String {
    let Some(workspace_root) = workspace_root else {
        return line.to_string();
    };
    let mut out = String::with_capacity(line.len());
    let mut token = String::new();
    for ch in line.chars() {
        if is_path_token_char(ch) {
            token.push(ch);
            continue;
        }
        flush_path_token(&mut out, &mut token, workspace_root);
        out.push(ch);
    }
    flush_path_token(&mut out, &mut token, workspace_root);
    out
}

fn flush_path_token(out: &mut String, token: &mut String, workspace_root: &Path) {
    if token.is_empty() {
        return;
    }
    if token.starts_with('/') && path_is_outside_workspace(token, workspace_root) {
        out.push_str(REDACTED_PATH);
    } else {
        out.push_str(token);
    }
    token.clear();
}

fn path_is_outside_workspace(token: &str, workspace_root: &Path) -> bool {
    let trimmed =
        token.trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | ')' | ']' | '}'));
    if trimmed == "/" {
        return false;
    }
    let candidate = Path::new(trimmed);
    if !candidate.is_absolute() {
        return false;
    }
    match normalize_path(candidate) {
        Some(path) => !path.starts_with(workspace_root),
        None => true,
    }
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        Some(path.components().collect())
    } else {
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join(path).components().collect())
    }
}

fn is_path_token_char(ch: char) -> bool {
    ch == '/' || ch == '.' || ch == '_' || ch == '-' || ch.is_ascii_alphanumeric()
}

fn default_patterns() -> &'static [Regex] {
    DEFAULT_PATTERNS.get_or_init(|| {
        [
            r"AKIA[0-9A-Z]{16}",
            r"ASIA[0-9A-Z]{16}",
            r"github_pat_[A-Za-z0-9_]{20,}",
            r"gh[pousr]_[A-Za-z0-9_]{20,}",
            r"sk-[A-Za-z0-9]{20,}",
            r"xox[baprs]-[A-Za-z0-9-]{20,}",
            r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        ]
        .into_iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    })
}

fn configured_patterns() -> &'static [Regex] {
    CONFIGURED_PATTERNS.get_or_init(|| {
        let Ok(path) = std::env::var("TANDEM_CHANNEL_REDACTION_PATTERNS_FILE") else {
            return Vec::new();
        };
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        raw.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|pattern| Regex::new(pattern).ok())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_aws_access_key() {
        let key = format!("{}{}", "AKIA123456", "7890ABCDEF");
        assert_eq!(
            redact_outbound(&format!("key {key} ok"), Path::new("/workspace")),
            "key [redacted-secret] ok"
        );
    }

    #[test]
    fn redacts_github_token() {
        assert_eq!(
            redact_outbound(
                "token ghp_abcdefghijklmnopqrstuvwxyz",
                Path::new("/workspace")
            ),
            "token [redacted-secret]"
        );
    }

    #[test]
    fn redacts_github_pat() {
        assert_eq!(
            redact_outbound(
                "github_pat_11ABCDEFGHIJKLMNOPQRSTUVWXYZ",
                Path::new("/workspace")
            ),
            "[redacted-secret]"
        );
    }

    #[test]
    fn redacts_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijklmnopqrstuvwxyz";
        assert_eq!(
            redact_outbound(jwt, Path::new("/workspace")),
            "[redacted-secret]"
        );
    }

    #[test]
    fn redacts_private_key_marker() {
        let marker = format!("{}{}", "-----BEGIN RSA ", "PRIVATE KEY-----");
        assert_eq!(
            redact_outbound(&marker, Path::new("/workspace")),
            "[redacted-secret]"
        );
    }

    #[test]
    fn redacts_slack_token() {
        let token = format!("{}{}", "xox", "b-1234567890-abcdefghijklmnop");
        assert_eq!(
            redact_outbound(&token, Path::new("/workspace")),
            "[redacted-secret]"
        );
    }

    #[test]
    fn redacts_openai_style_key() {
        assert_eq!(
            redact_outbound("sk-abcdefghijklmnopqrstuvwxyz", Path::new("/workspace")),
            "[redacted-secret]"
        );
    }

    #[test]
    fn redacts_absolute_paths_outside_workspace() {
        assert_eq!(
            redact_outbound(
                "see /home/evan/.ssh/id_rsa and /workspace/project/file.txt",
                Path::new("/workspace")
            ),
            "see [redacted-path] and /workspace/project/file.txt"
        );
    }

    #[test]
    fn preserves_markdown_structure() {
        let input = "- file: `/secret/path.txt`\n- ok: `/workspace/a.md`\n";
        assert_eq!(
            redact_outbound(input, Path::new("/workspace")),
            "- file: `[redacted-path]`\n- ok: `/workspace/a.md`\n"
        );
    }

    #[test]
    fn redacts_4kb_message_quickly() {
        let key = format!("{}{}", "AKIA123456", "7890ABCDEF");
        let input = format!("{}\n{key}", "safe text ".repeat(450));
        let _ = redact_outbound("warmup", Path::new("/workspace"));
        let started = Instant::now();
        let redacted = redact_outbound(&input, Path::new("/workspace"));
        assert!(started.elapsed().as_millis() < 2);
        assert!(redacted.contains(REDACTED_SECRET));
    }
}
