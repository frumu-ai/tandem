# Runtime Event Schema

Canonical schema for events published on the engine event bus
(`crates/tandem-core/src/event_bus.rs`). Defined in
`crates/tandem-types/src/runtime_event.rs` (TAN-199).

## Envelope contract

Every event published through `EventBus::publish` is stamped with a
`RuntimeEventEnvelope` (emitters may also pre-stamp one; the bus never
overwrites an existing envelope):

| Field            | Type             | Meaning                                                                                                                                                                                     |
| ---------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `event_id`       | string (UUID v4) | Globally unique id for this event instance.                                                                                                                                                 |
| `seq`            | u64              | Monotonic per-process sequence number assigned by the bus. Consumers can detect gaps caused by the broadcast channel dropping lagging subscribers (buffer 2048). Resets on process restart. |
| `schema_version` | u32              | Envelope contract version. Currently `1`. Bumped on any breaking change.                                                                                                                    |
| `occurred_at_ms` | u64              | Milliseconds since Unix epoch at publish time.                                                                                                                                              |
| `session_id`     | string?          | Canonical session correlation id, extracted from the payload's historical spellings (`sessionID` / `sessionId` / `session_id`).                                                             |
| `run_id`         | string?          | Canonical run correlation id (`runID` / `runId` / `run_id`).                                                                                                                                |
| `node_id`        | string?          | Canonical automation node id (`nodeID` / `nodeId` / `node_id`).                                                                                                                             |
| `tenant_context` | TenantContext?   | Tenant attribution, extracted from `tenantContext` / `tenant_context` in the payload.                                                                                                       |

On the wire (SSE `/event`, `/run/{id}/events`), the envelope appears as an
additional `envelope` key on the legacy event shape, which stays unchanged:

```json
{
  "type": "session.run.started",
  "properties": { "sessionID": "ses_1", "runID": "run_1" },
  "envelope": {
    "event_id": "7f9b6c2e-…",
    "seq": 184,
    "schema_version": 1,
    "occurred_at_ms": 1765430400000,
    "session_id": "ses_1",
    "run_id": "run_1"
  }
}
```

This is additive: pre-envelope consumers (TUI, control panel, the TS client's
Zod normalizer) ignore the extra key. New consumers should prefer
`envelope.*` over re-deriving ids from `properties` and may decode canonical
events into the typed `RuntimeEvent` via `RuntimeEvent::from_engine_event`.

`RuntimeEvent` serializes flat:

```json
{
  "event_id": "…",
  "seq": 184,
  "schema_version": 1,
  "occurred_at_ms": 1765430400000,
  "session_id": "ses_1",
  "run_id": "run_1",
  "event_type": "session.run.started",
  "payload": { "sessionID": "ses_1", "runID": "run_1" }
}
```

## Durable event log

The server persists canonical runtime events that have a `run_id` or
`session_id` to a JSONL ledger at `runtime/events.jsonl` under Tandem's
canonical data root. Each row is the flat `RuntimeEvent` shape above, including
`seq`, `event_id`, `occurred_at_ms`, and optional `tenant_context`.

The durable log persister registers a dedicated bounded, single-consumer queue
with the event bus before it waits for runtime readiness. Events published
after registration are buffered for the persister, so persistence does not
depend on a broadcast subscriber being attached at publish time. Plain
`EventBus` instances without a registered persister do not retain a runtime
event-log queue. If the bounded queue fills, publish stays non-blocking and the
event is dropped; consumers can detect missing persisted events through `seq`
gaps.

Retention is controlled by `TANDEM_RUNTIME_EVENT_LOG_RETENTION_DAYS` and
defaults to 30 days. Set it to `0` to disable startup cleanup.

Tenant-scoped replay is available via:

```text
GET /runs/{run_id}/events?after_seq=<seq>&limit=<n>
```

The response includes:

- `events`: canonical runtime event rows visible to the request tenant.
- `last_seq`: the last returned bus sequence, suitable for the next
  `after_seq` query.
- `sequence_scope`: currently `runtime_event_bus`, meaning `seq` is global to
  the event bus process. Missing sequence numbers between returned rows can
  indicate filtered tenants or other runs in the same process.

Explicit tenant requests only see rows whose `tenant_context` exactly matches
their `org_id`, `workspace_id`, and `deployment_id`. Local implicit requests
retain local single-tenant behavior and can read all rows.

## Schema policy

- **Closed vocabulary.** `RuntimeEventType` is a closed enum; `parse()`
  returns `None` for anything else. Adding an event type means adding it to
  the macro table in `runtime_event.rs` _and_ to this document in the same
  change.
- **Content by reference.** Payloads must not carry prompt text, tool
  arguments/results, or artifact content by default. Carry ids and refs
  (`messageID`, artifact refs, hashes, previews at most).
- **Non-canonical events.** Externally ingested events (e.g. workflow
  trigger ingest on `POST /workflows/events`) and a few dynamic emitters can
  publish event types outside the vocabulary. They still receive an envelope
  at the bus, but have no typed representation. Treat any new recurring
  non-canonical type as a bug: promote it into the vocabulary.
- **Legacy id spellings** (`sessionID` vs `session_id` vs `sessionId`) inside
  `properties` are **deprecated** in favor of the envelope's canonical
  fields. Do not introduce new spellings; the extractor accepts only the
  three historical ones.

## Event vocabulary

Grouped by domain. "Key payload fields" lists the load-bearing properties,
not every key.

### Session lifecycle

| Event type                           | Fires when                                       | Key payload fields                                                  |
| ------------------------------------ | ------------------------------------------------ | ------------------------------------------------------------------- |
| `session.created`                    | A session is created.                            | `sessionID`                                                         |
| `session.attached`                   | A client attaches to an existing session.        | `sessionID`                                                         |
| `session.updated`                    | Session metadata changes.                        | `sessionID`, `tenantContext`                                        |
| `session.status`                     | Session status broadcast.                        | `sessionID`                                                         |
| `session.error`                      | A session-scoped error is surfaced.              | `sessionID`, `runID`, `reason`, `component`                         |
| `session.delete.deferred`            | Session deletion deferred while a run is active. | `sessionID`                                                         |
| `session.run.started`                | A prompt run starts inside a session.            | `sessionID`, `runID`                                                |
| `session.run.finished`               | A prompt run finishes.                           | `sessionID`, `runID`, `finishedAtMs`, `status`                      |
| `session.run.conflict`               | A run is rejected because one is already active. | `sessionID`                                                         |
| `session.workspace_override.granted` | A temporary workspace override is granted.       | `sessionID`                                                         |
| `workspace.override.activated`       | Workspace override activates in the engine loop. | `sessionID`, `requestedTtlSeconds`, `cappedTtlSeconds`, `expiresAt` |
| `workspace.override.expired`         | Workspace override TTL expires.                  | `sessionID`                                                         |

### Engine / server lifecycle

| Event type                              | Fires when                                 | Key payload fields                                          |
| --------------------------------------- | ------------------------------------------ | ----------------------------------------------------------- |
| `server.connected`                      | SSE client attaches to `/event`.           | —                                                           |
| `engine.lifecycle.ready`                | Engine reports ready on the event stream.  | `status`, `transport`, `timestamp_ms`                       |
| `run.stream.connected`                  | SSE client attaches to `/run/{id}/events`. | `runID`                                                     |
| `registry.updated`                      | A registry entity (presets etc.) changes.  | `entity`                                                    |
| `resource.updated` / `resource.deleted` | A shared resource changes / is removed.    | `key`, `rev`, `updatedBy`, `updatedAtMs`                    |
| `channel.status.changed`                | Channel connectivity changes.              | `channels`                                                  |
| `channel.capability.changed`            | A channel user capability tier changes.    | `channel`, `user_id`, `max_tier`, `actor_id`, `executed_as` |

### Message & interaction

| Event type             | Fires when                                                                                                            | Key payload fields           |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------- | ---------------------------- |
| `message.part.updated` | A message part (text/tool) updates during a run. Running tool deltas are filtered from the session persistence queue. | `sessionID`, `runID`, `part` |
| `todo.updated`         | The todo part of a run is rewritten.                                                                                  | `part`                       |
| `question.asked`       | The engine asks the user a question.                                                                                  | `id`                         |
| `question.replied`     | A question receives an answer.                                                                                        | `id`, `ok`                   |

### Provider calls & context assembly

| Event type                                                     | Fires when                                              | Key payload fields                                                         |
| -------------------------------------------------------------- | ------------------------------------------------------- | -------------------------------------------------------------------------- |
| `provider.call.iteration.start`                                | A provider call iteration begins.                       | `sessionID`, `messageID`, `iteration`, `selectedToolCount`                 |
| `provider.call.iteration.finish`                               | An iteration completes.                                 | `sessionID`, `messageID`, `iteration`, `finishReason`, `acceptedToolCalls` |
| `provider.call.iteration.retry`                                | An iteration is retried.                                | `sessionID`, `messageID`, `providerID`, `modelID`, `iteration`             |
| `provider.call.iteration.error`                                | An iteration fails.                                     | `sessionID`, `messageID`, `iteration`, `error`                             |
| `provider.call.iteration.budget_exhausted`                     | The iteration budget is exhausted.                      | `sessionID`, `messageID`, `maxIterations`, `error`                         |
| `provider.usage`                                               | Provider token/cost usage is recorded.                  | usage counters                                                             |
| `context.profile.selected`                                     | A context profile is chosen for a prompt.               | `sessionID`, `messageID`, `contextMode`, `historyMessageCount`             |
| `context.mode.full.selected`                                   | Full-context mode is selected.                          | `sessionID`, `messageID`, `providerID`, `modelID`                          |
| `context.budget.final`                                         | Final context budget computed for a call.               | `sessionID`, `messageID`, `providerID`, `modelID`                          |
| `context.budget.bypassed`                                      | A component bypasses context budgeting.                 | `component`, `reason`, `sessionID`, `promptChars`                          |
| `context.full.budget.warning` / `context.full.budget.exceeded` | Estimated full-context size crosses soft / hard budget. | `sessionID`, `messageID`, `estimatedTotalChars`, contributors              |

### Data boundary

Emitted by the provider-dispatch boundary evaluation and the audit-only context-source guards (`TANDEM_DATA_BOUNDARY_MODE`, see `docs/DATA_BOUNDARY_MODULE.md`). Payloads carry classes, counts, hashes, spans, and policy fingerprints — never raw prompt, tool, memory, or model content. `enforced` distinguishes audit evidence from applied enforcement; source-guard events carry `sourceKind` (`tool_result`, `prompt_context_hook`).

| Event type                       | Fires when                                                            | Key payload fields                                                                                        |
| -------------------------------- | --------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `data_boundary.evaluated`        | A request or context source was evaluated without an enforcement outcome: every audit-mode decision, enforce-mode allows, and source-guard findings. | `sessionID`, `messageID`, `providerID`, `modelID`, `action`, `enforced`, `decidedEventKind`, `classificationSource`, `sourceKind`, `finding_summary`, `payload_hash`, `policy_fingerprint`, `reason_codes`, `decision_latency_ms` |
| `data_boundary.redacted`         | Enforce mode redacted the dispatched payload per message.             | same as `data_boundary.evaluated` plus `transformedSpans`                                                  |
| `data_boundary.tokenized`        | Enforce mode tokenized the dispatched payload per message.            | same as `data_boundary.evaluated` plus `transformedSpans`                                                  |
| `data_boundary.blocked`          | Enforce mode blocked the dispatch; the session run fails with an audit-safe error. | same as `data_boundary.evaluated`                                                          |
| `data_boundary.approval_required`| Enforce mode requires human approval before dispatch (permission ask `data_boundary_egress`; deny/timeout fail closed). | same as `data_boundary.evaluated`                                     |
| `data_boundary.routed_local`     | Enforce mode required local routing; fails closed with `route_to_local_unavailable` until routing exists. | same as `data_boundary.evaluated`                                                  |

### Tool execution & governance

| Event type                                | Fires when                                                                                                                                                                    | Key payload fields                                                          |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `tool.routing.decision`                   | Tool routing mode decided for an iteration.                                                                                                                                   | `sessionID`, `messageID`, `iteration`, `pass`, `mode`                       |
| `tool.args.normalized`                    | Tool args normalized before execution.                                                                                                                                        | `sessionID`, `messageID`, `tool`, `argsSource`, `argsIntegrity`             |
| `tool.args.recovered`                     | Malformed args recovered.                                                                                                                                                     | `sessionID`, `messageID`, `tool`, `rawArgsPreview`, `normalizedArgsPreview` |
| `tool.args.recovered_write_auto_approved` | Recovered write auto-approved.                                                                                                                                                | `sessionID`, `messageID`, `tool`                                            |
| `tool.args.missing_terminal`              | Args unrecoverable; call terminal.                                                                                                                                            | `sessionID`, `messageID`, `tool`, `rawArgsState`                            |
| `tool.call.rejected_unoffered`            | Model called a tool that was not offered.                                                                                                                                     | `sessionID`, `messageID`, `iteration`, `tool`, `offeredToolCount`           |
| `tool.call.rejected_write_policy`         | Write rejected by policy.                                                                                                                                                     | `part`                                                                      |
| `tool.loop_guard.triggered`               | Duplicate-call loop guard trips.                                                                                                                                              | `sessionID`, `messageID`, `tool`, `reason`, `duplicateLimit`                |
| `tool.mode.required.unsatisfied`          | Required-tool mode not satisfied.                                                                                                                                             | `sessionID`, `messageID`, `selectedToolCount`, `reason`                     |
| `tool.effect.recorded`                    | A tool effect is recorded in the ledger.                                                                                                                                      | `sessionID`, `messageID`, `tool`, `record`                                  |
| `tool.execution.denied`                   | Execution-time authorization denies a directly invoked tool that discovery filtering would have hidden (strict tenant context only; also written to the protected audit log). | `sessionID`, `messageID`, `tool`, `reason`, `principal`                     |
| `mutation.checkpoint.recorded`            | A mutation checkpoint is recorded.                                                                                                                                            | `sessionID`, `messageID`, `tool`, `record`                                  |
| `prewrite.gate.strict_mode.blocked`       | Strict prewrite gate blocks a write.                                                                                                                                          | `sessionID`, `messageID`, `iteration`, `unmetCodes`                         |
| `prewrite.gate.waived.write_executed`     | A waived prewrite gate lets a write run.                                                                                                                                      | `sessionID`, `messageID`, `unmetCodes`                                      |

### Permissions, approvals & egress

| Event type                                                              | Fires when                                                                                                                                     | Key payload fields                                                 |
| ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `permission.asked`                                                      | A permission request is raised.                                                                                                                | `sessionID`, `requestID`, `tool`, `argsSource`                     |
| `permission.replied`                                                    | A permission request is answered.                                                                                                              | `requestID`, `reply`                                               |
| `permission.auto_approved`                                              | A tool call is auto-approved.                                                                                                                  | `sessionID`, `messageID`, `tool`                                   |
| `permission.wait.timeout`                                               | A permission wait times out.                                                                                                                   | `sessionID`, `messageID`, `tool`, `requestID`, `timeoutMs`         |
| `approval.gate.tool.gated`                                              | The approval gate intercepts a risky tool.                                                                                                     | `sessionID`, `messageID`, `tool`, `riskTier`, `effect`             |
| `approval.decision.recorded`                                            | An approval decision is recorded (audit).                                                                                                      | `run_id`, `automation_id`, `node_id`, `decision`, `executed_as`    |
| `audit.export.denied`                                                   | A governance evidence export is rejected because the strict principal lacks read authority for an included data class (protected audit event). | `runID`, `resourceKind`, `dataClass`, `reason`                     |
| `egress.preflight.denied` / `egress.preflight.approval_required`        | Egress preflight inspection denies / gates an outbound action.                                                                                 | `sessionID`, `messageID`, `tool`, `riskTier`, `target`, `findings` |
| `fintech.protected_action.approved` / `fintech.protected_action.denied` | A fintech-protected action is approved / denied.                                                                                               | `runID`, `automationID`, `tool`, `category`, `classification`      |
| `policy.decision.recorded`                                              | A policy decision is persisted.                                                                                                                | `decisionID`, `sessionID`, `messageID`, `runID`, `automationID`    |

### Automations & routines

| Event type                                                            | Fires when                                                                                                                                                                  | Key payload fields                                                                       |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `automation.v2.run.created`                                           | An automation v2 run is created. **Deprecated naming**: dot-form `automation.v2.*` is inconsistent with `automation_v2.run.failed`; do not add new `automation.v2.*` types. | `automationID`, `run`, `tenantContext`, `triggerType`                                    |
| `automation_v2.run.failed`                                            | An automation v2 node/run fails (rich failure context).                                                                                                                     | `automation_id`, `run_id`, `node_id`, `error_kind`, `attempt`, `status`, `tenantContext` |
| `automation.updated`                                                  | An automation definition changes.                                                                                                                                           | `automationID`                                                                           |
| `automation.read_only_write.denied`                                   | A read-only automation attempts a write.                                                                                                                                    | `sessionID`, `messageID`, `tool`, `reason`                                               |
| `routine.created` / `routine.updated` / `routine.deleted`             | Routine CRUD.                                                                                                                                                               | `routineID`, `status`, `nextFireAtMs`                                                    |
| `routine.fired`                                                       | A routine trigger fires.                                                                                                                                                    | `routineID`, `runID`, `scheduledAtMs`, `nextFireAtMs`                                    |
| `routine.run.created` / `routine.run.started`                         | A routine run is created / starts.                                                                                                                                          | `runID`, `routineID`, `triggerType`                                                      |
| `routine.run.model_selected`                                          | Model resolved for a routine run.                                                                                                                                           | `runID`, `routineID`, `providerID`, `modelID`, `source`                                  |
| `routine.run.completed` / `routine.run.failed` / `routine.run.paused` | Routine run terminal/pause states.                                                                                                                                          | `runID`, `routineID`, `sessionID`, `finishedAtMs`, `reason`                              |
| `routine.run.approved` / `routine.run.denied` / `routine.run.resumed` | Operator decisions on a gated routine run.                                                                                                                                  | `runID`, `routineID`, `reason`                                                           |
| `routine.run.artifact_added`                                          | A routine run produces an artifact.                                                                                                                                         | `runID`, `routineID`, `artifact`                                                         |
| `routine.approval_required` / `routine.blocked`                       | A routine requires approval / is blocked.                                                                                                                                   | `routineID`, `runID`, `triggerType`, `reason`                                            |
| `routine.tool.denied`                                                 | A routine's tool call is denied.                                                                                                                                            | `sessionID`, `runID`, `routineID`, `tool`                                                |

### Workflows (server-side bindings)

| Event type                                                                         | Fires when                                                                                                     | Key payload fields                                             |
| ---------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `workflow.run.started` / `workflow.run.completed` / `workflow.run.failed`          | A bound workflow run starts / completes / fails.                                                               | `runID`, `workflowID`, `bindingID`, `taskID`, `error`          |
| `workflow.run.awaiting_approval`                                                   | A workflow run pauses on an `approval:gate` action until a human decides via `POST /workflows/runs/{id}/gate`. | `runID`, `workflowID`, `actionID`, `instructions`, `decisions` |
| `workflow.governance.gate_decided`                                                 | A workflow gate decision is recorded (protected audit event).                                                  | `runID`, `workflowID`, `actionID`, `decision`, `decidedBy`     |
| `workflow.action.started` / `workflow.action.completed` / `workflow.action.failed` | A workflow action transitions.                                                                                 | `runID`, `workflowID`, `actionID`, `action`, `taskID`          |
| `workflow_learning.candidate.auto_applied`                                         | A learned workflow improvement is auto-applied.                                                                | `candidate_id`, `workflow_id`, `kind`                          |
| `capabilities.readiness.evaluated`                                                 | Workflow capability readiness evaluated.                                                                       | `workflow_id`, `runnable`, `blocking_issue_count`              |
| `goal_capability_learning.discovered`                                              | Capability paths discovered for a goal.                                                                        | `request_id`, `goal_id`, `confidence`, `paths_found`           |

### Workflow planner

| Event type                                                            | Fires when                          | Key payload fields        |
| --------------------------------------------------------------------- | ----------------------------------- | ------------------------- |
| `workflow_planner.session.started`                                    | A planner session opens.            | plan/session ids          |
| `workflow_planner.draft.updated` / `workflow_planner.draft.validated` | Planner draft changes / validates.  | plan id, validation state |
| `workflow_planner.review.ready`                                       | Draft ready for scope review.       | plan id                   |
| `workflow_planner.approval.requested`                                 | Plan approval requested.            | plan id                   |
| `workflow_planner.capability.blocked`                                 | A required capability is blocked.   | capability, plan id       |
| `workflow_planner.requirements.missing`                               | Plan requirements missing.          | missing list              |
| `workflow_planner.docs_mcp.used`                                      | Docs MCP consulted during planning. | server, query ref         |

### Agent teams & missions

| Event type                                                                                                                       | Fires when                           | Key payload fields                                           |
| -------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ | ------------------------------------------------------------ |
| `agent_team.spawn.requested` / `agent_team.spawn.approved` / `agent_team.spawn.denied`                                           | Team spawn lifecycle.                | `sessionID`, `messageID`, `runID`, `missionID`, `instanceID` |
| `agent_team.instance.started` / `agent_team.instance.completed` / `agent_team.instance.failed` / `agent_team.instance.cancelled` | Team instance lifecycle.             | same ids                                                     |
| `agent_team.budget.usage`                                                                                                        | Periodic budget usage report.        | same ids + `remainingBudget`                                 |
| `agent_team.budget.exhausted` / `agent_team.mission.budget.exhausted`                                                            | Instance / mission budget exhausted. | same ids                                                     |
| `agent_team.capability.denied`                                                                                                   | A team capability is denied.         | same ids                                                     |
| `mission.created` / `mission.updated`                                                                                            | Mission CRUD.                        | `missionID`, `revision`, `status`, `workItemCount`           |

### Coder surface

| Event type                                                                 | Fires when                      | Key payload fields             |
| -------------------------------------------------------------------------- | ------------------------------- | ------------------------------ |
| `coder.run.created` / `coder.run.phase_changed`                            | Coder run lifecycle.            | run id, phase                  |
| `coder.approval.required`                                                  | Coder run requires approval.    | run id, `approval`             |
| `coder.artifact.added`                                                     | Coder run produces an artifact. | run id, `artifact_id`, `title` |
| `coder.pr.submitted` / `coder.merge.submitted` / `coder.merge.recommended` | PR/merge lifecycle.             | run id, PR refs                |
| `coder.memory.candidate_added` / `coder.memory.promoted`                   | Coder memory candidates.        | run id, memory refs            |

### Context governance (packs, tasks, runs)

| Event type                                                                                           | Fires when                        | Key payload fields                                    |
| ---------------------------------------------------------------------------------------------------- | --------------------------------- | ----------------------------------------------------- |
| `context.pack.published` / `context.pack.bound` / `context.pack.revoked` / `context.pack.superseded` | Context pack lifecycle.           | `pack_id`, `title`, `workspace_root`, `binding_count` |
| `context.pack.policy_hook`                                                                           | Pack policy hook fires.           | `action`, `pack_id`                                   |
| `context.task.created` / `context.task.completed` / `context.task.blocked` / `context.task.failed`   | Context task lifecycle.           | `task_id`, `tenantContext`, `automationID`, `runID`   |
| `context.run.failed`                                                                                 | A context run fails.              | `runID`                                               |
| `context.run.stream`                                                                                 | Context run stream payload chunk. | stream payload                                        |

### Memory & knowledge

| Event type                                                                                                                  | Fires when                                            | Key payload fields                                          |
| --------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------------- |
| `memory.context.injected` / `memory.docs.context.injected`                                                                  | Memory / docs context injected into a prompt.         | `runID`, `sessionID`, `messageID`, `iteration`, `count`     |
| `memory.search.performed`                                                                                                   | Memory search runs for a prompt.                      | `runID`, `sessionID`, `providerID`, `modelID`               |
| `memory.context.error`                                                                                                      | Memory context injection fails.                       | `sessionID`, `messageID`, `error`                           |
| `memory.put` / `memory.search` / `memory.promote` / `memory.updated` / `memory.deleted`                                     | Memory store operations (audit).                      | memory key/scope ids                                        |
| `kb.grounding.context.injected`                                                                                             | KB grounding context injected.                        | `runID`, `sessionID`, `iteration`, `strict`                 |
| `kb.grounding.required` / `kb.grounding.strict.applied` / `kb.grounding.strict.direct_answer` / `kb.grounding.strict.error` | Strict grounding lifecycle.                           | session/run ids, grounding state                            |
| `knowledge.preflight.injected`                                                                                              | Knowledge preflight injected into an automation node. | `automationID`, `runID`, `nodeID`, `taskFamily`, `decision` |

### MCP & integrations

| Event type                                                                                       | Fires when                                   | Key payload fields                                |
| ------------------------------------------------------------------------------------------------ | -------------------------------------------- | ------------------------------------------------- |
| `mcp.server.connected` / `mcp.server.updated` / `mcp.server.disconnected` / `mcp.server.deleted` | MCP server lifecycle.                        | `name`, `status`, `removedToolCount`, `reason`    |
| `mcp.tools.updated`                                                                              | MCP toolset refreshed.                       | `name`, `count`, `source`                         |
| `mcp.auth.required` / `mcp.auth.pending`                                                         | MCP server needs (re)authorization.          | `sessionID`, `tool`, `server`, `authorizationUrl` |
| `pack.install.started` / `pack.install.succeeded` / `pack.install.failed`                        | Pack install lifecycle.                      | `source`, `pack_id`, `version`, `error`           |
| `pack.detected`                                                                                  | A pack is detected in an attachment/channel. | `path`, `attachment_id`, `connector`              |
| `pack.update.not_available`                                                                      | Pack update check finds nothing.             | `pack_id`, `current_version`, `reason`            |

### Pack builder

| Event type                                                                                                                                                                                                                                                                       | Fires when                       | Key payload fields                           |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- | -------------------------------------------- |
| `pack_builder.apply_started` / `pack_builder.apply_completed` / `pack_builder.apply_blocked` / `pack_builder.cancelled` / `pack_builder.error` / `pack_builder.preview_ready`                                                                                                    | Pack-builder workflow lifecycle. | `sessionID`, `threadKey`, `planID`, `status` |
| `pack_builder.apply.success` / `pack_builder.apply.cancelled` / `pack_builder.apply.blocked_auth` / `pack_builder.apply.blocked_missing_secrets` / `pack_builder.apply.wrong_plan_prevented` / `pack_builder.apply.count` / `pack_builder.preview.count` / `pack_builder.metric` | Pack-builder metrics.            | `metric`, `value`, `surface`, `planID`       |

### Incident Monitor

| Event type                                                                               | Fires when                       | Key payload fields                                        |
| ---------------------------------------------------------------------------------------- | -------------------------------- | --------------------------------------------------------- |
| `incident_monitor.incident.detected`                                                     | A new incident is detected.      | `incident_id`, `fingerprint`, `draft_id`, `triage_run_id` |
| `incident_monitor.incident.duplicate_suppressed`                                         | Duplicate incident suppressed.   | `incident_id`, `fingerprint`, `duplicate_summary`         |
| `incident_monitor.incident.triage_failed` / `incident_monitor.incident.triage_timed_out` | Triage failure modes.            | incident refs                                             |
| `incident_monitor.triage_run.created`                                                    | A triage run is spawned.         | `draft_id`, `run_id`, `automation_run_id`, `repo`         |
| `incident_monitor.github.issue_created` / `incident_monitor.github.comment_posted`       | GitHub escalation.               | `draft_id`, `issue_number`, `repo`                        |
| `incident_monitor.error`                                                                 | Incident Monitor internal error. | `eventType`, `detail`                                     |

### Enterprise

| Event type                                                                                                   | Fires when                             | Key payload fields                                                       |
| ------------------------------------------------------------------------------------------------------------ | -------------------------------------- | ------------------------------------------------------------------------ |
| `enterprise.source_binding.cache_invalidation_required` / `enterprise.connector.cache_invalidation_required` | Enterprise cache invalidation signals. | `reason`, `tenant_context`, `binding_id` / `connector_id`, `cache_scope` |

## Adoption status

- The envelope is stamped centrally in `EventBus::publish`, so **all**
  emitters — including `tandem-core` engine_loop and `tandem-server`
  automation_v2 — publish the canonical envelope.
- Durable persistence of the event stream ships as the JSONL runtime event
  log with replay (see "Durable event log" above).
- The closed-vocabulary test (`vocabulary_round_trips_and_has_no_duplicates`)
  and this document must be updated together.
