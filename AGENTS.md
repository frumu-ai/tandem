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

## Docs

Docs exist in `docs/`:

## Fleet Task Scope

Repository-modifying fleet, Codex, and spawned-agent tasks must carry a machine-readable scope file at `.tandem/task-scope.json`. Before edits, run `python scripts/task_scope_guard.py --scope .tandem/task-scope.json --trust-registry .tandem/approved-task-scopes.json --trust-ref origin/main preflight --issue TAN-754 --deliverable documentation`, replacing the issue and deliverable with values approved for the current task. Run the guard's `diff` command before opening or updating a pull request.

The same scope file and digest apply to the root task, spawned agents, retries, and resumed work. Parked, canceled, blocked, or excluded issues are denied unless the file contains an exact, recorded human scope-expansion approval. Agents may not add or approve their own expansion. If requested issues, deliverables, or changed repository paths fall outside the effective scope, stop before modifying the repository and request human approval.

Pull requests from `codex/`, `fleet/`, or `agent/` branches are checked by `.github/workflows/task-scope.yml`. After the bootstrap change, CI executes the guard from the pull request's base commit, verifies linked issue IDs and the final diff, then uploads preflight/diff receipts for audit.
