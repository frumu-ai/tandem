# Channel Lifecycle and Diagnostics

This document summarizes how the channel surface is initialized and managed in v1 of
the registry-backed channel implementation.

## Lifecycle

- `AppState::restart_channel_listeners` rebuilds the runtime listener set from the effective config.
- Channels are discovered from the shared registry (`tandem_channels::registered_channels()`), so
  startup and status/update paths are no longer hardcoded to specific names.
- For each built-in channel, the server stores:
  - `enabled` from config presence.
  - `connected` after startup attempts.
  - A live diagnostics snapshot under `meta`.
- Listener supervision errors are surfaced in `/channels/status` through:
  - `state` (running, retrying, stopped, etc.)
  - `last_error` (string in top-level `ChannelStatus` + in diagnostics)
  - `last_error_code` (`listener_error`, `startup_error`, etc.)
  - `last_reconnect_at` (ms epoch)
  - `listener_start_count`

## Endpoints

- `GET /channels/config`
  - Returns the normalized config snapshot for each registry-built channel.
  - Never returns raw tokens; includes `token_masked`, `has_token`, and channel defaults.
- `GET /channels/status`
  - Returns one object entry per registered channel.
  - Unknown/unsupported channel names are not included.
- `PUT /channels/{name}` and `DELETE /channels/{name}`
  - Validate `{name}` against the registry.
  - Unknown names return `404`.
- `POST /channels/{name}/verify`
  - Currently supports Discord verify checks.
  - Unknown channel names or unsupported channels return `404`.

## Built-in config keys (backward compatible)

The v1 surface remains `telegram`, `discord`, and `slack` in config under `channels`.

- Telegram: `channels.telegram`
  - required for startup: `bot_token`
  - optional: `allowed_users`, `mention_only`, `style_profile`, `model_provider_id`, `model_id`, `security_profile`
  - env fallback: `TANDEM_TELEGRAM_BOT_TOKEN`
- Discord: `channels.discord`
  - required for startup: `bot_token`
  - optional: `guild_id`, `allowed_users`, `mention_only`, `model_provider_id`, `model_id`, `security_profile`
  - env fallback: `TANDEM_DISCORD_BOT_TOKEN`
- Slack: `channels.slack`
  - required for startup: `bot_token`, `channel_id`
  - optional: `allowed_users`, `mention_only`, `model_provider_id`, `model_id`, `security_profile`
  - env fallback: `TANDEM_SLACK_BOT_TOKEN`, `TANDEM_SLACK_CHANNEL_ID`

## Public demo security profile

The older public-channel security-profile board is superseded by the shipped
`security_profile: "public_demo"` runtime profile. Use it for open or lightly
trusted channel demos where the bot can answer, keep public channel-scoped memory,
and manage that user's demo session without exposing operator controls.

`public_demo` enforces a narrow command allowlist:

- Available: `/new`, `/sessions`, `/resume`, `/rename`, `/status`, `/run`, `/cancel`, `/memory`, and `/help`.
- Disabled: approval and gate control (`/answer`, `/approve`, `/deny`, `/pending`, `/rework`), internal queue visibility (`/todos`, `/requests`), model/provider changes, workspace/file access, MCP connector controls, tool-scope overrides, pack install/inspection, runtime config, workflow planning, automation control, and run administration.

The profile also constrains new channel sessions to public/channel-scoped memory
and web search. It omits workspace directories, shell/file permissions, browser
controls, MCP tools, and normal trusted project/global memory. `/help` and topic
help such as `/help workspace`, `/help mcp`, and `/help config` explain that those
capabilities exist for trusted/operator channels but are intentionally blocked in
the public integration.

Example:

```json
{
  "channels": {
    "slack": {
      "bot_token": "...",
      "channel_id": "C0123456789",
      "allowed_users": ["*"],
      "mention_only": true,
      "security_profile": "public_demo"
    }
  }
}
```
