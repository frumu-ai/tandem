# Tandem Architecture (High Level)

Tandem is a native desktop app built with:

- **React + Vite** frontend
- **Tauri v2 + Rust** backend
- An OpenCode **engine process** running locally (sidecar)

## Key subsystems

- **IPC commands**: `src-tauri/src/commands.rs`
- **Sidecar lifecycle**: `src-tauri/src/sidecar_manager.rs`, `src-tauri/src/sidecar.rs`
- **Permissions & tool proxy**: `src-tauri/src/tool_proxy.rs`
- **Skills**: `src-tauri/src/skills.rs` + UI in `src/components/skills/`
- **Packs (guided workflows)**: `src-tauri/src/packs.rs` + UI in `src/components/packs/`
