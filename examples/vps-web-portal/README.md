# VPS Web Portal (Tandem Engine Examples)

This project contains a highly-polished, multi-page showcase that connects directly to the `tandem-engine` running as a headless server on a Linux VPS. It provides a secure UI wrapper and a Node/Express backend that handles reverse-proxying and authentication, safeguarding the engine's open ports from the public internet.

## Auto Setup Script (recommended)

If you want the fastest setup, use the install script. It handles almost everything:

- Installs/uses `@frumu/tandem`
- Generates (or reuses) API token
- Creates `/etc/tandem/engine.env`
- Bootstraps `$TANDEM_STATE_DIR/config.json` when missing
- Builds the portal
- Creates and starts both systemd services
  - `tandem-engine.service`
  - `tandem-portal.service`

```bash
cd examples/vps-web-portal
sudo TANDEM_STATE_DIR=/srv/tandem bash setup-vps.sh
```

To keep the engine sandbox limited to `TANDEM_STATE_DIR` only (no `/home/$USER` access), run:

```bash
sudo TANDEM_ALLOW_HOME_ACCESS=0 bash ./setup-vps.sh
```

Optional: pre-create `.env` in this folder with provider keys (`OPENROUTER_API_KEY`, etc.).  
The script merges those into `/etc/tandem/engine.env` automatically.

If you are using this script, you can skip the manual setup section and jump to `Usage`.

## Manual Setup

### Prerequisites

- A Linux VPS (Ubuntu/Debian recommended)
- Node.js (v18+)
- `sudo` access
- PM2 (optional, only for manual run path)
- The Tandem Engine CLI (only required for manual setup path)

### 1. Setup the Tandem Engine

Use this section only if you are not running `setup-vps.sh`.

First, install the tandem engine directly on your VPS. Provide it access to local AI providers or proxies as well as MCP components.

```bash
npm install -g @frumu/tandem
```

Generate a secure Server Token. Do NOT lose this.

```bash
tandem-engine token generate
```

_(Copy the generated UUID. We'll need it later)._

Start the engine running in the background. It will bind to `localhost:39731` by default.

```bash
nohup tandem-engine serve --hostname 127.0.0.1 --port 39731 --api-token "<YOUR_TOKEN>" &
```

#### Systemd (recommended)

For a one-command setup (engine + portal systemd services), run:

```bash
cd vps-web-portal
sudo TANDEM_API_TOKEN=your-generated-tandem-token-here TANDEM_STATE_DIR=/srv/tandem bash setup-vps.sh
```

This command installs dependencies, builds the web app, and creates both:

- `/etc/systemd/system/tandem-engine.service`
- `/etc/systemd/system/tandem-portal.service`

using the invoking user (via `SUDO_USER`) and discovered absolute binary paths automatically.

Tool resolution is deterministic and does not depend on interactive shell startup files (`.bashrc`, `.profile`):

1. Prefer `npm` for `@frumu/tandem` global install (ensures native postinstall runs consistently).
2. Fallback to `pnpm` only when explicitly enabled with `SETUP_ALLOW_PNPM_FALLBACK=1`.
3. Refresh `@frumu/tandem` to `@latest` on each setup run (disable with `SETUP_ENGINE_AUTO_UPDATE=0`).
4. Resolve `node` and `tandem-engine` from absolute paths for use in systemd `ExecStart`.
5. If no standalone `tandem-engine` binary is usable, fallback to `npx -y @frumu/tandem`.

Or follow the manual steps below.

Create `/etc/systemd/system/tandem-engine.service`:

```ini
[Unit]
Description=Tandem Engine
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=tandem
Group=tandem
EnvironmentFile=/etc/tandem/engine.env
ExecStart=/usr/bin/env tandem-engine serve --hostname 127.0.0.1 --port 39731
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=/srv/tandem

[Install]
WantedBy=multi-user.target
```

Create `/etc/tandem/engine.env`:

```bash
sudo mkdir -p /etc/tandem
TANDEM_API_TOKEN=your-generated-tandem-token-here
TANDEM_STATE_DIR=/srv/tandem
TANDEM_MEMORY_DB_PATH=/srv/tandem/memory.sqlite
```

For persistent provider keys (recommended on VPS), use the template in this folder:

```bash
sudo cp engine.env.example /etc/tandem/engine.env
sudo nano /etc/tandem/engine.env
```

Uncomment and fill the provider keys you actually use (for example `OPENROUTER_API_KEY`).
This should be done before you use the web portal.

Persistent provider/model config file (engine startup source):

- `$TANDEM_STATE_DIR/config.json` (for example `/srv/tandem/config.json`)

You can start from:

```bash
sudo cp engine.config.example.json /srv/tandem/config.json
sudo chown $(id -un):$(id -gn) /srv/tandem/config.json
```

The `TANDEM_STATE_DIR` in `engine.env` is the state path. If you prefer CLI flags, add `--state-dir /srv/tandem` to `ExecStart` instead.

Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now tandem-engine
```

### 2. Setup the Web Portal

Use this section only if you are not running `setup-vps.sh`.

Clone this repository or copy the `vps-web-portal` directory to your VPS.

Navigate to the project directory and install the required dependencies:

```bash
cd vps-web-portal
npm install
```

### Environment Configuration

Create a `.env` file in the root of the portal directory:

```env
# The port the Node/Express proxy will listen on (Publicly accessible)
PORT=80

# The Token generated by 'tandem-engine token generate' in Step 1
# This is also the login key for the portal.
VITE_PORTAL_KEY=your-generated-tandem-token-here

# (Optional) Engine local address
VITE_TANDEM_ENGINE_URL=http://127.0.0.1:39731
```

### 3. Build & Run

Use this section only if you are not running `setup-vps.sh`.

Build the Vite React frontend for production:

```bash
npm run build
```

Start the Node.js Express server to host the application. We recommend using PM2 to keep the server alive forever:

```bash
npm install -g pm2
pm2 start npm --name "tandem-portal" -- run start
```

_(Alternatively, you can just run `npm run start` directly for testing)._

## Troubleshooting Setup Script

If setup fails, run with debug output:

```bash
bash -x setup-vps.sh
```

Then verify services and local connectivity:

```bash
sudo systemctl status tandem-engine --no-pager
sudo systemctl status tandem-portal --no-pager
sudo ss -ltnp | grep ':80 '
curl -I http://127.0.0.1:80
```

## Usage

Visit your server's public IP or Domain name in your browser (e.g., `http://your-server-ip:80`).
You will be greeted by the Login portal. Enter the `VITE_PORTAL_KEY` you configured earlier.

On first login (or when no provider/model is configured), the portal now opens a **Provider Setup** screen where you:

1. Select a provider
2. Enter an API key (if required)
3. Pick a default model

The new **Ops** workspace is available immediately after login (it does not require provider setup),
so you can manage missions, automations/routines, MCP, channels, and agent-team controls even before model defaults are set.

Storage behavior:

- Provider API keys are sent via engine runtime auth (`PUT /auth/{provider}`), which is runtime-only.
- Default provider/model are persisted via `PATCH /config`.

For production persistence across restarts:

- Store provider keys in `/etc/tandem/engine.env` (see `engine.env.example`).
- Store default provider/model via `PATCH /config` (saved in `$TANDEM_STATE_DIR/config.json`).
- `setup-vps.sh` now bootstraps `$TANDEM_STATE_DIR/config.json` if missing.

### Included Dashboards

1. **Deep Research:** Unleash an agent to scour the public web and answer heavy research questions.
2. **Parallel Swarm:** Submit an idea and watch 3 distinct personas debate its merits simultaneously using the Parallel APIs.
3. **Interactive RPG:** An AI Text Adventure game demonstrating the `Questions API` over Streaming SSE.
4. **Second Brain:** Grant an agent local system context using Local MCP (Model Context Protocol).
5. **Connectors:** Setup Slack, Discord, or Telegram bots so your team can @ the engine directly.
6. **Ops Workspace:** Unified mission/automation/MCP/agent-team/channel control center with live global events, run artifact browser, and engine process controls.

### Ops Workspace Highlights

- Full mission lifecycle controls (`/mission`, `/mission/{id}`, `/mission/{id}/event`)
- Dual automation compatibility (`/automations/*` and `/routines/*`)
- Agent-team operations (`/agent-team/*`) including spawn approvals and cancellation
- MCP lifecycle and tool visibility (`/mcp/*`, `/mcp/tools`, `/tool/ids`)
- Channel config + status (`/channels/config`, `/channels/status`, `/channels/{name}`)
- Unified live SSE stream view (`/global/event`)
- Run artifact browser (`/{automations|routines}/runs/{id}/artifacts`) with safe local preview
- Engine process controls via portal backend (`/portal/system/engine/*`)

### Engine Process Controls (systemd)

`setup-vps.sh` now installs:

- `/usr/local/bin/tandem-engine-ctl` (restricted helper: `status|start|stop|restart`)
- `/etc/sudoers.d/tandem-portal-engine-control` (passwordless execution only for that helper)

Portal `.env` includes:

- `TANDEM_SYSTEM_CONTROL_MODE=systemd`
- `TANDEM_ENGINE_SERVICE_NAME=tandem-engine.service`
- `TANDEM_ENGINE_CONTROL_SCRIPT=/usr/local/bin/tandem-engine-ctl`
- `TANDEM_ARTIFACT_READ_ROOTS=/srv/tandem`
- `TANDEM_PORTAL_MAX_ARTIFACT_BYTES=1048576`

## Security Note

**Never** expose port `39731` to `0.0.0.0` or disable the firewall on a public VPS. Only the Reverse Proxy (this portal) should converse with the background Tandem engine locally, proxying authenticated REST and SSE requests.
