# AGENTS.md

## Project Structure

```
tandem/
├── crates/           # Rust server and core libraries
├── engine/           # Engine components
├── packages/         # Frontend packages (React components, etc.)
├── src-tauri/        # Tauri desktop app
└── docs/             # Documentation
```

## Crates

| Crate                   | Purpose            |
| ----------------------- | ------------------ |
| `crates/tandem-server/` | Server application |
| `crates/tandem-core/`   | Core engine logic  |
| `crates/tandem-cli/`    | CLI tools          |

## Key Paths

| What             | Path                                               |
| ---------------- | -------------------------------------------------- |
| Automation logic | `crates/tandem-server/src/app/state/automation.rs` |
| Engine loop      | `crates/tandem-core/src/engine_loop.rs`            |
| HTTP handlers    | `crates/tandem-server/src/http/`                   |
| Control panel    | `packages/tandem-control-panel/src/`               |

## File Size Guidelines

- Source files: stay under 1500 lines
- If a file exceeds 1500 lines, consider whether it should be split

## Commit Sign-off

- All repository commits must include a DCO sign-off. Create commits with `git commit -s`.
- Before publishing a branch, verify every commit after the base branch contains a `Signed-off-by` trailer.
- If a local commit is missing its sign-off, amend or rebase only the commit metadata. Do not rewrite published history without explicit user approval.

## Docs

Docs exist in `docs/`:
