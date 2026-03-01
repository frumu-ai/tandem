---
title: Python SDK
description: "tandem-client — Python client for the Tandem engine"
---

## Install

```bash
pip install tandem-client
```

Requires **Python 3.10+**.

## Engine prerequisite

The SDK talks to a running `tandem-engine` over HTTP/SSE. Install and start the engine first:

```bash
npm install -g @frumu/tandem
tandem-engine serve --api-token "$(tandem-engine token generate)"
```

Then pass the same token into `TandemClient(base_url=..., token=...)`.

## Quick start

```python
import asyncio
from tandem_client import TandemClient

async def main():
    async with TandemClient(
        base_url="http://localhost:39731",
        token="your-engine-token",  # tandem-engine token generate
    ) as client:
        # 1. Create a session
        session_id = await client.sessions.create(
            title="My agent",
            directory="/path/to/project",
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
client.close()
```

:::caution
`stream()` and `global_stream()` are async-only. Use `TandemClient` to receive streamed events.
:::

## TandemClient

```python
TandemClient(base_url, token, *, timeout=20.0)
```

### Top-level methods

| Method                                           | Returns                       | Description              |
| ------------------------------------------------ | ----------------------------- | ------------------------ |
| `await health()`                                 | `SystemHealth`                | Check engine readiness   |
| `stream(session_id, run_id?)`                    | `AsyncGenerator[EngineEvent]` | Stream events from a run |
| `global_stream()`                                | `AsyncGenerator[EngineEvent]` | Stream all engine events |
| `await run_events(run_id, *, since_seq?, tail?)` | `list[EngineEvent]`           | Pull stored run events   |
| `await list_tool_ids()`                          | `list[str]`                   | List all tool IDs        |
| `await list_tools()`                             | `list[ToolSchema]`            | List tools with schemas  |
| `await execute_tool(tool, args?)`                | `ToolExecuteResult`           | Execute a tool directly  |

---

### `client.sessions`

| Method                                                          | Description                                   |
| --------------------------------------------------------------- | --------------------------------------------- |
| `create(*, title?, directory?, provider?, model?)`              | Create a session, returns `session_id`        |
| `list(*, q?, page?, page_size?, archived?, scope?, workspace?)` | List sessions                                 |
| `get(session_id)`                                               | Get session details                           |
| `update(session_id, *, title?, archived?)`                      | Update title or archive status                |
| `archive(session_id)`                                           | Archive a session                             |
| `delete(session_id)`                                            | Permanently delete                            |
| `messages(session_id)`                                          | Full message history                          |
| `todos(session_id)`                                             | Pending TODOs                                 |
| `active_run(session_id)`                                        | Currently active run                          |
| `prompt_async(session_id, prompt)`                              | Start async run → `PromptAsyncResult(run_id)` |
| `prompt_sync(session_id, prompt)`                               | Blocking prompt → reply `str`                 |
| `abort(session_id)`                                             | Abort the active run                          |
| `cancel(session_id)`                                            | Cancel the active run                         |
| `cancel_run(session_id, run_id)`                                | Cancel a specific run                         |
| `fork(session_id)`                                              | Fork into a child session                     |
| `diff(session_id)`                                              | Workspace diff from last run                  |
| `revert(session_id)`                                            | Revert uncommitted changes                    |
| `unrevert(session_id)`                                          | Undo a revert                                 |
| `children(session_id)`                                          | List forked child sessions                    |
| `summarize(session_id)`                                         | Trigger conversation summarization            |
| `attach(session_id, target_workspace)`                          | Re-attach to a different workspace            |

#### Prompt with file parts

Use a direct engine call when you need mixed `parts` payloads:

```python
import httpx

payload = {
    "parts": [
        {
            "type": "file",
            "mime": "image/png",
            "filename": "diagram.png",
            "url": "/srv/tandem/channel_uploads/telegram/667596788/diagram.png",
        },
        {"type": "text", "text": "Explain this diagram in plain English."},
    ]
}

async with httpx.AsyncClient(base_url="http://localhost:39731") as http:
    resp = await http.post(
        f"/session/{session_id}/prompt_async?return=run",
        headers={"Authorization": f"Bearer {token}"},
        json=payload,
    )
    run = resp.json()
```

`file` part shape:

- `type`: `"file"`
- `mime`: MIME type string
- `filename`: optional display filename
- `url`: HTTP URL, local path, or `file://...`

### `client.permissions`

```python
snapshot = await client.permissions.list()
for req in snapshot.requests:
    await client.permissions.reply(req.id, "always")
```

### `client.questions`

```python
qs = await client.questions.list()
for q in qs.questions:
    await client.questions.reply(q.id, "yes")
    # or: await client.questions.reject(q.id)
```

### `client.providers`

```python
catalog = await client.providers.catalog()
await client.providers.set_defaults("openrouter", "anthropic/claude-3.7-sonnet")
await client.providers.set_api_key("openrouter", "sk-or-...")
status = await client.providers.auth_status()
```

### `client.identity`

```python
identity = await client.identity.get()

await client.identity.patch(
    {
        "identity": {
            "bot": {"canonical_name": "Ops Assistant"},
            "personality": {
                "default": {
                    "preset": "concise",
                    "custom_instructions": "Prioritize deployment safety and rollback clarity.",
                }
            },
        }
    }
)
```

Built-in presets include: `balanced`, `concise`, `friendly`, `mentor`, `critical`.

### `client.channels`

```python
await client.channels.put("telegram", {
    "token": "bot:xxx",
    "allowed_users": ["@yourhandle"],
})
status = await client.channels.status()
print(status.telegram.connected)
```

### `client.mcp`

```python
await client.mcp.add("arcade", "https://mcp.arcade.ai/mcp")
await client.mcp.connect("arcade")
tools = await client.mcp.list_tools()
resources = await client.mcp.list_resources()
await client.mcp.set_enabled("arcade", False)
```

### `client.memory`

```python
# Store (global record; SDK `text` maps to server `content`)
await client.memory.put(
    "The team uses Rust for all backend services.",
    run_id="run-abc",
)

# Search
result = await client.memory.search("backend technology choices", limit=5)
for item in result.results:
    print(getattr(item, "content", None) or item.text, item.score)

# List, promote, demote, delete
listing = await client.memory.list(q="architecture", user_id="user-123")
await client.memory.promote(listing.items[0].id)
await client.memory.demote(listing.items[0].id, run_id="run-abc")
await client.memory.delete(listing.items[0].id)

# Audit
log = await client.memory.audit(run_id="run-abc")
```

### `client.skills`

```python
listing = await client.skills.list()
skill = await client.skills.get("security-auditor")
templates = await client.skills.templates()

await client.skills.import_skill(
    location="workspace",
    content=yaml_string,
    conflict_policy="overwrite",
)
```

### `client.resources`

```python
await client.resources.write(
    "agent-config/alert-threshold",
    {"threshold": 0.95},
)
listing = await client.resources.list(prefix="agent-config/")
await client.resources.delete("agent-config/alert-threshold")
```

### `client.routines`

```python
await client.routines.create({
    "name": "Daily digest",
    "schedule": "0 8 * * *",
    "entrypoint": "Summarize today's activity and write to daily-digest.md",
    "requires_approval": False,
})

runs = await client.routines.list_runs(limit=10)
await client.routines.approve_run(runs[0]["id"])
await client.routines.pause_run(run_id)
await client.routines.resume_run(run_id)
```

### `client.automations`

```python
await client.automations.create({
    "name": "Weekly security scan",
    "schedule": "0 9 * * 1",
    "mission": {
        "objective": "Audit the API for vulnerabilities",
        "success_criteria": ["Report written to reports/security.md"],
    },
    "policy": {
        "tool": {"external_integrations_allowed": False},
        "approval": {"requires_approval": True},
    },
})

run = await client.automations.get_run(run_id)
await client.automations.approve_run(run_id, "LGTM")
```

### `client.agent_teams`

```python
templates = await client.agent_teams.list_templates()
instances = await client.agent_teams.list_instances(status="active")

result = await client.agent_teams.spawn(
    role="builder",
    justification="Implementing feature X",
    mission_id="mission-123",
)

approvals = await client.agent_teams.list_approvals()
await client.agent_teams.approve_spawn(approvals.spawnApprovals[0].approvalID)
```

### `client.missions`

```python
resp = await client.missions.create(
    title="Q1 Security Hardening",
    goal="Audit and fix all critical security issues",
    work_items=[
        {"title": "Audit auth middleware", "assigned_agent": "security-auditor"},
    ],
)

full = await client.missions.get(resp.mission.id)
await client.missions.apply_event(resp.mission.id, {"type": "work_item.completed"})
```
