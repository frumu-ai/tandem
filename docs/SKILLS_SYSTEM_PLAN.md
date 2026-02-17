# Skills System Implementation Plan

## Executive Summary

To achieve parity with Opencode and support scalable agent capabilities, we must upgrade `tandem-skills` from a simple file loader to a robust **Discovery & Runtime System**.

**Key Objectives:**

1.  **Universal Discovery:** Scan standard directories (`~/.agents/skills`, `.tandem/skills`, etc.) to build a "Library" of available skills.
2.  **Granular Assignment:** Allow agents to "equip" specific skills, rather than having all discovered skills available to everyone.
3.  **Dynamic Loading:** Support loading skills from URLs (e.g., Gists or raw Git files) at runtime.

---

## 1. Discovery Layer (The Library)

We need to expand `SkillService` to scan multiple locations.

### Search Paths (Current Priority Order)

1.  **Project Local:** `.tandem/skill/` (canonical write path) and `.tandem/skills/` (read compatibility).
2.  **User Global:**
    - `~/.tandem/skills/` (canonical write path)
    - `~/.agents/skills/` (shared ecosystem format)
    - `~/.claude/skills/` (Anthropic compatibility)
3.  **AppData Global (compatibility):** `%APPDATA%/tandem/skills` (or platform equivalent)

Notes:

- Discovery deduplicates by skill name using the above priority (project overrides global).
- Session-ephemeral skills are not yet implemented.

### Implemented

`crates/tandem-skills/src/lib.rs` now discovers from multiple roots and deduplicates by name priority.

```rust
fn skill_roots(&self) -> Vec<(PathBuf, SkillLocation)> { /* implemented */ }
```

---

## 2. Activation Layer (The Equipment)

Currently, all loaded skills represent potential tools. We need a filter.

### Configuration Schema Update

`AgentDefinition` in `crates/tandem-core/src/agents.rs` now supports:

```rust
pub struct AgentDefinition {
    pub name: String,
    // ...
    pub tools: Option<Vec<String>>,  // Native tools (read, write)
    pub skills: Option<Vec<String>>, // NEW: Skill names to equip
}
```

### Selection Logic (Implemented for `skill` tool)

- `skills: null` -> unrestricted skill access (default behavior).
- `skills: ["*"]` (or `"all"`) -> unrestricted skill access.
- `skills: ["kubernetes", "docker"]` -> only those skills are visible/loadable via `skill`.
- `skills: []` -> no skills available via `skill` for that agent.

---

## 3. Runtime Commands

The user needs to be able to equip skills on the fly.

**Slash Command:** `/equip <skill_name>`

- Searches the **Library** for `skill_name`.
- If found: Adds the skill's tools to the current session's tool registry.
- If not found: Prompts to install/download.

**Slash Command:** `/unequip <skill_name>`

- Removes the tools from the session.

---

## 4. URL Loader (The Import)

Parity with Opencode's ability to pull skills from the web.

**Command:** `/import <url>`

- Downloads the content.
- Parses `SKILL.md`.
- Saves to `.tandem/skill/<name>/SKILL.md` (Project Local) or `~/.tandem/skills` (Global).

---

## Migration Plan

### Phase 1: Enhanced Discovery (P1)

- [x] Update `tandem-skills` to scan `~/.agents` and `~/.claude`.
- [x] Deduplicate skills by name (Project overrides Global).

### Phase 2: Agent Configuration (P1)

- [x] Add `skills` field to `AgentDefinition`.
- [x] Update `EngineLoop` to scope `skill` tool visibility/loading based on equipped skills.

### Phase 3: Runtime Control (P2)

- [ ] Implement `equip` and `unequip` commands in `tandem-core`.
- [ ] Add UI to "Manage Skills" (Visualize the Library vs Equipped).

### Phase 4: URL Import (P2)

- [ ] Add `reqwest` to `tandem-skills` (behind feature flag?).
- [ ] Implement `import_from_url`.
