---
title: Channel Integrations
description: Telegram, Discord, and Slack channel behavior, media handling, and storage.
---

Tandem channels let users chat with the same engine sessions from Telegram, Discord, or Slack.

For the full runtime model behind this behavior, see [How Tandem Works Under the Hood](https://docs.tandem.ac/how-tandem-works/).

## What channels do

1. Receive inbound channel messages.
2. Map user/channel identity to a Tandem session.
3. Send prompt parts (`text`, optionally `file`) to `/session/{id}/prompt_async`.
4. Stream run events and post replies back to the channel.

## Supported channels

- Telegram
- Discord
- Slack

## Slash Commands

Channels expose a lightweight command surface so users can manage sessions and draft automations directly from chat.

### Core commands

- `/help`
- `/new [name]`
- `/sessions`
- `/resume <id or name>`
- `/rename <name>`
- `/status`
- `/run`
- `/cancel`
- `/todos`
- `/requests`
- `/answer <question_id> <text>`
- `/approve <tool_call_id>`
- `/deny <tool_call_id>`

### Model commands

- `/providers`
- `/models [provider]`
- `/model <model_id>`

### Workflow-planning commands

- `/schedule help`
- `/schedule plan <prompt>`
- `/schedule show <plan_id>`
- `/schedule edit <plan_id> <message>`
- `/schedule reset <plan_id>`
- `/schedule apply <plan_id>`

### Operator commands

- `/automations ...`
- `/runs ...`
- `/memory ...`
- `/workspace ...`
- `/mcp ...`
- `/packs ...`
- `/config ...`

`/help schedule` and `/schedule help` both show the workflow-planning guide, and `/schedule` with no extra text defaults to the same help output.
`/help automations`, `/help memory`, `/help workspace`, `/help mcp`, `/help packs`, and `/help config` show the namespaced guides for the new operator commands.

Configure and inspect status with:

- `GET /channels/status`
- `PUT /channels/{name}`
- `DELETE /channels/{name}`

## Media and file uploads

When adapters are configured for media ingestion:

1. Files are stored under engine storage root in `channel_uploads/...`.
2. Stored file references are attached to prompts as `file` parts:
   - `type: "file"`
   - `mime`
   - `filename` (optional)
   - `url` (local path, `file://...`, or remote URL)
3. Prompt also includes user text part (`type: "text"`).

List uploaded files:

```bash
curl -s "http://127.0.0.1:39731/global/storage/files?path=channel_uploads&limit=200" \
  -H "X-Agent-Token: tk_your_token"
```

## Storage layout

Typical pattern:

```text
<state_root>/channel_uploads/<channel>/<chat_or_user>/<timestamp>_<filename>
```

Example:

```text
/srv/tandem/channel_uploads/telegram/667596788/1772310564423_photo_305646779.jpg
```

## Model and media compatibility

- Image-capable providers/models can analyze image `file` parts.
- Non-vision or unsupported models should still complete the run with a fallback response.
- Channel adapters should avoid hanging on unsupported media and return clear user guidance.

## Formatting notes (Telegram)

Telegram outbound formatting should use MarkdownV2-safe rendering:

- Prefer `parse_mode: "MarkdownV2"`
- Escape Telegram-reserved characters outside valid entities
- Keep fallback retry as plain text on Telegram parse errors

## Channel Memory

Channel sessions now keep memory in two layers:

1. **Raw transcript history** stays in normal Tandem session storage.
2. **Retrieval memory** stores exact user-visible completed user+assistant exchanges in global memory.

This lets future channel sessions recall prior work without loading full transcript history into every run.

For the storage-level breakdown of these layers, see [Memory Internals](https://docs.tandem.ac/memory-internals/).

### What gets archived

After a successful channel reply, Tandem writes one `chat_exchange` memory entry containing:

- the latest user message
- the latest user-visible assistant reply
- provenance such as session ID, project ID, workspace root, and message IDs

The engine dedupes retries of the same exchange automatically.

### Why this stays small and safe

- Tandem archives one completed exchange, not every partial stream event.
- Raw session history remains the source of truth even if retrieval memory is compacted or re-ranked later.
- Prompt context still stays bounded by normal memory search limits rather than unbounded chat log replay.

For the engine-side explanation of this layering, see [How Tandem Works Under the Hood](https://docs.tandem.ac/how-tandem-works/).

## Workflow planning from channels

The `/schedule` command family is a thin wrapper over Tandem's workflow-plan endpoints.

- `/schedule plan <prompt>` creates a workflow draft from a natural-language request.
- `/schedule edit <plan_id> <message>` revises the draft conversationally.
- `/schedule apply <plan_id>` saves the draft as an automation.

When the current channel session is already bound to a workspace, Tandem automatically forwards that workspace root into the planner so the resulting workflow targets the right repo or project by default.

## Expanded operator commands

Channels now support a broader operator surface on top of existing engine APIs:

- `/automations` to list, inspect, run, pause, resume, and delete saved automations
- `/runs` to inspect recent automation runs and their artifacts
- `/memory` to search, review, save, and delete memory entries
- `/workspace` to inspect the current workspace binding, search files, and check the git branch
- `/mcp`, `/packs`, and `/config` for integration/runtime inspection and lightweight control

Destructive commands require `--yes`, while read/list/show/search commands execute immediately.

## Related docs

- [Headless Service](./headless-service/)
- [Engine Commands](./reference/engine-commands/)
- [TypeScript SDK](./sdk/typescript/)
- [Python SDK](./sdk/python/)
