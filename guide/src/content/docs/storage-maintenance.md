---
title: Storage Maintenance For Agents
description: How agents should inspect Tandem storage, run cleanup safely, and understand hot indexes versus immutable history.
---

Tandem keeps active runtime state small and moves long-lived history into sharded storage. Agents working on local repair, release testing, or operator runbooks should treat storage cleanup as a maintenance operation, not as an automation workflow step.

## Storage shape

The local state root is usually `~/.local/share/tandem` on Linux or the configured `TANDEM_HOME` / `TANDEM_STATE_DIR`.

Important directories:

| Path                                 | Purpose                                          |
| ------------------------------------ | ------------------------------------------------ |
| `data/automation-runs/YYYY/MM/`      | Immutable Automation V2 per-run history shards   |
| `data/context-runs/hot/`             | Active and recent context-run directories        |
| `data/context-runs/archive/YYYY/MM/` | Compressed old context-run archives              |
| `data/mcp/`                          | MCP registry files                               |
| `data/channels/`                     | Channel sessions and tool preferences            |
| `data/routines/`                     | Routine definitions and run history              |
| `data/bug-monitor/`                  | Bug Monitor config, incidents, drafts, and posts |
| `data/actions/`                      | External action history                          |
| `data/pack-builder/`                 | Pack-builder workflows, plans, and zip artifacts |
| `data/system/`                       | Shared resources and other system-level state    |
| `data/knowledge/`                    | Legacy embedded-doc bootstrap markers            |
| `data/workflow-planner/`             | Workflow-planner sessions                        |

Hot indexes should stay small. Large node outputs, blackboards, runtime context, and terminal run details belong in per-run files, JSONL shards, or artifact files referenced by path.

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

## SDK inspection

The TypeScript and Python SDKs expose storage inspection helpers for agents and tools that need to list files or trigger the legacy session-storage repair scan:

```ts
const files = await client.storage.listFiles({ path: "data/context-runs", limit: 100 });
await client.storage.repair({ force: true });
```

```python
files = await client.storage.list_files(path="data/context-runs", limit=100)
await client.storage.repair(force=True)
```

These SDK methods do not run archive cleanup. Cleanup changes local files and may quarantine data, so agents should use the CLI command with explicit operator intent.

## Agent guidance

Before fixing workflow or Bug Monitor bugs on a machine with slow startup, inspect storage first. A root directory full of large JSON maps or thousands of legacy `context_runs` entries can make unrelated bugs look worse.

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
