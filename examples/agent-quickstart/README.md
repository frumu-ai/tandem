# Tandem Agent Quickstart

A minimal self-hosted AI agent portal. Run the setup script and you have a working agent accessible at `http://your-server-ip` in under 5 minutes.

## One-command setup (VPS / Linux server)

```bash
# Clone the repo and cd into this example
cd examples/agent-quickstart

# Run as root (or sudo) — installs engine, builds portal, creates systemd services
sudo bash setup-agent.sh
```

After setup, the script prints your portal URL and sign-in token:

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ Tandem Agent Quickstart is running!
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Portal URL:   http://<your-server-ip>
  Sign-in key:  <generated-token>

  Services:
    sudo systemctl status tandem-engine
    sudo systemctl status tandem-agent-portal
```

> **Prerequisites:** Node.js and npm must be installed. If not:
>
> ```bash
> curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo bash -
> sudo apt install -y nodejs
> ```

---

## What the setup script does

| Step | Action                                                                           |
| ---- | -------------------------------------------------------------------------------- |
| 1    | Detects Node / npm (handles nvm, system, and PATH installs)                      |
| 2    | Installs `@frumu/tandem` globally via npm                                        |
| 3    | Generates an API token via `tandem-engine token generate`                        |
| 4    | Writes `/etc/tandem/engine.env` (token + provider key placeholders)              |
| 5    | Bootstraps `/srv/tandem/config.json` with default providers                      |
| 6    | Builds the portal with `npm run build`                                           |
| 7    | Writes `examples/agent-quickstart/.env` with token + port                        |
| 8    | Creates and starts two systemd services: `tandem-engine` + `tandem-agent-portal` |
| 9    | Runs an engine health check and prints the final URL + token                     |

**Re-running is safe** — existing tokens and provider keys are preserved.

---

## Add an AI provider

After setup, you have two ways to add a provider key:

**Option A — UI (after sign-in):** Go to the **Provider Setup** tab in the portal.

**Option B — edit env file:**

```bash
sudo nano /etc/tandem/engine.env
# Uncomment and fill in one of:
# OPENROUTER_API_KEY=or-...
# OPENAI_API_KEY=sk-...
# ANTHROPIC_API_KEY=sk-ant-...

sudo systemctl restart tandem-engine
```

---

## Environment variables (setup-agent.sh)

| Variable                   | Default            | Description                         |
| -------------------------- | ------------------ | ----------------------------------- |
| `TANDEM_API_TOKEN`         | _(auto-generated)_ | Override the generated token        |
| `TANDEM_STATE_DIR`         | `/srv/tandem`      | Engine storage directory            |
| `SETUP_ENGINE_AUTO_UPDATE` | `1`                | Set to `0` to skip npm update check |

---

## Local dev (no server needed)

```bash
cd examples/agent-quickstart
cp .env.example .env       # set PORTAL_KEY + TANDEM_ENGINE_URL
npm install
npm run dev                # → http://localhost:5173 (Vite dev server with proxy)
```

## Troubleshooting

### "Failed to load provider settings" or `502 Engine unreachable`

Check that engine is up and quickstart points to the same URL/port:

```bash
ss -ltnp | rg 39731
curl -sS http://127.0.0.1:39731/global/health | jq .
grep -nE '^(TANDEM_ENGINE_URL|VITE_TANDEM_ENGINE_URL|PORTAL_KEY|PORT)=' .env
```

Then restart services:

```bash
sudo systemctl restart tandem-engine
sudo systemctl restart tandem-agent-portal
```

### Deploy updated engine from source

Use this when you changed Rust engine/runtime code locally and want the VPS service to run the new binary:

```bash
cd /home/evan/tandem
cargo build -p tandem-ai --release
sudo systemctl stop tandem-engine
sudo install -m 755 target/release/tandem-engine /usr/local/bin/tandem-engine-dev
sudo systemctl start tandem-engine
sudo systemctl status --no-pager tandem-engine
```

Health-check the engine and proxy:

```bash
curl -sS http://127.0.0.1:39731/global/health | jq .
KEY=$(sed -n 's/^PORTAL_KEY=//p' /home/evan/tandem/examples/agent-quickstart/.env | tail -n1)
curl -sS -H "Authorization: Bearer $KEY" http://127.0.0.1:3302/engine/global/health | jq .
```

### Rebuild and restart the portal service

```bash
cd /home/evan/tandem/examples/agent-quickstart
pnpm install
pnpm build
sudo systemctl restart tandem-agent-portal
sudo systemctl status --no-pager tandem-agent-portal
```

### Chat opens but agent does not respond

- Verify a provider + model is configured in **Provider Setup**.
- Quickstart now gates startup on provider setup, but stale config can still cause broken sessions.
- Open browser devtools and check failed `/engine/*` requests for auth or upstream errors.

### MCP OAuth loops (Arcade and similar)

- Ensure MCP headers include:
  - `Authorization: Bearer <api-key>`
  - `Arcade-User-ID: <stable-user-id>`
- `Arcade-User-ID` must stay stable, or authorization may repeat.
- If needed, disconnect + reconnect MCP server and refresh tool cache from the MCP page.

---

## Portal views

| View          | Route       | Purpose                                                       |
| ------------- | ----------- | ------------------------------------------------------------- |
| **Chat**      | `/chat`     | Multi-session AI chat with tool results and approval flow     |
| **Agents**    | `/agents`   | Create and manage scheduled routines (cron / interval)        |
| **Channels**  | `/channels` | Connect Telegram, Discord, or Slack bots                      |
| **Live Feed** | `/feed`     | Global SSE event stream — real-time view of all engine events |

---

## Project structure

```
examples/agent-quickstart/
├── setup-agent.sh        ← one-command setup script
├── server.js             ← Express: Bearer auth + /engine proxy
├── .env.example          ← PORTAL_KEY, TANDEM_ENGINE_URL, PORT
├── package.json
└── src/
    ├── api.ts            # Typed engine API client (~300 lines)
    ├── AuthContext.tsx   # Token persistence + provider status
    ├── runStream.ts      # SSE streaming utility (reconnect / watchdog)
    ├── App.tsx           # Sidebar layout + routing
    └── pages/
        ├── ChatBrain.tsx
        ├── Agents.tsx
        ├── Channels.tsx
        ├── LiveFeed.tsx
        ├── Login.tsx
        └── ProviderSetup.tsx
```
