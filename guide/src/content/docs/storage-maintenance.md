---
title: Storage Maintenance For Agents
description: How agents should inspect Tandem storage, run cleanup safely, and understand hot indexes versus immutable history.
---

Tandem keeps active runtime state small and moves long-lived history into sharded storage. Agents working on local repair, release testing, or operator runbooks should treat storage cleanup as a maintenance operation, not as an automation workflow step.

## Storage shape

The local state root is usually `~/.local/share/tandem` on Linux or the configured `TANDEM_HOME` / `TANDEM_STATE_DIR`.

Important directories:

| Path                                 | Purpose                                               |
| ------------------------------------ | ----------------------------------------------------- |
| `data/automation-runs/YYYY/MM/`      | Immutable Automation V2 per-run history shards        |
| `data/context-runs/hot/`             | Active and recent context-run directories             |
| `data/context-runs/archive/YYYY/MM/` | Compressed old context-run archives                   |
| `data/mcp/`                          | MCP registry files                                    |
| `data/channels/`                     | Channel sessions and tool preferences                 |
| `data/routines/`                     | Routine definitions and run history                   |
| `data/incident-monitor/`             | Incident Monitor config, incidents, drafts, and posts |
| `data/actions/`                      | External action history                               |
| `data/pack-builder/`                 | Pack-builder workflows, plans, and zip artifacts      |
| `data/system/`                       | Shared resources and other system-level state         |
| `data/knowledge/`                    | Legacy embedded-doc bootstrap markers                 |
| `data/workflow-planner/`             | Workflow-planner sessions                             |

Hot indexes should stay small. Large node outputs, blackboards, runtime context, and terminal run details belong in per-run files, JSONL shards, or artifact files referenced by path.

## PostgreSQL stateful backend

The stateful orchestration store can run on PostgreSQL while the normal local
session/questions and runtime-event databases remain SQLite files in the
runtime root. Backend changes are an offline operator task: stop the engine,
run the verified transfer, retain the JSON report, then start the engine with
the chosen backend. A migration to a different `--target-state-dir` also takes
verified SQLite snapshots of the session/messages and runtime-event stores.

```bash
sudo systemctl stop tandem-engine
tandem-engine storage migrate \
  --from sqlite --to postgres \
  --state-dir /srv/tandem \
  --target-postgres-url "$TANDEM_STORAGE_TARGET_POSTGRES_URL" \
  --json
sudo systemctl start tandem-engine
```

The target refuses to start while a transfer is incomplete. A rerun with the
same source resumes safely; never point the command at a populated target or
copy database files by hand. See the [PostgreSQL Stateful Storage runbook](https://github.com/frumu-ai/tandem/blob/main/docs/POSTGRES_STATEFUL_STORAGE.md)
for rollback, backup, and maintenance guidance.

## Cleanup commands

Most agents and operators should use the normal command:

```bash
tandem-engine storage cleanup --dry-run --context-runs --json
tandem-engine storage cleanup --dry-run --root-json --json
tandem-engine storage cleanup --dry-run --default-knowledge --json
```

For an actual local cleanup:

```bash
sudo systemctl stop tandem-engine
tandem-engine storage cleanup --context-runs --root-json --default-knowledge --quarantine --json
sudo systemctl restart tandem-engine
```

On developer machines with more than one `tandem-engine` on `PATH`, run `which -a tandem-engine` first and call the intended binary explicitly.

Use `--retention-days <N>` to tune how long terminal context runs stay hot. The default is conservative for local repair work.

## Managed worktree cleanup

Managed Git worktrees are different from the engine state root. Tandem creates them per repository under:

- `<repo>/.tandem/worktrees/<slug>`

These worktrees are used for isolated coder runs, agent-team instances, and other edit-capable repo tasks. If a run is blocked, crashes, or the process restarts before teardown, the Git worktree entry can remain registered even after the task is gone.

Typical symptoms:

- `.tandem/worktrees/` grows large inside a repo
- `git worktree list` shows many old `tandem/...` branches
- operators have to manually remove worktrees and branches

Use the packaged CLI for a preview first:

```bash
tandem-engine storage worktrees --repo-root /abs/path/to/repo --json
```

Apply cleanup only after reviewing the preview:

```bash
tandem-engine storage worktrees --repo-root /abs/path/to/repo --apply --json
```

What this cleanup does:

1. Reads Git-registered worktrees under `<repo>/.tandem/worktrees`
2. Compares them to Tandem's currently tracked in-memory managed worktrees
3. Skips worktrees that the live runtime still considers active
4. Removes stale Git worktrees and their managed branches when possible
5. Removes orphaned leftover directories that are no longer Git-registered

This cleanup is also available from the control panel at `Settings -> Maintenance`, where operators can preview stale worktrees, run cleanup, and inspect an animated per-item log of what was skipped, removed, or failed.

### Runtime API and SDK access

The same operation is available through the engine runtime API:

```http
POST /worktree/cleanup
```

Example request:

```json
{
  "repo_root": "/abs/path/to/repo",
  "dry_run": true,
  "remove_orphan_dirs": true
}
```

Use the HTTP or SDK path when an operator tool, external service, or governed agent flow needs to inspect or clean stale worktrees without shelling out to the CLI.

## SDK inspection

The TypeScript and Python SDKs expose storage inspection helpers for agents and tools that need to list files or trigger the legacy session-storage repair scan, plus worktree cleanup helpers for repo-local stale worktree maintenance:

```ts
const files = await client.storage.listFiles({ path: "data/context-runs", limit: 100 });
await client.storage.repair({ force: true });
const preview = await client.worktrees.cleanup({
  repoRoot: "/abs/path/to/repo",
  dryRun: true,
});
```

```python
files = await client.storage.list_files(path="data/context-runs", limit=100)
await client.storage.repair(force=True)
preview = await client.worktrees.cleanup(
    repo_root="/abs/path/to/repo",
    dry_run=True,
)
```

Storage SDK methods do not run archive cleanup. Worktree cleanup only affects repo-local managed worktrees for the selected repository. Both should still be treated as operator-directed maintenance actions rather than background workflow behavior.

## Agent guidance

Before fixing workflow or Incident Monitor bugs on a machine with slow startup, inspect storage first. A root directory full of large JSON maps or thousands of legacy `context_runs` entries can make unrelated bugs look worse.

The old embedded docs path used `guide_docs:` memory rows plus a legacy
`default_knowledge_state.json` marker. If those still exist on a machine, purge
them with `--default-knowledge` once and then keep using the docs MCP server for
Tandem-specific guidance.

Prefer this order:

1. Run dry-run cleanup with `--json`.
2. Stop the engine service before mutating local storage.
3. Run cleanup with `--quarantine` so moved root files can be recovered.
4. Restart the engine and verify startup time.
5. Only then continue debugging workflow behavior.

For worktree maintenance, prefer this order:

1. Run `tandem-engine storage worktrees --repo-root ... --json` first.
2. Confirm the reported stale paths are not tied to an active operator session.
3. Apply cleanup with `--apply`.
4. Re-run `git worktree list` and verify the repo is back to the expected set of worktrees.
