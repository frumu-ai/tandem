//! Write-target derivation (Invariant 1 of `docs/SPINE.md`).
//!
//! For any tool call, the set of paths it writes to is a pure function of
//! `(tool name, args)`. Read-only tools always return `∅`.
//!
//! All inputs flow through a single [`ToolKind`] classification. Both
//! [`paths`] and [`requires_concrete`] match exhaustively over `ToolKind`,
//! so adding a new variant fails to compile until both functions answer
//! for it. That is the spine: a new tool cannot silently slip past write
//! gating the way it could when classification was scattered across
//! `tool_execution.rs` and `prompt_helpers.rs`.
//!
//! See `docs/SPINE.md` for the full plan and `tests` below for the
//! property test that guards read-only invariance.
//!
//! Note: MCP write effects are intentionally not classified here; MCP
//! has its own readiness/write gate (Invariant 2, `mcp_ready.rs`). MCP
//! tools route through `ToolKind::Mcp` and contribute no session-level
//! write targets.

use serde_json::Value;

use super::normalize_tool_name;
use super::tool_execution::string_fields;

/// Write-relevant classification of a tool name. Every variant must be
/// answered for in `paths` and `requires_concrete`; the compiler enforces
/// this via exhaustive `match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolKind {
    // Read-only — `paths` returns ∅, `requires_concrete` is false.
    Read,
    Glob,
    Grep,
    Search,
    Codesearch,
    Ls,
    Lsp,
    WebSearch,
    WebFetch,

    // Workspace mutations — `paths` returns the declared targets.
    Write,
    Edit,
    Delete,
    ApplyPatch,

    // Conditional mutation — depends on shell command shape.
    Shell,

    // Out-of-band — gated separately by `mcp_ready` (Invariant 2).
    Mcp,

    // Catch-all for tools whose write effects we do not classify here.
    Other,
}

pub(crate) fn classify(tool: &str) -> ToolKind {
    let normalized = normalize_tool_name(tool);
    if normalized == "mcp_list" || normalized.starts_with("mcp.") {
        return ToolKind::Mcp;
    }
    match normalized.as_str() {
        "read" => ToolKind::Read,
        "glob" => ToolKind::Glob,
        "grep" => ToolKind::Grep,
        "search" => ToolKind::Search,
        "codesearch" => ToolKind::Codesearch,
        "list" | "ls" => ToolKind::Ls,
        "lsp" => ToolKind::Lsp,
        "websearch" => ToolKind::WebSearch,
        "webfetch" | "webfetch_html" => ToolKind::WebFetch,
        "write" => ToolKind::Write,
        "edit" => ToolKind::Edit,
        "delete" | "delete_file" => ToolKind::Delete,
        "apply_patch" => ToolKind::ApplyPatch,
        "bash" | "shell" => ToolKind::Shell,
        _ => ToolKind::Other,
    }
}

/// Paths this tool call writes to, sorted and deduplicated. Empty for
/// read-only and unclassified tools.
pub(crate) fn paths(tool: &str, args: &Value) -> Vec<String> {
    let mut out = match classify(tool) {
        ToolKind::Read
        | ToolKind::Glob
        | ToolKind::Grep
        | ToolKind::Search
        | ToolKind::Codesearch
        | ToolKind::Ls
        | ToolKind::Lsp
        | ToolKind::WebSearch
        | ToolKind::WebFetch
        | ToolKind::Mcp
        | ToolKind::Other => Vec::new(),

        ToolKind::Write | ToolKind::Edit | ToolKind::Delete => string_fields(
            args,
            &[
                "path",
                "file_path",
                "filePath",
                "filepath",
                "target_path",
                "output_path",
                "file",
            ],
        ),
        ToolKind::ApplyPatch => args
            .get("patchText")
            .or_else(|| args.get("patch"))
            .and_then(Value::as_str)
            .map(extract_apply_patch_paths)
            .unwrap_or_default(),
        ToolKind::Shell => extract_shell_redirect_targets(
            args.get("command")
                .or_else(|| args.get("cmd"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
    };
    out.sort();
    out.dedup();
    out
}

/// Whether the tool requires a declared, concrete write target before it
/// is allowed to run under a non-`RepoEdit` session policy.
pub(crate) fn requires_concrete(tool: &str, args: &Value) -> bool {
    match classify(tool) {
        ToolKind::Read
        | ToolKind::Glob
        | ToolKind::Grep
        | ToolKind::Search
        | ToolKind::Codesearch
        | ToolKind::Ls
        | ToolKind::Lsp
        | ToolKind::WebSearch
        | ToolKind::WebFetch
        | ToolKind::Mcp
        | ToolKind::Other => false,

        ToolKind::Write | ToolKind::Edit | ToolKind::Delete | ToolKind::ApplyPatch => true,

        ToolKind::Shell => args
            .get("command")
            .or_else(|| args.get("cmd"))
            .and_then(Value::as_str)
            .is_some_and(shell_command_appears_mutating),
    }
}

fn extract_apply_patch_paths(patch: &str) -> Vec<String> {
    use std::collections::HashSet;
    let mut paths = HashSet::new();
    for line in patch.lines() {
        let trimmed = line.trim();
        let marker = trimmed
            .strip_prefix("*** Add File: ")
            .or_else(|| trimmed.strip_prefix("*** Update File: "))
            .or_else(|| trimmed.strip_prefix("*** Delete File: "));
        if let Some(path) = marker.map(str::trim).filter(|value| !value.is_empty()) {
            paths.insert(path.to_string());
        }
    }
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    paths
}

fn extract_shell_redirect_targets(command: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for part in command.split(">>").flat_map(|value| value.split('>')) {
        let candidate = part.trim().split_whitespace().next().unwrap_or("").trim();
        if candidate.starts_with('/')
            || candidate.starts_with("./")
            || candidate.starts_with("../")
            || candidate.starts_with("~/")
            || candidate.starts_with(".tandem/")
        {
            targets.push(candidate.to_string());
        }
    }
    targets.sort();
    targets.dedup();
    targets
}

fn shell_command_appears_mutating(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    lowered.contains(" >")
        || lowered.contains(">>")
        || lowered.contains(" tee ")
        || lowered.starts_with("tee ")
        || lowered.contains(" sed -i")
        || lowered.starts_with("sed -i")
        || lowered.contains(" perl -pi")
        || lowered.starts_with("perl -pi")
        || lowered.contains(" rm ")
        || lowered.starts_with("rm ")
        || lowered.contains(" mv ")
        || lowered.starts_with("mv ")
        || lowered.contains(" cp ")
        || lowered.starts_with("cp ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Property test for Invariant 1: every read-only `ToolKind`
    /// produces no write targets and never requires a concrete write
    /// target, regardless of args. If a future change reclassifies a
    /// read-only tool as a writer, this test fails.
    #[test]
    fn read_only_kinds_never_write() {
        let read_only_tools = [
            "read",
            "glob",
            "grep",
            "search",
            "codesearch",
            "list",
            "ls",
            "lsp",
            "websearch",
            "webfetch",
            "webfetch_html",
        ];
        let arg_shapes = [
            json!({}),
            json!({"path": "src/foo.rs"}),
            json!({"file_path": "./bar"}),
            json!({"pattern": "**/*.ts"}),
            json!({"command": "rm -rf /"}),
            json!({"patchText": "*** Update File: x\n"}),
        ];
        for tool in read_only_tools {
            assert!(
                matches!(
                    classify(tool),
                    ToolKind::Read
                        | ToolKind::Glob
                        | ToolKind::Grep
                        | ToolKind::Search
                        | ToolKind::Codesearch
                        | ToolKind::Ls
                        | ToolKind::Lsp
                        | ToolKind::WebSearch
                        | ToolKind::WebFetch
                ),
                "tool {tool} should classify as a read-only ToolKind"
            );
            for args in &arg_shapes {
                assert!(
                    paths(tool, args).is_empty(),
                    "read-only tool {tool} produced write paths for {args}"
                );
                assert!(
                    !requires_concrete(tool, args),
                    "read-only tool {tool} required concrete target for {args}"
                );
            }
        }
    }

    #[test]
    fn write_extracts_path_field() {
        assert_eq!(
            paths("write", &json!({"path": "artifacts/report.md"})),
            vec!["artifacts/report.md".to_string()]
        );
    }

    #[test]
    fn write_extracts_alternative_field_aliases() {
        assert_eq!(
            paths("edit", &json!({"file_path": "src/lib.rs"})),
            vec!["src/lib.rs".to_string()]
        );
        assert_eq!(
            paths("delete", &json!({"target_path": "tmp/old.txt"})),
            vec!["tmp/old.txt".to_string()]
        );
    }

    #[test]
    fn apply_patch_extracts_files_from_patch_text() {
        let args = json!({
            "patchText": "*** Begin Patch\n*** Update File: packages/app/src/main.ts\n@@\n old\n*** End Patch\n"
        });
        assert_eq!(
            paths("apply_patch", &args),
            vec!["packages/app/src/main.ts".to_string()]
        );
    }

    #[test]
    fn shell_extracts_redirect_targets() {
        assert_eq!(
            paths("bash", &json!({"command": "echo hi > ./out.txt"})),
            vec!["./out.txt".to_string()]
        );
    }

    #[test]
    fn mcp_tools_route_to_their_own_gate() {
        // Invariant 2 (`mcp_ready.rs`) gates MCP separately. The
        // session-level write classifier deliberately treats MCP as
        // opaque so `paths` and `requires_concrete` agree on `Mcp` =>
        // (∅, false).
        assert_eq!(classify("mcp.fs.write"), ToolKind::Mcp);
        assert_eq!(classify("mcp_list"), ToolKind::Mcp);
        assert!(paths("mcp.fs.write", &json!({"path": "/tmp/x"})).is_empty());
        assert!(!requires_concrete(
            "mcp.fs.write",
            &json!({"path": "/tmp/x"})
        ));
    }

    #[test]
    fn requires_concrete_for_workspace_writes() {
        assert!(requires_concrete("write", &json!({})));
        assert!(requires_concrete("edit", &json!({})));
        assert!(requires_concrete("delete", &json!({})));
        assert!(requires_concrete("apply_patch", &json!({})));
    }

    #[test]
    fn requires_concrete_for_mutating_shell_only() {
        assert!(requires_concrete(
            "bash",
            &json!({"command": "cat <<'EOF' > packages/app/src/main.ts\nbroken\nEOF"})
        ));
        assert!(!requires_concrete(
            "bash",
            &json!({"command": "rg \"needle\" packages/app/src"})
        ));
    }
}
