# MCP + Plugins Support (Plan Of Record)

This document is the single, canonical plan for adding **OpenCode MCP servers** and **OpenCode plugins** support to Tandem.

It merges and supersedes:

- `docs/tandem_mcp_plugins_agent_prompt.md`
- the prior contents of this file

## Summary (Best Approach)

Tandem should **not** implement an MCP client nor a plugin runtime.

Instead, Tandem should:

- expose **configuration** (global + per-project/workspace) via a first-class UI (Extensions)
- add a small Rust **config manager** that can safely read/update OpenCode config files
- add best-effort status UX:
  - for HTTP MCP servers: protocol-correct MCP `initialize` probe (POST), validate JSON-RPC response
  - for stdio MCP servers: show "Not tested" unless/until OpenCode exposes status via its API

Crucially: Tandem updates OpenCode config on sidecar start to keep Ollama models fresh. That logic must **merge** (not overwrite) or else MCP/plugin settings will be lost on every start.

## Current Repo Reality (Confirmed In Code)

- Tandem runs OpenCode as a **sidecar** (`src-tauri/src/sidecar.rs`) and sets `OPENCODE_DIR` to the active workspace.
- On sidecar start, Tandem updates an OpenCode JSON config file and merges dynamic Ollama models:
  - path is resolved by `src-tauri/src/opencode_config.rs` (default is `dirs::config_dir()/opencode/config.json`)
  - OpenCode is forced to load this file by setting `OPENCODE_CONFIG` when spawning the sidecar (`src-tauri/src/sidecar.rs`)
  - only `provider.ollama.models` (and required Ollama provider metadata) is updated; unknown fields are preserved
- Skills are already treated as OpenCode-compatible:
  - project skills: `<workspace>/.opencode/skill/`
  - global skills: the repo currently uses multiple notions of "global" (see `src-tauri/src/skills.rs`), so we must unify this as part of MCP/plugins work.

## UX Direction (Match Tandem Theme)

We want MCP/plugins management to feel native to Tandem. The quickest way to achieve that is to reuse the existing view + tab patterns already present in the app:

- Top-level navigation in Tandem is the **icon sidebar** in `src/App.tsx` (Chat, Packs, Settings, etc). Extensions should be a new top-level `view` alongside those, with the same active state styling (`bg-primary/20 text-primary` vs muted hover).
- Tabs inside a view should match the existing **tab switcher** styling used in the Chat sidebar (Sessions/Files) in `src/App.tsx`:
  - container: `flex border-b border-border`
  - each tab: `flex-1 px-4 py-3 text-sm font-medium ...`
  - active: `border-b-2 border-primary text-primary`
  - inactive: `text-text-muted hover:text-text hover:bg-surface-elevated`
- Page layout should match Settings:
  - header block with icon in a `rounded-xl bg-primary/10` square and text using `text-2xl font-bold text-text` + `text-text-muted` (see `src/components/settings/Settings.tsx`)
  - content areas should use existing `Card` components (`src/components/ui/Card.tsx`) and existing `Button`/`Input` components.

## Goals / Non-Goals

Goals:

- Add a top-level **Extensions** view with tabs:
  - Skills (migrated out of Settings)
  - Plugins
  - Integrations (MCP)
- Support Global vs Project scope for plugins and MCP server configuration
- Preserve unknown config fields (round-trip safe)
- Atomic writes + clear UX errors for invalid JSON / permissions

Non-goals (initial):

- Full MCP handshake, tool schema introspection, or message routing in Tandem
- Installing plugins via npm/pnpm inside Tandem (unless OpenCode already handles it purely from config)
- OAuth flows (only placeholders in UI)

## Key Design Decision: Config Files And Scope

The most important work is choosing config file locations and ensuring we never clobber user config.

### OpenCode "Global" Location

The repo currently mixes these patterns:

- `dirs::config_dir()/opencode/...` (used by `src-tauri/src/sidecar.rs` and skill discovery)
- `dirs::home_dir()/.config/opencode/...` (used by `src-tauri/src/skills.rs` sync helpers)

Best approach: implement an `opencode_paths` helper and **support both**, with a deterministic preference order:

1. If one location already exists on disk, treat it as canonical.
2. If neither exists, pick one canonical default and use it consistently (and document it).

This avoids breaking existing installs while we converge on the correct OpenCode expectation on Windows.

### OpenCode "Project/Workspace" Location

Tandem should treat the workspace config as:

- `<workspace>/opencode.json` (or `opencode.jsonc` if it already exists)

This matches OpenCode's documented config search behavior and keeps workspace config user-visible in the project root.

## Config Manager (Rust) Requirements

Add a shared module, e.g. `src-tauri/src/opencode_config.rs`, that supports:

- `read_config(scope) -> serde_json::Value`:
  - returns an object (or `{}`) even if missing
  - preserves unknown fields
- `write_config(scope, value)`:
  - atomic write (temp file + rename)
  - creates parent directories if needed
  - never crashes the app on invalid JSON; bubble errors to UI
- `update_config(scope, mutator_fn)`:
  - load -> mutate -> validate minimally -> write
- `get_config_path(scope)`:
  - uses the `opencode_paths` decision above

### Critical Refactor: Stop Overwriting Config On Sidecar Start

Previously, `src-tauri/src/sidecar.rs` wrote a whole config file each launch (to inject/update Ollama models).

This must be implemented as:

- read existing config (if any)
- merge or update only `provider.ollama.models` (and any required provider metadata)
- write the updated config back atomically

Without this, any MCP/plugin settings managed by Tandem will be erased on every sidecar start.

## Schema Surface Area (What We Manage)

We intentionally keep schema scope small and preserve everything else.

### Plugins

OpenCode expects:

- `plugin`: an array of plugin identifier strings

Example:

```json
{
  "plugin": ["@acme/opencode-plugin-foo", "@acme/opencode-plugin-bar"]
}
```

Tandem manages:

- add/remove plugin identifier strings
- optional "installed" status (best-effort) if there is a well-known plugin directory to check

### MCP Servers

OpenCode expects:

- `mcp`: an object mapping server name -> server config

Remote (HTTP) servers:

```json
{
  "mcp": {
    "my-remote": {
      "type": "remote",
      "url": "https://example.com/mcp",
      "headers": ["Authorization: Bearer $TOKEN"],
      "enabled": true
    }
  }
}
```

Local (stdio) servers:

```json
{
  "mcp": {
    "my-local": {
      "type": "local",
      "command": ["npx", "@modelcontextprotocol/server-git"],
      "environment": { "TOKEN": "$TOKEN" },
      "enabled": true
    }
  }
}
```

Tandem manages:

- HTTP servers:
  - `url`
  - optional `headers` (array of strings, one per header line)
- stdio servers:
  - `command` (string array; first token is the command, remaining are args)
  - optional `environment` (string map)

Secrets:

- If an MCP server needs an API key, store it in Tandem's keystore and reference it via environment variables (or whatever OpenCode supports), not in JSON.

## UI Plan (Extensions View)

### Phase 0: Extensions Shell

- Add `Extensions` as a new top-level nav item
- Create `Extensions` view with tabs: Skills / Plugins / Integrations
- Move existing Skills section out of `src/components/settings/Settings.tsx` into the Skills tab

### Phase 1: Skills Tab

- Reuse the existing skills UI component(s) and backend commands
- Convert from collapsible-in-settings to full-page tab layout

### Phase 2: Plugins Tab

UI:

- list configured plugins for chosen scope (Global/Project)
- add plugin (string input)
- remove plugin

Backend (Tauri commands, names TBD):

- `opencode_list_plugins(scope)`
- `opencode_add_plugin(scope, name)`
- `opencode_remove_plugin(scope, name)`

### Phase 3: Integrations (MCP) Tab

UI:

- list configured servers with status
- add popular presets (optional)
- add remote HTTP server (name + url)
- add local stdio server (name + command + args)
- remove server
- test connection (HTTP only, best-effort)

Backend (Tauri commands, names TBD):

- `opencode_list_mcp_servers(scope)`
- `opencode_add_mcp_server(scope, name, config)`
- `opencode_remove_mcp_server(scope, name)`
- `opencode_test_mcp_connection(scope, name)`:
  - HTTP: protocol-correct MCP `initialize` (JSON-RPC POST) with a short timeout
  - stdio: return "not_supported" unless/until OpenCode exposes a real health/status API

Troubleshooting (HTTP MCP):

- 405: reachable endpoint, but wrong method or wrong transport for that URL
- 406: some servers require `Accept: application/json, text/event-stream` for MCP Streamable HTTP
- 410: endpoint deprecated or removed (DeepWiki deprecated `/sse`; use `/mcp`)

### UI Details (Concrete Components/Styles)

To avoid UI drift and keep this shippable:

- Use `Card` sections per tab for "Configured" lists and "Add new".
- Use the existing `Input` for name/url/command fields and `Button` for actions.
- Scope selector:
  - simplest: a two-button segmented control built from plain buttons using the same classes as the Sessions/Files tabs in `src/App.tsx`.
  - persist selection in local state only (do not store in config).
- Status badges:
  - reuse existing text colors (`text-success`, `text-error`, `text-text-subtle`) and soft backgrounds (`bg-success/10`, `bg-error/10`, etc).
- Empty states:
  - match Settings empty state conventions: centered `bg-surface-elevated` panel with a small icon and `text-text-muted`.

## Implementation Order (PR Sequence)

Keep PRs small and reviewable:

1. Config paths + config manager:
   - `opencode_paths` helper
   - `opencode_config` module with atomic update
   - refactor sidecar startup config write to merge, not overwrite
2. Backend commands:
   - plugins commands (read/write config)
   - MCP server commands (read/write config + HTTP probe)
3. Frontend navigation:
   - Extensions entry in sidebar + routing
4. Skills migration:
   - move Skills UI from Settings to Extensions
5. Plugins UI:
   - scope toggle + list/add/remove
6. MCP UI:
   - scope toggle + list/add/remove + test
7. Docs + polish:
   - troubleshooting + known limitations

## Manual Verification Checklist

- Skills still work, just moved to Extensions
- Add plugin (global): persists across restart
- Add plugin (project): persists across restart and is scoped correctly
- Add MCP HTTP server: persists; "Test connection" reports pass/fail
- Add MCP stdio server: persists; status shows "Not tested"
- Invalid JSON in config: UI shows actionable error; no crash
- Sidecar start no longer wipes MCP/plugins settings

## Troubleshooting Notes

- If MCP/plugins "disappear" after starting the sidecar, it indicates the sidecar config merge behavior is still overwriting config.
- If settings aren't visible in OpenCode, verify Tandem is writing to the same config location OpenCode actually reads on that platform. The `opencode_paths` logic should make this observable and debuggable.
