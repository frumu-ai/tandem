# AI Agent Prompt: Implement MCP + Plugins Support (Config-First, OpenCode-Native)

This file is an "execution prompt" version of the plan. It should remain consistent with:

- `docs/MCP_PLUGINS_IMPLEMENTATION.md` (plan of record)

## Objective

Add **MCP Servers** + **Plugins** support to Tandem by managing **OpenCode configuration**, not by implementing MCP or plugin runtime in Tandem.

Deliver:

1. A new top-level **Extensions** view with tabs: Skills / Plugins / Integrations (MCP)
2. Safe, atomic, round-trip config updates for OpenCode config (global + project scopes)
3. Best-effort MCP status UX:
   - HTTP servers: protocol-correct MCP `initialize` probe (POST), validate JSON-RPC response
   - stdio servers: "Not tested" (until OpenCode exposes a definitive health/status API)

## Key Repo Reality (Do Not Miss This)

Tandem updates OpenCode config at sidecar start to keep Ollama models fresh.

- Config path is resolved by `src-tauri/src/opencode_config.rs` (default: `dirs::config_dir()/opencode/config.json`)
- The sidecar is spawned with `OPENCODE_CONFIG` set so OpenCode reliably loads that file.

Do not overwrite the entire config; only merge/update `provider.ollama.models` and preserve unknown fields so MCP/plugin settings survive restarts.

## UX Requirements (Match Existing Theme)

Extensions must look like it shipped with Tandem:

- Navigation:
  - Add Extensions as a top-level view using the same icon-sidebar pattern in `src/App.tsx`.
  - Active state must match existing buttons (`bg-primary/20 text-primary`).
- Tabs:
  - Implement tab UI matching the Sessions/Files switcher in `src/App.tsx`:
    - `flex border-b border-border`, active tab with `border-b-2 border-primary text-primary`.
- Layout:
  - Use the same header + Cards pattern as Settings (`src/components/settings/Settings.tsx`).
  - Build form controls out of existing `Card`, `Button`, and `Input` components.

## Implementation Phases (Follow This Order)

### Phase 1: Config Paths + Config Manager (Backend)

Add `src-tauri/src/opencode_config.rs` with:

- `read_config(scope) -> serde_json::Value` (preserve unknown fields)
- `write_config(scope, value)` atomic (temp + rename)
- `update_config(scope, mutator_fn)`
- `get_config_path(scope)` using a path resolver that supports both:
  - `dirs::config_dir()/opencode/config.json` (current behavior)
  - `~/.config/opencode/...` (used elsewhere in repo)

Then refactor `src-tauri/src/sidecar.rs` to update/merge the Ollama provider into existing config instead of overwriting.

### Phase 2: MCP + Plugin Commands (Backend)

Add Tauri commands (module split optional, but keep cohesive):
Plugins:

- `opencode_list_plugins(scope)`
- `opencode_add_plugin(scope, name)`
- `opencode_remove_plugin(scope, name)`

MCP:

- `opencode_list_mcp_servers(scope)`
- `opencode_add_mcp_server(scope, name, config)`
- `opencode_remove_mcp_server(scope, name)`
- `opencode_test_mcp_connection(scope, name)`:
  - HTTP: protocol-correct MCP `initialize` (JSON-RPC POST) with short timeout
  - stdio: return "not_supported" / "not_tested"

Secrets:

- Do not store API keys in JSON config. Use Tandem keystore and reference via env var conventions (or whatever OpenCode supports).

### Phase 3: Extensions View (Frontend)

Add a new view and nav entry and implement tab shells:

- Skills tab: migrate existing Skills UI (do not regress behavior)
- Plugins tab: scope toggle + list/add/remove
- Integrations tab: scope toggle + list/add/remove + test (HTTP only)

## Manual Verification

1. Start sidecar, confirm config merge does not wipe unrelated fields.
2. Extensions -> Plugins: add/remove global and project entries; restart app; persists.
3. Extensions -> Integrations: add HTTP server; test connection shows success/failure; restart app; persists.
4. Invalid JSON: UI surfaces error; app does not crash.
