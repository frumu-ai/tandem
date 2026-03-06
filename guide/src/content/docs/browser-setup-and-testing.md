---
title: Browser Setup and Testing
description: Build, install, test, and incorporate tandem-browser with tandem-engine, desktop, and control panel flows.
---

Use this guide when you need browser automation in Tandem and want to verify the full path from sidecar install to engine readiness.

## What `tandem-browser` is

`tandem-browser` is the Chromium automation sidecar used by `tandem-engine`. It is not the same thing as the control panel browser UI or the Tauri desktop app.

The runtime model is:

- `tandem-browser` runs on the same host as `tandem-engine`
- a Chromium-family browser must also exist on that host
- desktop and control panel query the engine for readiness instead of hosting the browser sidecar themselves

## Prerequisites

- Rust stable if you are building from source
- `tandem-engine` and `tandem-browser` on the same host
- Chrome, Chromium, Edge, or Brave on that host
- Linux only: the shared libraries Chromium needs

### Linux: install Chromium

On Debian/Ubuntu hosts, install Chromium with:

```bash
sudo apt update && sudo apt install -y chromium
```

Then verify:

```bash
tandem-browser doctor --json
```

If the browser binary is not on `PATH`, set:

- `TANDEM_BROWSER_EXECUTABLE`
- or `browser.executable_path` in config

## Current setup paths

### Engine-managed install

This is the preferred operator path.

Install the sidecar for the current Tandem version:

```bash
tandem-engine browser install
```

This downloads the matching `tandem-browser` release asset from GitHub Releases and installs it on the engine host.

Then verify readiness:

```bash
tandem-engine browser doctor --json
```

If the sidecar is installed but Chromium is missing, the doctor output will usually show `browser_not_found`.

### Build from source

From the repo root:

```bash
cargo build -p tandem-browser
./target/debug/tandem-browser --version
./target/debug/tandem-browser doctor --json
```

Build a release binary when you want a realistic size measurement:

```bash
cargo build --release -p tandem-browser
ls -lh target/release/tandem-browser
```

## Config keys

Browser automation is controlled through the `browser` config block or equivalent env vars.

```json
{
  "browser": {
    "enabled": true,
    "sidecar_path": "/path/to/tandem-browser",
    "executable_path": "/path/to/chromium",
    "headless_default": true,
    "allow_no_sandbox": false,
    "user_data_root": "/path/to/browser-data",
    "allowed_hosts": ["example.com"]
  }
}
```

Environment equivalents:

- `TANDEM_BROWSER_ENABLED`
- `TANDEM_BROWSER_SIDECAR`
- `TANDEM_BROWSER_EXECUTABLE`
- `TANDEM_BROWSER_HEADLESS`
- `TANDEM_BROWSER_ALLOW_NO_SANDBOX`
- `TANDEM_BROWSER_USER_DATA_ROOT`
- `TANDEM_BROWSER_ALLOWED_HOSTS`

## Test flow

### 1. Sidecar doctor

Standalone sidecar smoke test:

```bash
tandem-browser doctor --json
```

This confirms:

- the sidecar binary is present
- a Chromium-family browser can be found
- Chromium can actually launch in the configured headless/sandbox mode

### 2. Engine doctor

Run the engine-facing doctor using the effective engine config:

```bash
tandem-engine browser doctor --json
```

This is the command to trust when validating a real engine deployment.

### 3. Running engine status

Start the engine and query browser readiness through HTTP:

```bash
tandem-engine serve --hostname 127.0.0.1 --port 39731
tandem-engine browser status --hostname 127.0.0.1 --port 39731
curl -s http://127.0.0.1:39731/browser/status | jq .
```

### 4. Tool-level smoke test

Once status is runnable, validate real browser actions through the engine runtime:

1. Call `browser_status`
2. Call `browser_open`
3. Call `browser_snapshot`
4. Call `browser_close`

If `browser_status` says blocked, do not debug the model flow first. Fix host readiness first.

### 5. Exact smoke-test commands

After Chromium is installed, run this sequence.

Verify the sidecar and engine host first:

```bash
tandem-browser doctor --json
tandem-engine browser doctor --json
```

Both should report a runnable browser setup.

Start the engine:

```bash
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

In another terminal, confirm engine browser status:

```bash
tandem-engine browser status --hostname 127.0.0.1 --port 39731
curl -s http://127.0.0.1:39731/browser/status | jq .
```

Then run a full browser tool smoke test through the engine.

Check browser tool readiness:

```bash
cat <<'JSON' | tandem-engine tool --json -
{"tool":"browser_status","args":{}}
JSON
```

Open a page:

```bash
cat <<'JSON' | tandem-engine tool --json -
{"tool":"browser_open","args":{"url":"https://example.com"}}
JSON
```

Copy the returned `session_id`, then snapshot that session:

```bash
cat <<'JSON' | tandem-engine tool --json -
{"tool":"browser_snapshot","args":{"session_id":"PASTE_SESSION_ID_HERE","include_screenshot":false}}
JSON
```

Close the browser session:

```bash
cat <<'JSON' | tandem-engine tool --json -
{"tool":"browser_close","args":{"session_id":"PASTE_SESSION_ID_HERE"}}
JSON
```

Expected result:

- `browser_status` returns readiness info
- `browser_open` returns a new `session_id`
- `browser_snapshot` returns page metadata and elements
- `browser_close` succeeds without leaking the session

## Desktop and Control Panel

### Control Panel

The control panel does not run `tandem-browser` directly. It talks to the engine host.

Use Settings -> Browser automation to:

- read `GET /browser/status`
- trigger `POST /browser/install`

If the control panel is connected to a remote engine, installation still happens on that engine host.

### Tauri Desktop

The desktop app should be treated as an engine client for browser automation. The browser sidecar belongs next to the engine host, not inside the web frontend runtime.

The recommended desktop behavior is:

- keep managing the Tandem engine sidecar as usual
- use engine browser diagnostics for browser automation readiness
- rely on engine-managed `tandem-browser` install instead of bundling a second browser sidecar into the desktop app

## Release assets and incorporation

`tandem-browser` should be published as standalone GitHub release assets alongside other Tandem binaries:

- `tandem-browser-linux-x64.tar.gz`
- `tandem-browser-darwin-x64.zip`
- `tandem-browser-darwin-arm64.zip`
- `tandem-browser-windows-x64.zip`

The engine install flow targets the matching Tandem version, not an unconstrained latest release. That keeps the browser sidecar protocol aligned with the engine build.

## Troubleshooting

### `browser_sidecar_not_found`

- Run `tandem-engine browser install`
- or set `TANDEM_BROWSER_SIDECAR`
- or set `browser.sidecar_path`

### `browser_not_found`

- Install Chrome, Chromium, Edge, or Brave on the engine host
- Debian/Ubuntu example: `sudo apt update && sudo apt install -y chromium`
- set `TANDEM_BROWSER_EXECUTABLE` if the binary is outside `PATH`

### Launch fails on Linux VPS

- confirm Chromium shared libraries are installed
- try headless mode first
- only enable `allow_no_sandbox` when the host environment requires it

### Control panel shows blocked

- read the blocking issue from `/browser/status`
- run `tandem-engine browser doctor --json` on the engine host
- confirm the control panel is connected to the engine you actually configured
