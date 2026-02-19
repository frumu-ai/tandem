---
title: Channel Integrations (Telegram, Discord, Slack)
description: Connect Tandem AI sessions to Telegram, Discord, and Slack — in both desktop and headless modes.
---

Route messages from Telegram, Discord, and Slack directly into Tandem AI sessions.
Every user gets a persistent session that survives server restarts.

## How It Works

```
User sends message in Telegram / Discord / Slack
        ↓
tandem-channels dispatcher
        ↓  (creates or resumes session)
POST /session/{id}/prompt_sync
        ↓
Tandem LLM engine responds
        ↓
Reply delivered back to the channel
```

Each `{channel}:{sender}` pair maps to one Tandem session, persisted to
`channel_sessions.json` so context is never lost across restarts.

---

## Setup: Desktop App

### Recommended: GUI Setup (Settings -> Connections)

Use the desktop UI to configure channel adapters per active project:

1. Open **Settings**.
2. Open the **Connections** tab.
3. Configure Telegram, Discord, or Slack fields.
4. Click **Enable**.

Notes:

- Bot tokens are stored in the encrypted desktop vault (not plaintext config files).
- **Disable** keeps the saved token for quick re-enable.
- **Forget token** removes the stored token.

### Advanced: Environment Variables

The desktop app runs the engine as a sidecar. You can also set environment variables
before launching the app:

import { Tabs, TabItem } from '@astrojs/starlight/components';

<Tabs>
<TabItem label="Telegram">
```bash
TANDEM_TELEGRAM_BOT_TOKEN="7123456789:AAF..."
TANDEM_TELEGRAM_ALLOWED_USERS="@alice,@bob"   # or * for anyone
TANDEM_SERVER_BASE_URL="http://localhost:39731"
TANDEM_API_TOKEN="your-engine-api-token"
```

Then launch Tandem normally. The sidecar engine picks these up at startup.

</TabItem>
<TabItem label="Discord">
```bash
TANDEM_DISCORD_BOT_TOKEN="MTIz..."
TANDEM_DISCORD_GUILD_ID="123456789"        # optional: restrict to one server
TANDEM_DISCORD_ALLOWED_USERS="alice,bob"  # Discord usernames, or *
TANDEM_SERVER_BASE_URL="http://localhost:39731"
TANDEM_API_TOKEN="your-engine-api-token"
```
</TabItem>
<TabItem label="Slack">
```bash
TANDEM_SLACK_BOT_TOKEN="xoxb-..."
TANDEM_SLACK_CHANNEL_ID="C0EXAMPLE"       # channel to listen in
TANDEM_SLACK_ALLOWED_USERS="U01A,U02B"   # Slack user IDs, or *
TANDEM_SERVER_BASE_URL="http://localhost:39731"
TANDEM_API_TOKEN="your-engine-api-token"
```
</TabItem>
</Tabs>

> **How to find your API token:** Open the desktop app → Settings → Engine Token,
> or run `tandem-engine token show` in a terminal.

---

## Setup: Headless Server

When running without the desktop app, channels activate alongside the HTTP API:

```bash
TANDEM_TELEGRAM_BOT_TOKEN="7123456789:AAF..."
TANDEM_TELEGRAM_ALLOWED_USERS="@alice"
TANDEM_SERVER_BASE_URL="http://localhost:39731"
TANDEM_API_TOKEN="your-token"

tandem-engine serve \
  --hostname 0.0.0.0 \
  --port 39731 \
  --api-token your-token
```

The channels start automatically alongside the HTTP server.
If no bot token env vars are set, channel adapters are simply skipped — the
server starts normally.

---

## Slash Commands

Available in all channels:

| Command                 | Description                  |
| ----------------------- | ---------------------------- |
| `/help`                 | List all commands            |
| `/new [name]`           | Start a fresh session        |
| `/sessions`             | List your recent sessions    |
| `/resume <id or title>` | Switch to a previous session |
| `/status`               | Show current session info    |
| `/rename <name>`        | Rename the current session   |

---

## Allowlist

Control who can use Tandem via the channel:

```bash
# Specific users only
TANDEM_TELEGRAM_ALLOWED_USERS="@alice,@bob"

# Everyone (bots excluded automatically)
TANDEM_TELEGRAM_ALLOWED_USERS="*"

# No one (effectively disables the adapter)
TANDEM_TELEGRAM_ALLOWED_USERS=""
```

---

## Session Persistence

The channel→session mapping is saved to:

- **Linux/macOS:** `~/.local/share/tandem/channel_sessions.json`
- **Windows:** `%USERPROFILE%\.local\share\tandem\channel_sessions.json`
- **Custom:** Set `TANDEM_STATE_DIR=/your/path`

---

## Environment Variable Reference

| Variable                        | Required        | Description                                         |
| ------------------------------- | --------------- | --------------------------------------------------- |
| `TANDEM_TELEGRAM_BOT_TOKEN`     | For Telegram    | Bot token from [@BotFather](https://t.me/BotFather) |
| `TANDEM_TELEGRAM_ALLOWED_USERS` | No              | Comma-separated usernames or `*`                    |
| `TANDEM_DISCORD_BOT_TOKEN`      | For Discord     | Bot token from Discord Developer Portal             |
| `TANDEM_DISCORD_GUILD_ID`       | No              | Restrict to a specific server                       |
| `TANDEM_DISCORD_ALLOWED_USERS`  | No              | Comma-separated usernames or `*`                    |
| `TANDEM_SLACK_BOT_TOKEN`        | For Slack       | `xoxb-...` token                                    |
| `TANDEM_SLACK_CHANNEL_ID`       | For Slack       | Channel to listen in (e.g. `C0EXAMPLE`)             |
| `TANDEM_SLACK_ALLOWED_USERS`    | No              | Comma-separated Slack user IDs or `*`               |
| `TANDEM_SERVER_BASE_URL`        | Yes             | Where the engine HTTP API is running                |
| `TANDEM_API_TOKEN`              | If auth enabled | Engine API token                                    |
| `TANDEM_STATE_DIR`              | No              | Override session map storage path                   |

---

## See Also

- [Headless Service](./headless-service/)
- [Configuration](./configuration/)
- [Agents and Sessions](./agents-and-sessions/)
