# Implementation Plan: MCP & Plugins Support for Tandem

**Date:** 2026-01-22
**Status:** PROPOSAL
**Priority:** High

---

## Executive Summary

This document outlines the implementation plan for adding **MCP (Model Context Protocol)** and **Plugin** management capabilities to Tandem.

**Key Discovery:** OpenCode already supports both MCP servers and Plugins natively via `opencode.json` configuration. Tandem's role is to provide a **user-friendly UI** for managing these capabilities.

**Architecture Simplification:** To avoid overloading the Settings panel, we will introduce a new **"Extensions"** top-level view that houses Skills, Plugins, and MCP Integrations.

---

## Current Problem

The existing `Settings.tsx` is already ~1000 lines with:

- Updates
- Projects (collapsible)
- Appearance
- Skills (collapsible)
- LLM Providers

Adding Plugins and MCP here would make it unwieldy.

---

## Proposed Architecture

### Option A: Dedicated "Extensions" View (Recommended)

Create a new top-level view accessible from the sidebar, separate from Settings.

```
┌─────────────────────────────────────────────────────────────────────┐
│  Sidebar                │  Main Content                             │
│  ──────────────────     │  ─────────────────────────────────────── │
│  [Chat]                 │                                           │
│  [Files]                │  ┌─────────────────────────────────────┐ │
│  [Extensions] ← NEW     │  │  [Skills] [Plugins] [Integrations]  │ │
│  [Settings]             │  │  ─────────────────────────────────  │ │
│                         │  │                                     │ │
│                         │  │  (Tab content here)                 │ │
│                         │  │                                     │ │
│                         │  └─────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

**Benefits:**

- Keeps Settings focused on core config (Projects, Providers, Appearance)
- Gives extension-related features room to grow
- Cleaner mental model: "Settings" = config, "Extensions" = capabilities

### Option B: Tabbed Settings Panel

Keep everything in Settings but add horizontal tabs.

```
┌─────────────────────────────────────────────────────────────────────┐
│  Settings                                                           │
│  ─────────────────────────────────────────────────────────────────  │
│  [General] [Extensions] [Providers]                                 │
│  ─────────────────────────────────────────────────────────────────  │
│                                                                     │
│  (Tab content here)                                                 │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

**Benefits:**

- No sidebar changes needed
- Familiar pattern

---

## Recommended: Option A - Extensions View

### New File Structure

```
src/
├── components/
│   ├── extensions/                    [NEW] Extensions view
│   │   ├── Extensions.tsx            Main view with tabs
│   │   ├── SkillsTab.tsx             Skills management (moved from Settings)
│   │   ├── PluginsTab.tsx            Plugin management
│   │   ├── IntegrationsTab.tsx       MCP server management
│   │   └── index.ts
│   ├── settings/
│   │   ├── Settings.tsx              [MODIFY] Remove Skills section
│   │   └── ...
│   └── sidebar/
│       └── Sidebar.tsx               [MODIFY] Add Extensions link
│
src-tauri/
├── src/
│   ├── plugins.rs                    [NEW] Plugin management commands
│   ├── mcp.rs                        [NEW] MCP management commands
│   ├── opencode_config.rs            [NEW] Config file manager
│   └── lib.rs                        [MODIFY] Register new commands
```

---

## Implementation Phases

### Phase 0: Create Extensions View Shell (Foundation)

#### [NEW] `src/components/extensions/Extensions.tsx`

A new top-level view with three tabs.

```tsx
// Extensions.tsx structure
const tabs = ["Skills", "Plugins", "Integrations"];

<div className="h-full overflow-y-auto">
  <TabBar tabs={tabs} activeTab={activeTab} onChange={setActiveTab} />

  {activeTab === "Skills" && <SkillsTab />}
  {activeTab === "Plugins" && <PluginsTab />}
  {activeTab === "Integrations" && <IntegrationsTab />}
</div>;
```

#### [MODIFY] `src/components/sidebar/Sidebar.tsx`

Add an "Extensions" navigation item with a `Puzzle` or `Blocks` icon.

#### [MODIFY] `src/components/settings/Settings.tsx`

Remove the Skills section (it moves to Extensions).

---

### Phase 1: Migrate Skills to Extensions

#### [NEW] `src/components/extensions/SkillsTab.tsx`

Move the existing `SkillsPanel` logic here with full-page layout.

- Keep existing functionality
- Expand to use available space (no longer collapsible)

---

### Phase 2: Plugins Tab

#### [NEW] `src/components/extensions/PluginsTab.tsx`

**Features:**

- List installed plugins (from `.opencode/plugins/` and npm config)
- Install from npm (add to `opencode.json` `plugin` array)
- Create local plugin from template
- Link to OpenCode Ecosystem

**UI:**

```
┌─────────────────────────────────────────────────────────────────────┐
│  Plugins                                                            │
│  ─────────────────────────────────────────────────────────────────  │
│  Plugins extend OpenCode with custom tools, hooks, and workflows.   │
│                                                                     │
│  Installed                                                          │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ 📦 opencode-wakatime                              [Remove]  │   │
│  │    Track coding time in WakaTime                             │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ─────────────────────────────────────────────────────────────────  │
│  Add Plugin                                                         │
│  ┌────────────────────────────────────────────┐ [Install]          │
│  │ npm package name (e.g. opencode-wakatime)  │                    │
│  └────────────────────────────────────────────┘                    │
│                                                                     │
│  [Browse Ecosystem →]                                               │
└─────────────────────────────────────────────────────────────────────┘
```

#### [NEW] `src-tauri/src/plugins.rs`

Rust commands:

- `list_plugins()` - Read `opencode.json` and scan plugin directories
- `add_plugin(name: String, scope: "project" | "global")` - Add to config
- `remove_plugin(name: String)` - Remove from config

---

### Phase 3: Integrations (MCP) Tab

#### [NEW] `src/components/extensions/IntegrationsTab.tsx`

**Features:**

- List configured MCP servers with status
- Add from popular presets (Sentry, Context7, Grep, Notion)
- Add custom local or remote server
- Manage authentication (API keys, OAuth)

**UI:**

```
┌─────────────────────────────────────────────────────────────────────┐
│  Integrations                                                       │
│  ─────────────────────────────────────────────────────────────────  │
│  Connect external tools via Model Context Protocol (MCP).           │
│                                                                     │
│  Connected                                                          │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ 🔌 sentry                                    ● Connected    │   │
│  │    https://mcp.sentry.dev         [Configure] [Remove]      │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ─────────────────────────────────────────────────────────────────  │
│  Add Integration                                                    │
│                                                                     │
│  Popular:                                                           │
│  [+ Sentry]  [+ Context7]  [+ Grep]  [+ GitHub]  [+ Notion]        │
│                                                                     │
│  Custom:                                                            │
│  [+ Local Server]  [+ Remote Server]                                │
└─────────────────────────────────────────────────────────────────────┘
```

#### [NEW] `src-tauri/src/mcp.rs`

Rust commands:

- `list_mcp_servers()` - Parse `opencode.json` mcp configuration
- `add_mcp_server(config: McpServerConfig)` - Add to config
- `remove_mcp_server(name: String)` - Remove from config
- `test_mcp_connection(name: String)` - Check if reachable

---

### Phase 4: Shared Config Manager

#### [NEW] `src-tauri/src/opencode_config.rs`

Unified module for reading/writing `opencode.json`:

- `read_config(scope: "project" | "global")` - Parse config
- `write_config(scope, config)` - Safely update
- `get_config_path(scope)` - Return path

**Config Locations:**

- Global: `~/.config/opencode/opencode.json`
- Project: `<workspace>/opencode.json`

---

## Implementation Order

| Phase | Task                                    | Effort | Dependencies |
| :---- | :-------------------------------------- | :----- | :----------- |
| 0.1   | Create Extensions.tsx shell with tabs   | 2h     | None         |
| 0.2   | Add Extensions to Sidebar               | 1h     | 0.1          |
| 0.3   | Remove Skills from Settings.tsx         | 0.5h   | 0.1          |
| 1.1   | Create SkillsTab.tsx (migrate existing) | 2h     | 0.1          |
| 2.1   | Create opencode_config.rs               | 2h     | None         |
| 2.2   | Create plugins.rs                       | 2h     | 2.1          |
| 2.3   | Create PluginsTab.tsx                   | 3h     | 2.2          |
| 3.1   | Create mcp.rs                           | 3h     | 2.1          |
| 3.2   | Create IntegrationsTab.tsx              | 4h     | 3.1          |
| 3.3   | Add popular MCP presets                 | 2h     | 3.2          |
| 4.1   | End-to-end testing                      | 2h     | All          |
| 4.2   | Documentation and CHANGELOG             | 1h     | All          |

**Total Estimated Effort:** ~24.5 hours

---

## Verification Plan

### Manual Verification

1. Navigate to Extensions → Skills tab → verify existing skills visible
2. Navigate to Extensions → Plugins tab → install `opencode-wakatime` → verify in config
3. Navigate to Extensions → Integrations tab → add Context7 → verify tools available in chat
4. Restart app → verify all settings persist

---

## References

- [OpenCode Plugins Documentation](https://opencode.ai/docs/plugins/)
- [OpenCode MCP Servers Documentation](https://opencode.ai/docs/mcp-servers)
- [OpenCode Config Schema](https://opencode.ai/config.json)
- [Model Context Protocol Specification](https://modelcontextprotocol.io)
