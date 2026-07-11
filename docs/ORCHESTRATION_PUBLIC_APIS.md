# Orchestration Public APIs

The long-running workflow orchestration kernel (contracts: TAN-687, store:
TAN-688, governed transitions: TAN-689/690, waits: TAN-692/693) is exposed
through two tenant-scoped HTTP surfaces:

- **Authoring** (`/orchestrations`, TAN-694): drafts, validation, immutable
  published versions, stale-reference reporting, dry-run transition previews.
- **Runtime** (`/goals`, TAN-695): goal lifecycle, graph/lineage/event read
  models, governed handoff emission and decisions, external-condition wait
  inspection/resolution, and an SSE change stream.

All routes read the tenant from the standard `x-tandem-*` headers (or the
verified enterprise context). Resource visibility is scoped by
org/workspace/deployment; cross-tenant access fails closed as 404.

## Authoring: `/orchestrations`

| Method | Path | Purpose |
| --- | --- | --- |
| POST | `/orchestrations` | Create a draft (mutable `version 0` slot; incomplete graphs allowed) |
| GET | `/orchestrations` | List orchestrations (draft + published version summary; `?status=`) |
| GET | `/orchestrations/{id}` | Draft and latest published version |
| PUT | `/orchestrations/{id}` | Update the draft — requires `expected_updated_at_ms` (optimistic concurrency; stale token → 409 `draft_concurrency_conflict`) |
| POST | `/orchestrations/{id}/archive` | Archive the draft (published versions stay immutable) |
| POST | `/orchestrations/{id}/validate` | Graph validation + referenced-workflow checks, node/edge-addressed issues |
| GET | `/orchestrations/{id}/stale-references` | Per-node pinned-hash freshness (`fresh` / `stale` / `unpinned` / `missing`) |
| POST | `/orchestrations/{id}/refresh-references` | Re-pin every workflow node to the current definition hash (explicit refresh step) |
| POST | `/orchestrations/{id}/publish` | Snapshot the draft as the next immutable version; blocked (422) while the graph is invalid or references are stale |
| GET | `/orchestrations/{id}/versions` | Immutable version index |
| GET | `/orchestrations/{id}/versions/{version}` | One published snapshot |
| POST | `/orchestrations/{id}/dry-run` | Pure transition preview (`from_node_id`, `transition_key`, optional `artifact_type`/`version`) — no state touched |

Publishing embeds `metadata.publish` into the immutable snapshot: the acting
principal, timestamp, full validation report, and the exact referenced
workflow definition hashes. Active goals keep executing against their
original published snapshot; later publishes create new versions.

## Runtime: `/goals`

| Method | Path | Purpose |
| --- | --- | --- |
| POST | `/goals` | Start a goal from a published version. Requires `idempotency_key`; replaying returns the same goal + root run (`replayed: true`). Root run, hop-0 lineage, and goal-started event are written in one transaction |
| GET | `/goals` | List (`?status=`, `?orchestration_id=`, `?limit=`) |
| GET | `/goals/{id}` | Goal with budget accounting |
| POST | `/goals/{id}/pause` / `/resume` / `/cancel` | Operator lifecycle; terminal goals reject pause/resume with 409 `goal_terminal`; cancel is durable + idempotent |
| GET | `/goals/{id}/graph` | Published graph with per-node state (`current`/`visited`/`not_started`), per-node runs, and the active workflow's internal Automation V2 stage |
| GET | `/goals/{id}/runs` | Hop-ordered lineage links joined with run records |
| GET | `/goals/{id}/events` | Durable event read model; `?cursor=` pages strictly after the given cursor |
| GET | `/goals/{id}/events/stream` | SSE stream (see below) |
| GET | `/goals/{id}/artifacts` | Handoff artifacts + the goal's final artifact |
| GET | `/goals/{id}/budgets` | Policy vs consumed vs remaining (hops/tokens/cost/deadline) |
| POST | `/goals/{id}/transitions` | Emit a governed named transition (`transition_key`, `idempotency_key`, `artifact`); replay returns `AlreadyCommitted`; approval-gated edges return 202 `pending_approval` |
| POST | `/goals/{id}/handoffs/{handoff_id}/decision` | Approve/reject a pending handoff |
| GET | `/goals/{id}/handoffs` | All handoffs for the goal |
| POST | `/goals/{id}/completion` | Settle the active workflow's completion into a terminal node (`transition_key`) or the awaiting-transition state |
| GET | `/goals/{id}/waits` / `/waits/{wait_id}` | Inspect registered waits |
| POST | `/goals/{id}/waits/{wait_id}/resolve` | Resolve an external-condition wait (bounded `idempotency_key` + schema-validated payload) |

### SSE semantics

`GET /goals/{id}/events/stream` emits only events read back from the durable
stateful store, in SQLite-rowid order; the rowid is the SSE `id`. Reconnecting
with `Last-Event-ID` (or `?cursor=`) resumes exactly after the last delivered
event — no gaps, no duplicates — regardless of in-memory event-bus behavior.
The engine bus is used only as a wake signal, with a polling floor so durable
writes from other processes are picked up promptly. Keep-alives every 10s.

### Error contract

Stable machine-readable error codes across both surfaces:
`orchestration_not_found`, `draft_concurrency_conflict`,
`published_version_conflict`, `orchestration_invalid` (422 with the full
validation report), `goal_not_found`, `goal_terminal`, `goal_state_conflict`,
`goal_forbidden`, `wait_not_found`, `wait_resolution_conflict`.

Fine-grained enterprise authority (org-unit grants, delegation, run-as) over
these surfaces lands with TAN-705; SDK/MCP clients land with TAN-696.

Contract tests: `crates/tandem-server/src/http/tests/orchestration_goals.rs`.
