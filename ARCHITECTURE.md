# Tandem Architecture (Updated)

Tandem is a local-first desktop app with three core runtime layers:

1. **React + Vite frontend** (`src/`)
2. **Tauri v2 backend in Rust** (`src-tauri/src/`)
3. **OpenCode sidecar process** managed by Tauri (`src-tauri/src/sidecar.rs`, `src-tauri/src/sidecar_manager.rs`)

---

## 1) Frontend architecture (`src/`)

### App shell and major panels

- Root app composition and view routing live in `src/App.tsx`.
- The shell coordinates:
  - chat workspace (`src/components/chat/`)
  - session/project sidebar (`src/components/sidebar/`)
  - task/todo sidebar (`src/components/tasks/`)
  - file browser + previews (`src/components/files/`)
  - settings/about/extensions/packs panels (`src/components/settings/`, `src/components/about/`, `src/components/extensions/`, `src/components/packs/`)
  - orchestration UI (`src/components/orchestrate/`)
  - Ralph loop UI (`src/components/ralph/`)

### Frontend state and API layer

- Tauri IPC wrappers and shared frontend types: `src/lib/tauri.ts`
- App-level state hooks:
  - `src/hooks/useAppState.ts`
  - `src/hooks/usePlans.ts`
  - `src/hooks/useTodos.ts`
  - `src/hooks/useStagingArea.ts`
- Memory indexing UI context: `src/contexts/MemoryIndexingContext.tsx`

---

## 2) Backend architecture (`src-tauri/src/`)

### Entry point and app wiring

- `src-tauri/src/lib.rs` is the main backend entrypoint used by `src-tauri/src/main.rs`.
- In `run()`, Tandem:
  - initializes plugins and logging
  - loads persisted settings/projects
  - initializes vault state and app state
  - initializes the memory manager (SQLite-backed)
  - syncs bundled skills/tools
  - registers the full IPC surface via `tauri::generate_handler!`

### Core backend modules

- **IPC command surface**: `src-tauri/src/commands.rs`
- **Global app state**: `src-tauri/src/state.rs`
- **Sidecar process management**:
  - `src-tauri/src/sidecar.rs`
  - `src-tauri/src/sidecar_manager.rs`
- **Tool approval, staging, and operation journal**: `src-tauri/src/tool_proxy.rs`
- **Vault + key storage**:
  - `src-tauri/src/vault.rs`
  - `src-tauri/src/keystore.rs`
- **Provider/model routing helpers**: `src-tauri/src/llm_router.rs`
- **Skills and templates**:
  - `src-tauri/src/skills.rs`
  - `src-tauri/src/skill_templates.rs`
- **Packs (guided workflows)**: `src-tauri/src/packs.rs`
- **OpenCode plugin/MCP config management**: `src-tauri/src/opencode_config.rs`
- **Logs and file watcher**:
  - `src-tauri/src/logs.rs`
  - `src-tauri/src/file_watcher.rs`

### Feature subsystems in backend

- **Memory subsystem** (`src-tauri/src/memory/`)
  - chunking, embeddings, sqlite persistence, indexing manager, and types
- **Orchestrator subsystem** (`src-tauri/src/orchestrator/`)
  - task graph + scheduler + policy + budget + engine/store/locks
- **Ralph loop subsystem** (`src-tauri/src/ralph/`)
  - loop service, storage, and run types
- **Presentation export**: `src-tauri/src/presentation.rs`

---

## 3) Runtime data flow

1. User interacts with React UI.
2. Frontend calls Tauri commands via `invoke` wrappers in `src/lib/tauri.ts`.
3. `commands.rs` delegates work to state managers/subsystems (sidecar, tool proxy, memory, orchestrator, etc.).
4. Sidecar events/streams are emitted back to frontend listeners.
5. Frontend updates chat/tasks/files/orchestration views.

This keeps UX logic in TypeScript while safety-critical operations (process control, filesystem policy, encrypted key handling) stay in Rust.

---

## 4) Security and trust boundaries

- API keys are encrypted and stored locally (vault + keystore modules).
- File and tool execution paths are mediated by backend approval/proxy logic.
- Sidecar execution is supervised by the Tauri backend instead of directly by the frontend.
- Workspace/project scoping lives in shared backend state and is enforced before sensitive operations.

---

## 5) Notes on parity with current code

This document reflects the currently implemented modules and features, including:

- multi-project management
- memory indexing/statistics
- orchestration mode
- Ralph loop controls
- packs/skills/template installation
- OpenCode plugin + MCP configuration helpers

If new IPC domains are added, update this file together with `src-tauri/src/lib.rs` command registration and `src/lib/tauri.ts` wrappers.
