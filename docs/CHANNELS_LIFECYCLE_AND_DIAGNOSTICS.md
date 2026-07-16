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
  - Discord: token/gateway/intent checks.
  - Slack (TAN-766): verifies every resolved connection against the live
    installation — `auth.test` token check, team binding, and (when `app_id`
    is configured) `bots.info` app binding — returning per-connection
    `{channel_id, ok, token_ok, team_ok, app_ok, error}` rows plus an
    aggregate `ok`.
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
  - optional: `allowed_users`, `mention_only`, `model_provider_id`, `model_id`, `security_profile`,
    `team_id`, `app_id`, `signing_secret`, `events_enabled`, `tenant`, `require_approval_step_up`,
    `api_base_url`, `org_units`, `notify_approvals`, `connections`
  - env fallback: `TANDEM_SLACK_BOT_TOKEN`, `TANDEM_SLACK_CHANNEL_ID`

## Slack channel connections (TAN-763)

`channels.slack.connections` turns the Slack surface into a set of per-channel
connections instead of a single bound channel. Each entry names a `channel_id`
and may override any top-level field (`team_id`, `app_id`, `bot_token`,
`signing_secret`, `events_enabled`, `tenant`, `allowed_users`, `mention_only`,
`strict_kb_grounding`, `model_provider_id`/`model_id`, `security_profile`,
`require_approval_step_up`, `api_base_url`, `org_units`, `notify_approvals`);
anything unset inherits the top-level value, so a workspace-wide app declares
its installation identity and secrets once:

```json
{
  "channels": {
    "slack": {
      "bot_token": "xoxb-…",
      "team_id": "T0123456789",
      "app_id": "A0123456789",
      "signing_secret": "…",
      "events_enabled": true,
      "tenant": { "org_id": "acme", "workspace_id": "hq" },
      "connections": [
        { "channel_id": "C_SALES", "allowed_users": ["U_SALES1", "U_SALES2"] },
        { "channel_id": "C_ENG", "allowed_users": ["U_ENG1"] }
      ]
    }
  }
}
```

Semantics:

- **Routing.** Signed Events and interaction callbacks resolve their connection
  by the payload's `(team_id, api_app_id, channel_id)`. Events from a channel no
  connection claims are rejected and audited, exactly like the legacy
  single-channel mismatch.
- **Legacy shape unchanged.** Without `connections`, the top-level object
  resolves as one connection — behavior, error messages, and audits are
  identical to before. When `connections` is present, a non-empty top-level
  `channel_id` still defines a connection of its own (an entry with the same
  `channel_id` overrides it).
- **Per-connection authorization.** `allowed_users`, `security_profile`,
  `require_approval_step_up`, and the `tenant` binding apply per connection;
  a sender allowlisted in one channel has no standing in another.
- **Governed Slack is Events-only (TAN-762, converged).** Any events-capable
  connection (`events_enabled` + `signing_secret`) disables the legacy history
  poller for Slack entirely, so the two ingress modes never double-process.
  Beyond that, a Slack config that carries a governed binding — a `tenant`
  binding (GOV-B5c) or `org_units` departments (TAN-764) — **fails closed**
  when it is not events-capable: the poller refuses to start (warn code
  `slack_governed_requires_events`) rather than running those bindings
  through an ingress path with no per-sender verified identity. The poller
  remains available only for unbound local/demo polling (single top-level
  `channel_id`, one shared static identity).
- **Approvals.** Every connection with `notify_approvals` enabled (the default)
  receives approval cards; card edits route by the recorded recipient channel.
  Until per-run routing lands (TAN-764), multi-connection deployments that must
  not broadcast approvals across departments should set
  `"notify_approvals": false` on the channels that shouldn't receive them.
- **Department binding (TAN-764).** `org_units` on a connection (or top-level,
  inherited) binds the channel to departments. On signed Events ingress the
  run's authority becomes the **intersection** of the sender's active org-unit
  memberships and the channel's bound units — roles, grants, tool capabilities,
  and the strict memory projection all derive from the intersected set, so the
  channel can only narrow authority, never widen it. An empty intersection
  fails closed with an audited denial naming both inputs; `run.started` audit
  events record `channel_org_units` alongside the effective `org_units`.
  Entries match a unit's principal id (`department/engineering`) or bare unit
  id (`engineering`). On the interactions (approval button) path, a
  department-bound connection additionally requires the approver to hold an
  active membership in a bound unit; a department binding without a `tenant`
  binding is a misconfiguration and fails closed.
- **Sender discovery (TAN-765).** `GET /channels/slack/senders` aggregates
  recently seen Slack senders from the protected audit ledger
  (`channel.slack.ingress.accepted`/`.denied`): per sender it returns the
  exact principal string (`channel:slack:{team}:{app}:{user}`), the channels
  they were seen in, accepted/denied counts, the latest denial reason, and
  mapping state. `mapped` is computed **per observed channel**
  (`channel_access[]` rows carry `channel_id`, `bound_org_units`, `mapped`,
  `configured`): a department-bound channel counts as mapped only when the
  sender belongs to one of *its* bound units — the same gate the run-time
  intersection enforces — so an engineering membership never masks a
  sales-channel denial. Denial audits are attributed to the matched
  connection's bound tenant (falling back to the top-level binding, then to
  the single unambiguous tenant across connections), so senders stay
  discoverable when `tenant` lives only on `connections[]` entries. Admins
  map a sender by passing `principal` as `member_id` to the enterprise
  membership API — no hand-composed ids.
- **Department-binding enrollment (TAN-765).** `POST /channels/enroll`
  (action `issue`) accepts `org_units` (bare unit id or `taxonomy/unit_id`);
  unknown units fail at issue time. Redeeming the pairing code establishes
  active org-unit memberships for the enrolled identity (persisted through
  the governance store) in addition to the capability tier, so a
  department-bound enrollment immediately yields a working governed run.
- **Diagnostics.** `GET /channels/config` includes a `connections` array for
  Slack with per-connection presence flags (`has_token`, `has_signing_secret`,
  `events_capable`, tenant/org-unit bindings) — never raw secrets.
- **Caveat.** `PUT /channels/slack` replaces the whole Slack object; clients
  that don't echo `connections` will drop them. Hand-edit the config file or
  include the full object when updating programmatically.

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
