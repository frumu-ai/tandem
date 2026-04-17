use tandem_types::{ContextMode, HostOs, HostRuntimeContext, PathStyle, ShellFamily};

use crate::tool_router::max_tools_per_call_expanded;

pub(super) fn format_context_mode(requested: &ContextMode, auto_compact: bool) -> &'static str {
    match requested {
        ContextMode::Full => "full",
        ContextMode::Compact => "compact",
        ContextMode::Auto => {
            if auto_compact {
                "auto_compact"
            } else {
                "auto_standard"
            }
        }
    }
}

pub(super) fn tandem_runtime_system_prompt(
    host: &HostRuntimeContext,
    mcp_server_names: &[String],
) -> String {
    let mut sections = Vec::new();
    if os_aware_prompts_enabled() {
        sections.push(format!(
            "[Execution Environment]\nHost OS: {}\nShell: {}\nPath style: {}\nArchitecture: {}",
            host_os_label(host.os),
            shell_family_label(host.shell_family),
            path_style_label(host.path_style),
            host.arch
        ));
    }
    sections.push(
        "You are operating inside Tandem (Desktop/TUI) as an engine-backed coding assistant.
Use tool calls to inspect and modify the workspace when needed instead of asking the user
to manually run basic discovery steps. Permission prompts may occur for some tools; if
a tool is denied or blocked, explain what was blocked and suggest a concrete next step."
            .to_string(),
    );
    sections.push(
        "For greetings or simple conversational messages (for example: hi, hello, thanks),
respond directly without calling tools."
            .to_string(),
    );
    if host.os == HostOs::Windows {
        sections.push(
            "Windows guidance: prefer cross-platform tools (`glob`, `grep`, `read`, `write`, `edit`) and PowerShell-native commands.
Avoid Unix-only shell syntax (`ls -la`, `find ... -type f`, `cat` pipelines) unless translated.
If a shell command fails with a path/shell mismatch, immediately switch to cross-platform tools (`read`, `glob`, `grep`)."
                .to_string(),
        );
    } else {
        sections.push(
            "POSIX guidance: standard shell commands are available.
Use cross-platform tools (`glob`, `grep`, `read`) when they are simpler and safer for codebase exploration."
                .to_string(),
        );
    }
    if !mcp_server_names.is_empty() {
        let cap = mcp_catalog_max_servers();
        let mut listed = mcp_server_names
            .iter()
            .take(cap)
            .cloned()
            .collect::<Vec<_>>();
        listed.sort();
        let mut catalog = listed
            .iter()
            .map(|name| format!("- {name}"))
            .collect::<Vec<_>>();
        if mcp_server_names.len() > cap {
            catalog.push(format!("- (+{} more)", mcp_server_names.len() - cap));
        }
        sections.push(format!(
            "[Connected Integrations]\nThe following external integrations are currently connected and available:\n{}",
            catalog.join("\n")
        ));
    }
    sections.join("\n\n")
}

pub(super) fn os_aware_prompts_enabled() -> bool {
    std::env::var("TANDEM_OS_AWARE_PROMPTS")
        .ok()
        .map(|v| {
            let normalized = v.trim().to_ascii_lowercase();
            !(normalized == "0" || normalized == "false" || normalized == "off")
        })
        .unwrap_or(true)
}

pub(super) fn semantic_tool_retrieval_enabled() -> bool {
    std::env::var("TANDEM_SEMANTIC_TOOL_RETRIEVAL")
        .ok()
        .map(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true)
}

pub(super) fn semantic_tool_retrieval_k() -> usize {
    std::env::var("TANDEM_SEMANTIC_TOOL_RETRIEVAL_K")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(max_tools_per_call_expanded)
}

pub(super) fn mcp_catalog_in_system_prompt_enabled() -> bool {
    std::env::var("TANDEM_MCP_CATALOG_IN_SYSTEM_PROMPT")
        .ok()
        .map(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true)
}

pub(super) fn mcp_catalog_max_servers() -> usize {
    std::env::var("TANDEM_MCP_CATALOG_MAX_SERVERS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(20)
}

pub(super) fn host_os_label(os: HostOs) -> &'static str {
    match os {
        HostOs::Windows => "windows",
        HostOs::Linux => "linux",
        HostOs::Macos => "macos",
    }
}

pub(super) fn shell_family_label(shell: ShellFamily) -> &'static str {
    match shell {
        ShellFamily::Powershell => "powershell",
        ShellFamily::Posix => "posix",
    }
}

pub(super) fn path_style_label(path_style: PathStyle) -> &'static str {
    match path_style {
        PathStyle::Windows => "windows",
        PathStyle::Posix => "posix",
    }
}
