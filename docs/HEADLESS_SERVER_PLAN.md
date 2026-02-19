# Tandem Headless Server & Web Admin UI

This document covers running `tandem-engine` as a standalone headless server (no desktop app, no TUI),
configuring external channel connections via API or config files, and an optional lightweight
security-first web admin interface served alongside the engine.

---

## 1. Headless Server Mode

`tandem-engine` already ships as a standalone binary with a `serve` subcommand. No Tauri, no GUI
dependency — just Rust + Axum.

### Quick Start

```bash
# Basic — listens on 127.0.0.1:39731
tandem-engine serve

# Exposed to LAN/internet with auth
tandem-engine serve \
  --hostname 0.0.0.0 \
  --port 39731 \
  --api-token $(tandem-engine token generate) \
  --provider openai \
  --model gpt-4o

# Custom state directory (config + sessions + memory live here)
tandem-engine serve --state-dir /srv/tandem --hostname 0.0.0.0 --api-token $TOKEN
```

### Config File — `config.json`

The engine reads `{state_dir}/config.json` at startup. This is the primary way to configure
providers, channels, and behaviour without environment variables.

**Full example:**
```json
{
  "default_provider": "openai",
  "providers": {
    "openai": {
      "api_key": "sk-...",
      "default_model": "gpt-4o"
    }
  },
  "channels": {
    "telegram": {
      "bot_token": "7xxxxxxxx:AAF...",
      "allowed_users": ["@evan", "@alice"],
      "mention_only": false
    },
    "discord": {
      "bot_token": "MTM...",
      "guild_id": "123456789",
      "allowed_users": ["*"],
      "mention_only": true
    },
    "slack": {
      "bot_token": "xoxb-...",
      "channel_id": "C0XXXXXXXX",
      "allowed_users": ["U0XXXXXXXX"]
    }
  },
  "web_ui": {
    "enabled": true,
    "path_prefix": "/admin"
  }
}
```

> **Hot reload**: A running server responds to `POST /admin/reload-config` (authenticated) to
> re-read `config.json` without restarting. Channel listeners that changed are stopped and
> restarted automatically.

### Environment Variables (Override Config)

All `config.json` fields can be overridden by env vars — useful for containers or secrets managers:

| Variable | Overrides |
|---|---|
| `TANDEM_API_TOKEN` | API bearer token |
| `TANDEM_ENGINE_HOST` | Bind hostname |
| `TANDEM_ENGINE_PORT` | Bind port |
| `TANDEM_STATE_DIR` | State directory |
| `TANDEM_TELEGRAM_BOT_TOKEN` | `channels.telegram.bot_token` |
| `TANDEM_TELEGRAM_ALLOWED_USERS` | `channels.telegram.allowed_users` (comma-separated) |
| `TANDEM_DISCORD_BOT_TOKEN` | `channels.discord.bot_token` |
| `TANDEM_DISCORD_GUILD_ID` | `channels.discord.guild_id` |
| `TANDEM_DISCORD_ALLOWED_USERS` | `channels.discord.allowed_users` |
| `TANDEM_SLACK_BOT_TOKEN` | `channels.slack.bot_token` |
| `TANDEM_SLACK_CHANNEL_ID` | `channels.slack.channel_id` |
| `TANDEM_SLACK_ALLOWED_USERS` | `channels.slack.allowed_users` |
| `TANDEM_WEB_UI` | `"true"` or `"false"` — enable/disable web admin |

### Configuring Channels via the HTTP API

Channels can also be configured dynamically without restarting the server using authenticated API calls.

#### Check current channel status
```http
GET /channels/status
Authorization: Bearer <token>
```
Response:
```json
{
  "telegram": { "enabled": true, "connected": true, "bot_username": "@mytandembot", "active_sessions": 3 },
  "discord":  { "enabled": false, "connected": false },
  "slack":    { "enabled": false, "connected": false }
}
```

#### Enable/update a channel
```http
PUT /channels/telegram
Authorization: Bearer <token>
Content-Type: application/json

{
  "bot_token": "7xxx:AAF...",
  "allowed_users": ["@evan"],
  "mention_only": false
}
```
This writes the config to `config.json` and starts the listener immediately. Bot tokens are
stored in the OS keystore (`storeApiKey` / `secret_service` / Windows Credential Manager),
not written to `config.json` in plain text.

#### Disable a channel
```http
DELETE /channels/telegram
Authorization: Bearer <token>
```
Stops the listener and clears the config entry.

---

## 2. Lightweight Web Admin UI

A minimal, security-first HTML interface served directly by the Axum server on the same port as
the engine API. No separate process, no Node, no build step in production — the UI is a single
self-contained HTML file baked into the binary at compile time via `include_str!`.

### Design Principles

- **Single file** — `crates/tandem-server/src/webui/admin.html` compiled in with `include_str!`.
  No asset pipeline, no CDN, zero external requests.
- **Vanilla HTML + minimal JS** — ~600 lines total. No frameworks. Uses the Fetch API
  to call the existing engine REST endpoints.
- **Security first** — see Security section below.
- **Fast** — full page loads in under 50ms on LAN. No JS bundle, no waiting for a CDN.
- **Dark-mode native** — uses CSS `prefers-color-scheme`.

### What it Shows

```
┌─────────────────────────────────────────────────────────────────┐
│  🤖 Tandem Engine  v0.3.7  ●  Ready    [Sign out]               │
├──────────────┬──────────────────────────────────────────────────┤
│  NAVIGATION  │  Connections                                      │
│  ──────────  │  ──────────────────────────────────────────────  │
│  Connections │  [🟢 Telegram]  @mytandembot  3 sessions  [Edit] │
│  Sessions    │  [⚫ Discord ]  Not configured          [Set up] │
│  Memory      │  [⚫ Slack   ]  Not configured          [Set up] │
│  Settings    │                                                   │
└──────────────┴──────────────────────────────────────────────────┘
```

**Pages:**
- **Connections** — enable/disable Telegram, Discord, Slack; enter bot tokens (masked input);
  show live session counts and connection health
- **Sessions** — list active and recent sessions, click to view conversation history; session
  `source` tag shows which channel started it (e.g. `telegram:@evan`)
- **Memory** — browse memory records, add/delete entries
- **Settings** — provider config, API token display, hot-reload config button

### Implementation

#### `crates/tandem-server/src/webui/mod.rs` [NEW]

```rust
static ADMIN_HTML: &str = include_str!("admin.html");

pub fn web_ui_router(prefix: &str) -> Router {
    Router::new()
        .route(&format!("{}/", prefix), get(serve_index))
        .route(&format!("{}/*path", prefix), get(serve_index))
}

async fn serve_index() -> impl IntoResponse {
    Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", CSP_HEADER)
        .header("X-Frame-Options", "DENY")
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .body(ADMIN_HTML)
}
```

The HTML file makes authenticated fetch calls to the existing `/channels/*`, `/sessions`,
`/memory` endpoints using a session cookie set after login.

#### `crates/tandem-server/src/webui/admin.html` [NEW]

Single-file app structure:
```html
<!DOCTYPE html>
<html lang="en">
<head>
  <!-- All CSS inline in <style> — zero external requests -->
</head>
<body>
  <!-- Static shell rendered immediately -->
  <nav>...</nav>
  <main id="content">...</main>

  <script>
    // ~400 lines vanilla JS:
    // - login() → POST /admin/login → sets Authorization header in memory (not localStorage)
    // - loadPage(name) → fetch relevant endpoints → DOM render
    // - Per-page render functions: renderConnections(), renderSessions(), renderMemory()
    // - Form handlers: enableChannel(), disableChannel(), saveChannelConfig()
  </script>
</body>
</html>
```

### Enabling the Web UI

**Via CLI flag:**
```bash
tandem-engine serve --web-ui --hostname 0.0.0.0 --api-token $TOKEN
```

**Via config:**
```json
{ "web_ui": { "enabled": true, "path_prefix": "/admin" } }
```

**Via env:**
```bash
TANDEM_WEB_UI=true tandem-engine serve
```

When enabled, the engine logs:
```
INFO tandem_server: web admin UI available at http://0.0.0.0:39731/admin/
```

---

## 3. Security Model

Running the engine headlessly and exposing it to a network requires explicit attention to
security. These measures apply whether using the API directly or through the web UI.

### Authentication

- All API endpoints (and the web UI) are gated behind `Authorization: Bearer <token>` or the
  `X-Tandem-Token` header — **including `/channels/*` and `/admin`**.
- The web UI login page accepts the same token and stores it **only in memory**
  (`sessionStorage` at most) — never `localStorage`, never a cookie without `HttpOnly + Secure`.
- Unauthenticated requests to any endpoint receive `401 Unauthorized` with no body.

### Transport

```
CRITICAL: Do NOT expose port 39731 directly to the internet without TLS.
```

Recommended options (easiest first):

| Option | Setup |
|--------|-------|
| **Cloudflare Tunnel** | `cloudflared tunnel --url http://localhost:39731` — free, TLS automatic |
| **Caddy reverse proxy** | `caddy reverse-proxy --from :443 --to :39731` — auto-TLS via ACME |
| **Nginx + Let's Encrypt** | Standard reverse proxy with `ssl_certificate` |
| **Tailscale** | Zero-config private WireGuard mesh — no port forwarding needed |

The engine itself does **not** handle TLS — this is intentionally kept at the reverse proxy
layer so the engine binary stays simple.

### Content Security Policy (Web UI)

The admin HTML is served with a strict CSP header:

```
Content-Security-Policy:
  default-src 'none';
  script-src 'self' 'nonce-{random}';
  style-src 'self' 'unsafe-inline';
  connect-src 'self';
  img-src data:;
  frame-ancestors 'none';
```

- `default-src 'none'` — everything blocked by default
- `connect-src 'self'` — API calls only to same origin (no data exfiltration)
- `frame-ancestors 'none'` — prevents clickjacking
- `X-Frame-Options: DENY` for legacy browsers

### Rate Limiting & Input Validation

- The existing Axum `RequestBodyLimitLayer` (already in `tandem-server`) caps request bodies
- Channel config endpoints validate bot token format before storing
- `allowed_users` values are sanitized (strip quotes, newlines, etc.) before write

### Principle of Least Privilege (Headless Deployment)

```bash
# Run as a dedicated non-root user
useradd -r -s /sbin/nologin tandem
chown -R tandem:tandem /srv/tandem
sudo -u tandem tandem-engine serve --state-dir /srv/tandem --api-token $TOKEN

# Systemd unit (recommended)
[Service]
User=tandem
AmbientCapabilities=
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=/srv/tandem
```

---

## 4. Deployment Recipes

### Docker

```dockerfile
FROM debian:bookworm-slim
COPY tandem-engine /usr/local/bin/
RUN useradd -r tandem
USER tandem
VOLUME ["/data"]
ENV TANDEM_STATE_DIR=/data
ENV TANDEM_WEB_UI=true
EXPOSE 39731
ENTRYPOINT ["tandem-engine", "serve", "--hostname", "0.0.0.0"]
```

```bash
docker run -d \
  -v tandem-data:/data \
  -e TANDEM_API_TOKEN=tk_xxx \
  -e TANDEM_TELEGRAM_BOT_TOKEN=7xxx:AAF \
  -e TANDEM_TELEGRAM_ALLOWED_USERS="@evan" \
  -p 127.0.0.1:39731:39731 \   # Only bind to localhost — put Caddy/nginx in front
  tandem-engine
```

### Systemd (Linux)

```ini
[Unit]
Description=Tandem Engine
After=network-online.target

[Service]
User=tandem
EnvironmentFile=/etc/tandem/env
ExecStart=/usr/local/bin/tandem-engine serve --hostname 127.0.0.1 --web-ui
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

`/etc/tandem/env`:
```
TANDEM_API_TOKEN=tk_xxx
TANDEM_STATE_DIR=/srv/tandem
TANDEM_TELEGRAM_BOT_TOKEN=7xxx:AAF
TANDEM_TELEGRAM_ALLOWED_USERS=@evan
TANDEM_WEB_UI=true
```

---

## 5. Implementation Checklist

- [ ] Add `--web-ui` flag to `tandem-engine serve` CLI
- [ ] Add `web_ui` section to `config.json` schema
- [ ] Create `crates/tandem-server/src/webui/mod.rs` + `admin.html`
- [ ] Add `GET /channels/status` endpoint to `http.rs`
- [ ] Add `PUT /channels/{name}` endpoint (enable/configure channel)
- [ ] Add `DELETE /channels/{name}` endpoint (disable channel)
- [ ] Add `POST /admin/reload-config` endpoint (hot reload)
- [ ] Store bot tokens via OS keystore (not plain-text `config.json`)
- [ ] Mount `web_ui_router` in `app_router` when feature enabled
- [ ] Write CSP headers via Axum response builder
- [ ] Docker image + Systemd unit example in docs

---

## 6. Cross-Reference

- Channel adapter implementation: [CHANNELS_INTEGRATION_PLAN.md](./CHANNELS_INTEGRATION_PLAN.md)
- Desktop GUI / TUI connections UI: [CHANNELS_INTEGRATION_PLAN.md — UI Integration](./CHANNELS_INTEGRATION_PLAN.md#ui-integration)
- Engine CLI reference: [ENGINE_CLI.md](./ENGINE_CLI.md)
