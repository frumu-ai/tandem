# tandem-client

Python client for the [Tandem](https://tandem.ac/) autonomous agent engine HTTP + SSE API.

## Install

```bash
pip install tandem-client
```

Python 3.10+ required.

## Quick start

```python
import asyncio
from tandem_client import TandemClient

async def main():
    async with TandemClient(
        base_url="http://localhost:39731",
        token="your-engine-token",     # from `tandem-engine token generate`
    ) as client:
        # 1. Create a session
        session_id = await client.sessions.create(
            title="My agent",
            directory="/path/to/my/project",
        )

        # 2. Start an async run
        run = await client.sessions.prompt_async(
            session_id, "Summarize the README and list the top 3 TODOs"
        )

        # 3. Stream the response
        async for event in client.stream(session_id, run.run_id):
            if event.type == "session.response":
                print(event.properties.get("delta", ""), end="", flush=True)
            if event.type in ("run.complete", "run.completed", "run.failed", "session.run.finished"):
                break

asyncio.run(main())
```

## Sync usage (scripts)

```python
from tandem_client import SyncTandemClient

client = SyncTandemClient(base_url="http://localhost:39731", token="...")
session_id = client.sessions.create(title="My agent")
run = client.sessions.prompt_async(session_id, "Analyze this folder")
print(f"Run started: {run.run_id}")
# Note: stream() is async-only; use the async client to receive events
client.close()
```

## API

### `TandemClient(base_url, token, *, timeout=20.0)`

Use as an async context manager or call `await client.aclose()` manually.

| Method                               | Description                      |
| ------------------------------------ | -------------------------------- |
| `await client.health()`              | Check engine readiness           |
| `client.stream(session_id, run_id?)` | Async generator of `EngineEvent` |
| `client.global_stream()`             | Stream all engine events         |
| `await client.list_tool_ids()`       | List all registered tool IDs     |
| `await client.list_tools()`          | List tool schemas                |
| `await client.execute_tool(...)`     | Execute a specific engine tool   |

---

### `client.sessions`

| Method                                                       | Description                                          |
| ------------------------------------------------------------ | ---------------------------------------------------- |
| `create(title?, directory?, provider?, model?)`              | Create session, returns `session_id`                 |
| `list(q?, page?, page_size?, archived?, scope?, workspace?)` | List sessions                                        |
| `get(session_id)`                                            | Get session details                                  |
| `delete(session_id)`                                         | Delete a session                                     |
| `messages(session_id)`                                       | Get message history                                  |
| `active_run(session_id)`                                     | Get active run state                                 |
| `prompt_async(session_id, prompt)`                           | Start async run, returns `PromptAsyncResult(run_id)` |
| `prompt_async_parts(session_id, parts)`                      | Start async run with text/file parts                 |

**Prompt with file attachments:**

```python
run = await client.sessions.prompt_async_parts(
    session_id,
    [
        {
            "type": "file",
            "mime": "image/png",
            "filename": "diagram.png",
            "url": "/srv/tandem/channel_uploads/telegram/667596788/diagram.png",
        },
        {"type": "text", "text": "Explain this diagram."},
    ],
)
```

**Prompt with browser tools enabled for QA:**

```python
run = await client.sessions.prompt_async(
    session_id,
    "Run QA on the target web app. Capture screenshots for failures.",
    tool_mode="required",
    tool_allowlist=[
        "browser_status",
        "browser_open",
        "browser_navigate",
        "browser_snapshot",
        "browser_click",
        "browser_type",
        "browser_press",
        "browser_wait",
        "browser_extract",
        "browser_screenshot",
        "browser_close",
    ],
)
```

### Browser automation via tools

The SDK exposes two different browser paths:

- `client.browser` for readiness, install, and smoke-test flows
- `client.execute_tool(...)` or session runs for actual browser automation

The browser namespace does not currently wrap `open`, `click`, `type`, `extract`, or `screenshot`. Those actions are exposed as standard engine tools.

```python
status = await client.execute_tool("browser_status", {})

opened = await client.execute_tool("browser_open", {
    "url": "https://example.com",
})
session_id = opened.output["session_id"]

snapshot = await client.execute_tool("browser_snapshot", {
    "session_id": session_id,
    "include_screenshot": True,
})

html = await client.execute_tool("browser_extract", {
    "session_id": session_id,
    "format": "html",
})

await client.execute_tool("browser_close", {
    "session_id": session_id,
})
```

Use this pattern for agents that need to open pages, click elements, enter text, wait for content, extract page HTML or text, and capture screenshots through the engine.

### `client.routines`

| Method                            | Description                   |
| --------------------------------- | ----------------------------- |
| `list(family?)`                   | List routines or automations  |
| `create(options, family?)`        | Create a scheduled routine    |
| `delete(routine_id, family?)`     | Delete a routine              |
| `run_now(routine_id, family?)`    | Trigger a routine immediately |
| `list_runs(family?, limit?)`      | List recent run records       |
| `list_artifacts(run_id, family?)` | List run artifacts            |

**Create a cron routine:**

```python
await client.routines.create({
    "name": "Daily digest",
    "schedule": "0 8 * * *",
    "prompt": "Summarize today's activity and write a report to daily-digest.md",
    "allowed_tools": ["read", "write", "websearch"],
})
```

### `client.automations_v2`

```python
automation = await client.automations_v2.create({
    "name": "Daily Marketing Engine",
    "status": "active",
    "schedule": {
        "type": "interval",
        "interval_seconds": 86400,
        "timezone": "UTC",
        "misfire_policy": "run_once",
    },
    "agents": [
        {
            "agent_id": "research",
            "display_name": "Research",
            "model_policy": {
                "default_model": {
                    "provider_id": "openrouter",
                    "model_id": "openai/gpt-4o-mini",
                }
            },
            "tool_policy": {"allowlist": ["read", "websearch"], "denylist": []},
            "mcp_policy": {"allowed_servers": []},
        }
    ],
    "flow": {
        "nodes": [
            {"node_id": "market-scan", "agent_id": "research", "objective": "Find 3 trend signals."}
        ]
    },
})
run = await client.automations_v2.run_now(automation.automation_id or "")
```

### `client.workflow_plans`

```python
preview = await client.workflow_plans.preview(
    prompt="Create a release checklist automation",
    plan_source="planner_page",
)

started = await client.workflow_plans.chat_start(
    prompt="Create a release checklist automation",
    plan_source="planner_page",
)

updated = await client.workflow_plans.chat_message(
    plan_id=started.plan.plan_id or "",
    message="Add a smoke-test step before rollout.",
)

applied = await client.workflow_plans.apply(
    plan_id=updated.plan.plan_id,
    creator_id="operator-1",
)

import_preview = await client.workflow_plans.import_preview(
    bundle=applied.plan_package_bundle,
)

imported = await client.workflow_plans.import_plan(
    bundle=import_preview.bundle or applied.plan_package_bundle,
)
```

### Planner page workflow

The Planner page uses the same `workflow_plans` surface and keeps the plan bundle as the portable artifact. A minimal end-to-end flow looks like this:

```python
started = await client.workflow_plans.chat_start(
    prompt="Plan a release workflow with approval and handoff",
    plan_source="intent_planner_page",
    workspace_root="/workspace/repos/tandem",
)

revised = await client.workflow_plans.chat_message(
    plan_id=started.plan.plan_id or "",
    message="Split the work into review, validate, and publish phases.",
)

applied = await client.workflow_plans.apply(
    plan_id=revised.plan.plan_id or "",
    creator_id="planner-operator",
)

preview_import = await client.workflow_plans.import_preview(
    bundle=applied.plan_package_bundle,
)

if preview_import.import_validation.get("compatible"):
    await client.workflow_plans.import_plan(
        bundle=preview_import.bundle or applied.plan_package_bundle,
    )
```

Use this flow when you want the same governed bundle the control-panel Planner page hands to Automations, Coding, and Orchestrator.

### Additional namespaces

The Python SDK already includes the newer engine surfaces that have landed across the repo:

- `client.browser` for `status()`, `install()`, and `smoke_test()`
- `client.storage` for storage file inspection and legacy repair scan helpers
- `client.workflows` for workflow registry, runs, hooks, simulation, and live events
- `client.resources` for key-value resources
- `client.skills` for list/get/import plus preview, templates, validation, routing, evals, compile, and generate flows
- `client.packs` and `client.capabilities` for pack lifecycle and capability resolution
- `client.automations_v2`, `client.bug_monitor`, `client.coder`, `client.agent_teams`, and `client.missions` for newer orchestration APIs
- For the Bug Monitor flow, see [Bug Monitor And Issue Reporter](https://docs.tandem.ac/reference/bug-monitor/)

```python
browser = await client.browser.status()
storage_files = await client.storage.list_files(path="data/context-runs", limit=100)
workflows = await client.workflows.list()
resources = await client.resources.list(prefix="agent-config/")
templates = await client.skills.templates()
```

Storage archive cleanup and root JSON migration are local maintenance operations. Run them with the engine CLI, for example `tandem-engine storage cleanup --dry-run --context-runs --json`.

### `client.coder`

`client.coder` also includes project-scoped GitHub Project intake helpers:

```python
await client.coder.put_project_binding("repo-123", {
    "github_project_binding": {
        "owner": "acme-inc",
        "project_number": 7,
        "repo_slug": "acme-inc/tandem",
    }
})

inbox = await client.coder.get_project_github_inbox("repo-123")
intake = await client.coder.intake_project_item("repo-123", {
    "project_item_id": inbox.items[0].project_item_id,
    "source_client": "sdk_test",
})
```

### `client.agent_teams` template management

```python
await client.agent_teams.create_template({"templateID": "marketing-writer", "role": "worker"})
await client.agent_teams.update_template("marketing-writer", {"system_prompt": "Write concise copy."})
await client.agent_teams.delete_template("marketing-writer")
```

### `client.mcp`

```python
await client.mcp.add(
    "arcade",
    "https://mcp.arcade.ai/mcp",
    allowed_tools=["search", "search_docs"],
)
await client.mcp.connect("arcade")
tools = await client.mcp.list_tools()
await client.mcp.patch("arcade", allowed_tools=["search"])
await client.mcp.patch("arcade", clear_allowed_tools=True)
```

| Method                                                           | Description                 |
| ---------------------------------------------------------------- | --------------------------- |
| `list()`                                                         | List registered MCP servers |
| `list_tools()`                                                   | List discovered tools       |
| `add(name, transport, *, headers?, enabled?, allowed_tools?)`    | Register an MCP server      |
| `patch(name, *, enabled?, allowed_tools?, clear_allowed_tools?)` | Update MCP server settings  |
| `connect(name)`                                                  | Connect and discover tools  |
| `disconnect(name)`                                               | Disconnect                  |
| `refresh(name)`                                                  | Re-discover tools           |
| `set_enabled(name, enabled)`                                     | Enable/disable              |

### `client.channels`

```python
await client.channels.put("discord", {
    "bot_token": "bot:xxx",
    "guild_id": "1234567890",
    "security_profile": "public_demo",
})
status = await client.channels.status()
config = await client.channels.config()
prefs = await client.channels.tool_preferences("discord")
await client.channels.set_tool_preferences("discord", {"disabled_tools": ["webfetch_html"]})
verification = await client.channels.verify("discord")
print(status.discord.connected)
print(config.discord.security_profile)
print(prefs.enabled_tools)
print(verification.ok)
```

### `client.packs`

```python
packs = await client.packs.list()
detected = await client.packs.detect(path="/tmp/my-pack.zip")
if detected.get("is_pack"):
    await client.packs.install(path="/tmp/my-pack.zip", source={"kind": "local"})
```

### `client.capabilities`

```python
bindings = await client.capabilities.get_bindings()
discovery = await client.capabilities.discovery()
resolution = await client.capabilities.resolve(
    {
        "workflow_id": "wf-pr",
        "required_capabilities": ["github.create_pull_request"],
    }
)
```

### `client.permissions`

```python
snapshot = await client.permissions.list()
for req in snapshot.requests:
    await client.permissions.reply(req.id, "allow")
```

### `client.memory`

```python
# Put (SDK accepts `text`; server persists global `content`)
await client.memory.put(
    "Use WAL mode for sqlite in long-lived services.",
    run_id="run-123",
)

# Search
result = await client.memory.search("sqlite wal", limit=5)

# List by user scope
listing = await client.memory.list(user_id="user-123", q="sqlite")

# Audit
audit = await client.memory.audit(run_id="run-123")

# Promote / demote / delete
await client.memory.promote(listing.items[0].id)
await client.memory.demote(listing.items[0].id, run_id="run-123")
await client.memory.delete(listing.items[0].id)
```

### `client.providers`

```python
catalog = await client.providers.catalog()
await client.providers.set_defaults("openrouter", "anthropic/claude-3.7-sonnet")
await client.providers.set_api_key("openrouter", "sk-or-...")
```

---

## Common event types

| `event.type`              | Description                                   |
| ------------------------- | --------------------------------------------- |
| `session.response`        | Text delta in `event.properties["delta"]`     |
| `session.tool_call`       | Tool invocation                               |
| `session.tool_result`     | Tool result                                   |
| `run.complete`            | Run finished successfully (legacy event name) |
| `run.completed`           | Run finished successfully                     |
| `run.failed`              | Run failed                                    |
| `session.run.finished`    | Session-scoped terminal run event             |
| `permission.request`      | Approval needed                               |
| `memory.write.succeeded`  | Memory write persisted                        |
| `memory.search.performed` | Memory retrieval telemetry                    |
| `memory.context.injected` | Prompt context injection telemetry            |

## License

MIT
