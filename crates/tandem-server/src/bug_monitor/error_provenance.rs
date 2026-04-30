//! Deterministic error-string → workspace-source-location lookup.
//!
//! When a Bug Monitor incident has an error message that contains a
//! distinctive literal (most do — runtime emit_event payloads, anyhow
//! bail strings, panic messages), this module greps the workspace's
//! tracked source files and returns the matching file/line/snippet.
//!
//! Pure code, no LLM, no triage dependency. Runs at issue-creation
//! time. Intent: every issue lands in front of the autonomous coding
//! agent (or a human reviewer) with a concrete starting point in the
//! codebase, even when the LLM-driven triage hasn't run or has
//! produced unrelated file references.

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

const GREP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HITS: usize = 10;
const MAX_SUBSTRINGS: usize = 3;
const MIN_SUBSTRING_CHARS: usize = 20;
const MIN_SUBSTRING_WORDS: usize = 3;
const MATCH_LINE_TRUNCATE: usize = 240;

/// One match in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceHit {
    /// Workspace-relative path.
    pub path: String,
    pub line: u32,
    /// The matched line itself, truncated to a sane width. Reviewers
    /// can click `path:line` to see surrounding context — emitting
    /// context lines from `git grep -C` is ambiguous to parse when
    /// paths contain `-`, so we keep the snippet to the matched line.
    pub snippet: String,
}

/// Strip dynamic tokens (IDs, paths, durations, quoted templated
/// arguments) and return the longest static substrings worth grepping
/// for. Capped at [`MAX_SUBSTRINGS`] entries, longest first.
pub fn distinctive_substrings(error: &str) -> Vec<String> {
    let cleaned = strip_dynamic_tokens(error);
    let mut runs = collect_runs(&cleaned);
    runs.sort_by(|a, b| b.len().cmp(&a.len()));
    runs.dedup();
    runs.truncate(MAX_SUBSTRINGS);
    runs
}

fn strip_dynamic_tokens(error: &str) -> String {
    let mut out = String::with_capacity(error.len());
    let mut chars = error.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            // Drop backtick-quoted segments wholesale: `xxx`.
            '`' => {
                for inner in chars.by_ref() {
                    if inner == '`' {
                        break;
                    }
                }
                out.push(' ');
            }
            // Drop single-quoted segments — be cautious about
            // apostrophes inside words. Only strip if the next char is
            // a non-space (likely a quoted token) and we find a closing
            // quote within ~64 chars.
            '\'' => {
                let mut buffer = String::new();
                let mut found_close = false;
                for inner in chars.by_ref().take(64) {
                    if inner == '\'' {
                        found_close = true;
                        break;
                    }
                    buffer.push(inner);
                }
                if found_close && !buffer.is_empty() {
                    out.push(' ');
                } else {
                    out.push('\'');
                    out.push_str(&buffer);
                }
            }
            // Drop digit runs and trailing unit-like suffix (s, ms, h,
            // min) so "180000 ms" → " " and we don't grep for a
            // run-specific number.
            d if d.is_ascii_digit() => {
                while let Some(next) = chars.peek() {
                    if next.is_ascii_digit() {
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push(' ');
            }
            _ => out.push(ch),
        }
    }
    out
}

fn collect_runs(cleaned: &str) -> Vec<String> {
    let words = cleaned
        .split_whitespace()
        .filter(|w| !looks_like_dynamic_token(w))
        .collect::<Vec<_>>();
    if words.is_empty() {
        return Vec::new();
    }
    // Take the whole cleaned line as one run — grep needs contiguous
    // text — and shorter sub-runs as fallbacks.
    let mut out = Vec::new();
    let joined = words.join(" ");
    if substring_qualifies(&joined) {
        out.push(joined.clone());
    }
    // Also add the longest contiguous chunk between any commas/colons
    // so a long error like "X: Y, Z" yields three independent greps.
    for chunk in cleaned.split(|c: char| c == ':' || c == ',' || c == ';' || c == '\n') {
        let chunk = chunk
            .split_whitespace()
            .filter(|w| !looks_like_dynamic_token(w))
            .collect::<Vec<_>>()
            .join(" ");
        if substring_qualifies(&chunk) && !out.iter().any(|existing| existing == &chunk) {
            out.push(chunk);
        }
    }
    out
}

fn substring_qualifies(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.len() < MIN_SUBSTRING_CHARS && trimmed.split_whitespace().count() < MIN_SUBSTRING_WORDS
    {
        return false;
    }
    true
}

fn looks_like_dynamic_token(word: &str) -> bool {
    if word.is_empty() {
        return true;
    }
    // UUIDs and IDs with hyphens and hex.
    let hex_or_dash = word
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '-' || c == '_');
    if hex_or_dash && word.len() >= 8 {
        return true;
    }
    // Absolute or workspace-relative paths.
    if word.starts_with('/') || word.contains('/') {
        return true;
    }
    // Long alnum strings without vowels look like hashes/tokens.
    let alnum_only = word.chars().all(|c| c.is_ascii_alphanumeric());
    if alnum_only && word.len() >= 12 {
        let has_vowel = word
            .chars()
            .any(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u'));
        if !has_vowel {
            return true;
        }
    }
    false
}

/// Run `git grep` for the distinctive substrings of `error_message` in
/// `workspace_root`. Returns up to [`MAX_HITS`] matches across source
/// files. Best-effort: any failure (no git, timeout, no matches)
/// results in an empty `Vec`.
pub async fn locate_error_provenance(
    workspace_root: &Path,
    error_message: &str,
) -> Vec<ProvenanceHit> {
    let substrings = distinctive_substrings(error_message);
    if substrings.is_empty() {
        return Vec::new();
    }
    let workspace_root = workspace_root.to_path_buf();
    let mut hits: Vec<ProvenanceHit> = Vec::new();
    for needle in substrings {
        if hits.len() >= MAX_HITS {
            break;
        }
        match timeout(GREP_TIMEOUT, git_grep(&workspace_root, &needle)).await {
            Ok(Ok(found)) => {
                for hit in found {
                    if hits.len() >= MAX_HITS {
                        break;
                    }
                    if hits
                        .iter()
                        .any(|existing| existing.path == hit.path && existing.line == hit.line)
                    {
                        continue;
                    }
                    hits.push(hit);
                }
            }
            _ => continue,
        }
    }
    hits
}

async fn git_grep(workspace_root: &Path, needle: &str) -> std::io::Result<Vec<ProvenanceHit>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .arg("grep")
        .arg("-n")
        .arg("-F")
        .arg("--no-color")
        .arg(needle)
        .arg("--")
        .args([
            "*.rs",
            "*.ts",
            "*.tsx",
            "*.js",
            "*.jsx",
            "*.py",
            "*.go",
            "*.java",
            "*.kt",
            "*.swift",
        ])
        .output()
        .await?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(parse_git_grep_output(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_git_grep_output(stdout: &str) -> Vec<ProvenanceHit> {
    // `git grep -n` emits `path:line:content`. With `-F` (literal) and
    // no `-C`, every line is a match — no context lines, no `--`
    // record separators, no ambiguity from dashes in paths.
    let mut hits = Vec::new();
    for raw in stdout.lines() {
        if raw.is_empty() {
            continue;
        }
        let Some((path, line_no, body)) = split_grep_line(raw) else {
            continue;
        };
        let snippet = truncate_on_char_boundary(body, MATCH_LINE_TRUNCATE);
        hits.push(ProvenanceHit {
            path: path.to_string(),
            line: line_no,
            snippet,
        });
    }
    hits
}

/// Split a `path:line:body` line. Searches from the right because
/// the body can contain `:`. The line number is the integer between
/// the last two `:` separators. Returns `None` if the line doesn't
/// match the shape (which happens for malformed input or non-grep
/// output we accidentally received).
fn split_grep_line(raw: &str) -> Option<(&str, u32, &str)> {
    // Find the second-from-the-end colon by scanning from the start
    // for the path-line boundary: smallest i such that everything
    // after the colon up to the next colon parses as a number.
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            // Try to parse digits following the colon.
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start && end < bytes.len() && bytes[end] == b':' {
                if let Ok(n) = raw[start..end].parse::<u32>() {
                    return Some((&raw[..i], n, &raw[end + 1..]));
                }
            }
        }
        i += 1;
    }
    None
}

/// Render a markdown section for an issue body. Returns `None` when
/// there are no hits, so callers can avoid emitting an empty section.
pub fn render_provenance_section(hits: &[ProvenanceHit]) -> Option<String> {
    if hits.is_empty() {
        return None;
    }
    let mut out = String::from("### Error provenance\n\n");
    out.push_str(
        "Likely emission sites for the failure message in this workspace:\n\n",
    );
    let mut total = 0usize;
    for hit in hits {
        let entry = format!(
            "- `{}:{}`\n  ```\n{}\n  ```\n",
            hit.path,
            hit.line,
            indent_snippet(&hit.snippet)
        );
        if total + entry.len() > 3_000 {
            break;
        }
        total += entry.len();
        out.push_str(&entry);
    }
    Some(out)
}

fn indent_snippet(snippet: &str) -> String {
    snippet
        .lines()
        .map(|line| format!("  {}", truncate_on_char_boundary_no_ellipsis(line, 200)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Truncate `s` to at most `max_bytes` bytes on a UTF-8 character
/// boundary, appending `…` when truncation occurred. Indexing
/// `&s[..n]` directly is unsafe for arbitrary `n` because Rust panics
/// when `n` falls inside a multi-byte character; this helper steps
/// back to the previous boundary so any source line containing
/// multibyte characters survives the snippet step.
fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn truncate_on_char_boundary_no_ellipsis(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinctive_substrings_strips_backtick_segments() {
        let result = distinctive_substrings(
            "automation node `search_multi_agent` timed out after 180000 ms",
        );
        let joined = result.join(" | ");
        assert!(
            joined.contains("automation node") && joined.contains("timed out after"),
            "should keep static text: {joined}"
        );
        assert!(
            !joined.contains("search_multi_agent"),
            "should drop backtick-quoted node name: {joined}"
        );
        assert!(
            !joined.contains("180000"),
            "should drop the templated duration: {joined}"
        );
    }

    #[test]
    fn distinctive_substrings_passes_through_fully_static_message() {
        let result =
            distinctive_substrings("automation run blocked by upstream node outcome");
        assert_eq!(
            result.first().map(String::as_str),
            Some("automation run blocked by upstream node outcome")
        );
    }

    #[test]
    fn distinctive_substrings_strips_uuid_like_tokens() {
        let result =
            distinctive_substrings("draft 9ee33834-bf6d-4f86-acb3-3cd41d9cef19 failed to publish");
        let joined = result.join(" | ");
        assert!(joined.contains("failed to publish"), "got: {joined}");
        assert!(
            !joined.contains("9ee33834"),
            "should strip uuid-like token: {joined}"
        );
    }

    #[test]
    fn distinctive_substrings_strips_durations_and_paths() {
        let result = distinctive_substrings(
            "no provider activity for at least 300s on /tmp/run-1/state",
        );
        let joined = result.join(" | ");
        assert!(joined.contains("no provider activity for at least"), "got: {joined}");
        assert!(!joined.contains("300"), "should strip number: {joined}");
        assert!(!joined.contains("/tmp"), "should drop path: {joined}");
    }

    #[test]
    fn distinctive_substrings_returns_empty_for_trivial_input() {
        assert!(distinctive_substrings("").is_empty());
        assert!(distinctive_substrings("ok").is_empty());
        assert!(distinctive_substrings("`x` 123").is_empty());
    }

    #[test]
    fn distinctive_substrings_caps_at_max() {
        let input = "alpha bravo charlie delta: echo foxtrot golf hotel; india juliet kilo lima, mike november oscar papa, quebec romeo sierra tango";
        let result = distinctive_substrings(input);
        assert!(result.len() <= MAX_SUBSTRINGS);
    }

    #[test]
    fn render_provenance_section_returns_none_for_empty_hits() {
        assert!(render_provenance_section(&[]).is_none());
    }

    #[test]
    fn render_provenance_section_includes_path_line_and_snippet() {
        let hits = vec![ProvenanceHit {
            path: "crates/foo/src/bar.rs".to_string(),
            line: 42,
            snippet: "let x = 1;\nlet y = 2;\nlet z = 3;".to_string(),
        }];
        let rendered = render_provenance_section(&hits).expect("section");
        assert!(rendered.contains("Error provenance"));
        assert!(rendered.contains("crates/foo/src/bar.rs:42"));
        assert!(rendered.contains("let y = 2;"));
    }

    #[test]
    fn render_provenance_section_caps_total_size() {
        let big_snippet = "x".repeat(3_500);
        let hits = vec![
            ProvenanceHit {
                path: "a.rs".to_string(),
                line: 1,
                snippet: big_snippet.clone(),
            },
            ProvenanceHit {
                path: "b.rs".to_string(),
                line: 1,
                snippet: "small".to_string(),
            },
        ];
        let rendered = render_provenance_section(&hits).expect("section");
        // Second hit should be truncated out by the size cap.
        assert!(!rendered.contains("b.rs"));
    }

    #[test]
    fn parse_git_grep_output_extracts_path_line_body() {
        let stdout = "\
src/lib.rs:11:    bail!(\"automation run blocked by upstream node outcome\");
crates/foo/bar.rs:42:fn x() {}
";
        let hits = parse_git_grep_output(stdout);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "src/lib.rs");
        assert_eq!(hits[0].line, 11);
        assert!(hits[0].snippet.contains("blocked by upstream"));
        assert_eq!(hits[1].path, "crates/foo/bar.rs");
        assert_eq!(hits[1].line, 42);
    }

    #[test]
    fn parse_git_grep_output_handles_paths_with_dashes() {
        // `node_modules/some-package/file.js` contains a dash; legacy
        // parsers that found the first `:` or `-` would misidentify
        // the path boundary. The current split_grep_line scans for
        // a `:digits:` triple.
        let stdout = "node_modules/some-package/file.js:7:throw new Error('boom');\n";
        let hits = parse_git_grep_output(stdout);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "node_modules/some-package/file.js");
        assert_eq!(hits[0].line, 7);
        assert!(hits[0].snippet.contains("throw new Error"));
    }

    #[test]
    fn parse_git_grep_output_truncates_long_lines() {
        let body = "x".repeat(1_000);
        let stdout = format!("file.rs:1:{body}\n");
        let hits = parse_git_grep_output(&stdout);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.len() <= MATCH_LINE_TRUNCATE + 4);
        assert!(hits[0].snippet.ends_with('…'));
    }

    #[test]
    fn parse_git_grep_output_does_not_panic_on_multibyte_boundary() {
        // Construct a body that is just over MATCH_LINE_TRUNCATE bytes
        // and contains a 3-byte char straddling the boundary so naive
        // byte-slicing would panic.
        let mut body = "x".repeat(MATCH_LINE_TRUNCATE - 1);
        body.push_str("漢字漢字漢字");
        let stdout = format!("file.rs:1:{body}\n");
        let hits = parse_git_grep_output(&stdout);
        assert_eq!(hits.len(), 1);
        // Snippet must stay valid UTF-8 and contain at most one
        // truncation marker.
        let _ = hits[0].snippet.chars().count();
        assert!(hits[0].snippet.ends_with('…'));
    }

    #[test]
    fn truncate_on_char_boundary_passes_through_short_input() {
        assert_eq!(truncate_on_char_boundary("hello", 240), "hello");
    }

    #[test]
    fn truncate_on_char_boundary_steps_back_for_multibyte() {
        let s = format!("{}漢", "x".repeat(238));
        // 238 bytes of 'x' + 3 bytes of '漢' = 241 bytes, > 240.
        // Naive slice at byte 240 would split the 3-byte char.
        let out = truncate_on_char_boundary(&s, 240);
        assert!(out.ends_with('…'));
        assert!(out.is_char_boundary(out.len() - '…'.len_utf8()));
    }

    #[test]
    fn parse_git_grep_output_skips_malformed_lines() {
        let stdout = "no colon here at all\nstill nothing\n";
        let hits = parse_git_grep_output(stdout);
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn locate_error_provenance_finds_known_string_in_temp_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let init = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .arg("init")
            .arg("-q")
            .output();
        if init.is_err() {
            // git not available — skip.
            return;
        }
        // git config user identity so commit succeeds in CI.
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["config", "user.email", "test@example.com"])
            .output();
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["config", "user.name", "test"])
            .output();
        std::fs::write(
            root.join("source.rs"),
            "fn main() {\n    panic!(\"the oracle has spoken from the void\");\n}\n",
        )
        .expect("write source");
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["add", "."])
            .output();
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["commit", "-q", "-m", "init"])
            .output();
        let hits = locate_error_provenance(
            root,
            "the oracle has spoken from the void",
        )
        .await;
        assert!(
            hits.iter().any(|h| h.path == "source.rs" && h.line == 2),
            "expected hit at source.rs:2, got: {hits:?}"
        );
    }

    #[tokio::test]
    async fn locate_error_provenance_returns_empty_for_nonsense() {
        let dir = tempfile::tempdir().expect("tempdir");
        let hits = locate_error_provenance(dir.path(), "").await;
        assert!(hits.is_empty());
    }
}
