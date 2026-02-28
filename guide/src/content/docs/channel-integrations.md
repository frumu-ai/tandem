---
title: Channel Integrations
description: Telegram, Discord, and Slack channel behavior, media handling, and storage.
---

Tandem channels let users chat with the same engine sessions from Telegram, Discord, or Slack.

## What channels do

1. Receive inbound channel messages.
2. Map user/channel identity to a Tandem session.
3. Send prompt parts (`text`, optionally `file`) to `/session/{id}/prompt_async`.
4. Stream run events and post replies back to the channel.

## Supported channels

- Telegram
- Discord
- Slack

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
  -H "X-Tandem-Token: tk_your_token"
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

## Related docs

- [Headless Service](./headless-service/)
- [Engine Commands](./reference/engine-commands/)
- [TypeScript SDK](./sdk/typescript/)
- [Python SDK](./sdk/python/)
