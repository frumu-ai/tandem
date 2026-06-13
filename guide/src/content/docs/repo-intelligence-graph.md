---
title: Repo Intelligence Graph
description: How to create, refresh, and use Tandem's source-derived repository graph for coding agents.
---

The repo intelligence graph is Tandem's deterministic map of a workspace repo.
It indexes files, symbols, imports, config references, docs headings, and graph
edges so coding agents can orient before broad file discovery.

Use it as discovery evidence. Agents should still read concrete files before
editing them or making final claims about their contents.

## When the graph is created

The graph is created by the `repo.index` engine tool. It persists a snapshot at:

```text
.tandem/repo-index.json
```

It also writes a human-debuggable export at:

```text
.tandem/repo-graph.json
```

The `.tandem/` directory must be ignored by git. Tandem Agents deliberately
skips the persistent refresh when `.tandem/repo-index.json` is not ignored, so
runtime artifacts do not accidentally become tracked source files.

## Automatic use in Tandem Agents

ACA uses repo intelligence during the manager planning phase when the connected
engine exposes the repo tools.

At the start of planning, ACA:

1. checks whether `repo.context_bundle` is available
2. refreshes the index with `repo.index` when that tool is available and the
   snapshot path is ignored
3. calls `repo.context_bundle` for the task
4. writes the bundle artifact into the run's artifacts directory as
   `repo_context_bundle.json`
5. records `repo_context` metadata in the run blackboard and status file

If the tools are missing, denied, or fail, ACA falls back to its older heuristic
repo discovery path and records that fallback in run state.

ACA does not have to use planning mode for the graph to exist. Planning is the
automatic path. Manual and non-planning flows can call the same engine tools
directly.

## Manual trigger with the CLI

Run this from any shell that can see the target repo and the Tandem engine CLI:

```bash
tandem-engine tool --json '{"tool":"repo.index","args":{"repo_path":"/path/to/repo"}}'
```

For the Tandem repo itself:

```bash
tandem-engine tool --json '{"tool":"repo.index","args":{"repo_path":"/home/evan/tandem"}}'
```

Verify that query tools are reading the stored snapshot:

```bash
tandem-engine tool --json '{"tool":"repo.context_bundle","args":{"repo_path":"/home/evan/tandem","task":"Explain how repo intelligence is wired into Tandem Agents","limit":8}}'
```

In the result metadata, `index_source` should be `stored`. If it is
`ephemeral_scan_after_load_error:...`, Tandem could not load the stored snapshot
and scanned the repo for that query instead.

## Manual trigger over HTTP

When the engine is running as a service, call the same tool through
`POST /tool/execute`:

```bash
curl -sS -X POST http://127.0.0.1:39731/tool/execute \
  -H "content-type: application/json" \
  -d '{"tool":"repo.index","args":{"repo_path":"/path/to/repo"}}'
```

If your engine requires an API token, include the same authorization header you
use for other Tandem engine requests.

## Refresh after edits

After files change, refresh the index before relying on impact analysis or test
target suggestions:

```bash
tandem-engine tool --json '{"tool":"repo.update_changed_files","args":{"repo_path":"/path/to/repo","changed_files":["src/example.ts"]}}'
```

The current implementation performs a safe full rescan and records
`refresh_mode: full_rescan`.

## Tools agents should use

After the index exists, use these tools before broad `grep`, `glob`, or
semantic search:

- `repo.context_bundle`: build a bounded task-oriented context bundle
- `repo.search`: search indexed files, symbols, imports, config, and docs
- `repo.symbol`: find symbols by name and optional kind
- `repo.neighbors`: traverse related graph nodes from a file or symbol
- `repo.impact`: summarize fallout from changed files
- `repo.test_targets`: return likely test targets from changed files

For task startup, prefer `repo.context_bundle` first. For post-edit validation,
refresh with `repo.update_changed_files`, then call `repo.impact` or
`repo.test_targets`.

## Rebuild checklist

After repo intelligence changes land in Tandem, rebuild or restart whichever
engine your agents are actually using.

For a local source checkout:

```bash
cd /home/evan/tandem
git checkout main
git pull --ff-only origin main
cargo build -p tandem-ai
```

For the Tandem Agents Compose stack, make sure the `tandem-engine` sidecar is
using a Tandem release or local package that contains the repo intelligence
tools, then rebuild/restart the stack:

```bash
cd /home/evan/tandem-agents
./scripts/build-containers.sh --build
./scripts/build-containers.sh --up
./scripts/run.sh --check-engine
```

Check tool availability before expecting ACA or Tandem Coder to use the graph:

```bash
curl -sS http://127.0.0.1:39731/tool/ids
```

Look for `repo.index`, `repo.context_bundle`, and the other `repo.*` tools.

## Troubleshooting

- If `repo.index` is missing, the running engine is older than the repo
  intelligence tool release.
- If ACA falls back to heuristic discovery, inspect the run's `repo_context`
  status metadata and `repo_context_bundle.json` artifact.
- If the snapshot is skipped, confirm `.tandem/` is ignored by git in the target
  repo.
- If query results are stale after edits, call `repo.update_changed_files`.
- If a query reports `ephemeral_scan_after_load_error`, recreate the stored
  snapshot with `repo.index`.
