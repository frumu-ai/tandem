# Workflows

Tandem workflows provide a declarative layer for extending agent and automation behavior without changing core engine code.

The v1 implementation adds:

- workflow definitions loaded from YAML
- event-driven hook bindings
- pack and workspace workflow discovery
- workflow run persistence
- workflow simulation and execution APIs

## Architecture Overview

The workflow layer is engine-owned.

- `tandem-workflows` defines schemas, loading, merge rules, and validation.
- `tandem-server` owns runtime execution, event dispatch, persistence, and HTTP APIs.
- the existing Tandem event bus remains the source of truth for triggers and workflow lifecycle events.

Workflow sources are merged in this order:

1. built-in workflow directories
2. installed packs
3. workspace-local `.tandem`

Later sources override earlier workflow definitions with the same `workflow_id`. Hook enablement can be overridden at runtime without editing source files.

## Workflow Definition

Workflow files live under a `workflows/` directory and use YAML.

Example:

```yaml
workflow:
  id: build_feature
  name: Build Feature
  description: Standard feature delivery pipeline
  steps:
    - planner
    - action: task_generator
      with:
        max_tasks: 5
    - executor
    - verifier

  hooks:
    task_created:
      - kanban.update
    task_completed:
      - slack.notify
```

Each step is normalized into a `WorkflowStepSpec`:

- `step_id`
- `action`
- optional `with` payload

Supported action prefixes in v1:

- `capability:<id>`
- `tool:<id>`
- `agent:<id>`
- `workflow:<id>`
- `event:<type>`
- `resource:put:<key>`
- `resource:patch:<key>`
- `resource:delete:<key>`

If no prefix is supplied, Tandem treats the action as `capability:<id>`.

## Event System

The engine maps runtime events to canonical workflow lifecycle names.

Standard names:

- `workflow_started`
- `task_created`
- `task_started`
- `task_completed`
- `task_failed`
- `workflow_completed`

Current built-in mappings include:

- `context.task.created` -> `task_created`
- `context.task.started` -> `task_started`
- `context.task.completed` -> `task_completed`
- `context.task.failed` -> `task_failed`
- `workflow.run.started` -> `workflow_started`
- `workflow.run.completed` -> `workflow_completed`

The workflow runtime emits its own execution events:

- `workflow.run.started`
- `workflow.action.started`
- `workflow.action.completed`
- `workflow.action.failed`
- `workflow.run.completed`
- `workflow.run.failed`

These events are published on the existing Tandem event bus and are available through `/workflows/events`.

## Hook Design

Hook bindings live under `hooks/` or can be embedded inside a workflow file.

Example:

```yaml
hooks:
  - id: build_feature.task_completed.notify
    workflow_id: build_feature
    event: task_completed
    enabled: true
    actions:
      - action: slack.notify
        with:
          channel: engineering
```

The runtime resolves each hook into a `WorkflowHookBinding`:

- `binding_id`
- `workflow_id`
- `event`
- `enabled`
- `actions`
- source provenance

Duplicate dispatch is prevented per `(binding_id, source_event_id)` in memory during the current server process.

## Pack Integration

Installed packs are scanned for:

- `workflows/*.yaml`
- `hooks/*.yaml`

Recommended manifest additions:

```yaml
entrypoints:
  workflows: ["build_feature"]

contents:
  workflows:
    - id: build_feature
      path: workflows/build_feature.yaml
  workflow_hooks:
    - id: build_feature.task_completed.notify
      path: hooks/notify.yaml
```

These keys are additive and intended for pack inspection and UI tooling. The current loader uses the installed pack root on disk, so workflow files are discovered even if the manifest omits those fields.

## Runtime APIs

The workflow HTTP surface currently includes:

- `GET /workflows`
- `GET /workflows/{id}`
- `POST /workflows/validate`
- `POST /workflows/simulate`
- `POST /workflows/{id}/run`
- `GET /workflows/runs`
- `GET /workflows/runs/{id}`
- `GET /workflow-hooks`
- `PATCH /workflow-hooks/{id}`
- `GET /workflows/events`

## Sharing or Reusing a Generated Workflow

Generated workflows can be moved between workspaces as a bundle instead of being rebuilt from scratch. The shareable artifact is `plan_package_bundle`.

The Planner page uses the same bundle-shaped workflow when it hands plans off into Automations, Coding, Orchestrator, or a saved intent. The SDKs expose the same preview/chat/apply/import calls, so external callers can drive the same workflow without going through the control panel UI.

1. Preview or apply a workflow plan.
   - `POST /workflow-plans/preview` returns `plan_package_bundle` alongside `plan`, `plan_package`, and `plan_package_validation`.
   - `POST /workflow-plans/apply` returns the same bundle plus `approved_plan_materialization` for the stored automation snapshot.
2. Copy the bundle into the target workspace or install.
3. Validate the bundle before importing it.
   - `POST /workflow-plans/import/preview` returns `import_validation`, `plan_package_preview`, `derived_scope_snapshot`, and `summary`.
   - Check `import_validation.compatible` before import.
4. Import the bundle.
   - `POST /workflow-plans/import` creates a durable planner session with the imported draft, provenance, and validation state.
   - The imported workflow appears in the Workflow Center and can be reopened from the planner session view.
   - The imported workflow is revisable from the Workflow Center and the planner session view.
   - Import does not apply or arm the workflow; schedule data remains staged until apply.

Example request flow:

```bash
curl -s -X POST http://localhost:4000/workflow-plans/preview \
  -H 'content-type: application/json' \
  -d '{"prompt":"Create a shareable workflow bundle","workspace_root":"/tmp/source"}'
```

The response includes a `plan_package_bundle` field. Copy that value into an import preview request:

```json
{
  "bundle": { "...": "copied from plan_package_bundle" }
}
```

If you are building against the SDK instead of raw HTTP, the same flow is available as `workflowPlans.preview`, `workflowPlans.chatStart`, `workflowPlans.chatMessage`, `workflowPlans.apply`, `workflowPlans.importPreview`, and `workflowPlans.importPlan` in TypeScript, or the snake_case equivalents in Python.

Example SDK flow:

```typescript
const started = await client.workflowPlans.chatStart({
  prompt: "Plan a release workflow with approval and handoff",
  planSource: "intent_planner_page",
  workspaceRoot: "/workspace/repos/tandem",
});

const revised = await client.workflowPlans.chatMessage({
  planId: started.plan.plan_id!,
  message: "Split the work into review, validate, and publish phases.",
});

const applied = await client.workflowPlans.apply({
  planId: revised.plan.plan_id!,
  creatorId: "planner-operator",
});

const previewImport = await client.workflowPlans.importPreview({
  bundle: applied.plan_package_bundle!,
});

if (previewImport.import_validation?.compatible) {
  await client.workflowPlans.importPlan({
    bundle: previewImport.bundle ?? applied.plan_package_bundle!,
  });
}
```

## Creating a Custom Workflow Hook

1. Create `.tandem/workflows/build_feature.yaml`

```yaml
workflow:
  id: build_feature
  name: Build Feature
  steps:
    - action: agent:planner
      with:
        prompt: Plan the feature work.
```

2. Add an embedded hook or a separate `.tandem/hooks/notify.yaml`

```yaml
hooks:
  - id: build_feature.task_completed.notify_user
    workflow_id: build_feature
    event: task_completed
    actions:
      - action: capability:slack.notify
        with:
          channel: engineering
          text: Task completed
```

3. Reload or validate workflows

```bash
curl -X POST http://localhost:4000/workflows/validate \
  -H 'content-type: application/json' \
  -d '{"reload":true}'
```

4. Simulate the trigger

```bash
curl -X POST http://localhost:4000/workflows/simulate \
  -H 'content-type: application/json' \
  -d '{"event_type":"context.task.completed","properties":{"event_id":"demo-task-1"}}'
```

5. Enable or disable the hook without editing YAML

```bash
curl -X PATCH http://localhost:4000/workflow-hooks/build_feature.task_completed.notify_user \
  -H 'content-type: application/json' \
  -d '{"enabled":false}'
```

## Example Pack

See [`examples/packs/workflow_hook_demo`](/home/user123/tandem/examples/packs/workflow_hook_demo) for a minimal pack that adds:

- `task_created -> capability:kanban.update`
- `task_completed -> capability:slack.notify`

## Notes and Current Limits

- Nested `workflow:<id>` actions are parsed but not executed yet in this slice.
- Hook dedupe is process-local and not yet persisted across restarts.
- UI workflow editing and visual workflow inspection are still pending; the current implementation exposes the backend APIs they will build on.
