# Tandem Browser Local Guide

This file documents the current local `tandem-browser` build and the commands to test it on this machine.

## Current local build paths

Current debug build:

```text
/home/evan/tandem/target/debug/tandem-browser
```

Current release build:

```text
/home/evan/tandem/target/release/tandem-browser
```

## Current browser detection on this host

The sidecar currently detects Chromium here:

```text
/snap/bin/chromium
```

The verified standalone doctor result is:

- `enabled: true`
- `runnable: true`
- `browser.path: /snap/bin/chromium`
- `blocking_issues: []`

## Standalone sidecar test

Run:

```bash
./target/debug/tandem-browser doctor --json
```

Expected result:

- `runnable: true`
- no blocking issues

## Engine-side readiness test

Run:

```bash
cargo run -p tandem-ai -- browser doctor --json
```

Expected result:

- `runnable: true`
- the engine sees the same browser setup that the standalone sidecar sees

## Full engine smoke test

Start the engine:

```bash
cargo run -p tandem-ai -- serve --hostname 127.0.0.1 --port 39731
```

In another terminal, check engine browser status:

```bash
cargo run -p tandem-ai -- browser status --hostname 127.0.0.1 --port 39731
curl -s http://127.0.0.1:39731/browser/status \
  -H 'Authorization: Bearer tk_3b9e2f1f5d194e46b4204f751acb9b27' | jq .
```

Expected result:

- browser status returns successfully
- `runnable: true`

If the engine is running with API token auth enabled, include the token on direct HTTP requests:

```bash
export TANDEM_API_TOKEN='tk_3b9e2f1f5d194e46b4204f751acb9b27'
curl -s http://127.0.0.1:39731/browser/status \
  -H "Authorization: Bearer $TANDEM_API_TOKEN" | jq .
```

## If `/browser/status` says browser automation is disabled

That means the engine process was started without browser automation enabled, even if the standalone `tandem-browser doctor --json` check passes.

For this machine, start the engine with browser automation explicitly enabled:

```bash
TANDEM_BROWSER_ENABLED=true \
TANDEM_BROWSER_SIDECAR=/home/evan/tandem/target/debug/tandem-browser \
TANDEM_BROWSER_EXECUTABLE=/snap/bin/chromium \
TANDEM_API_TOKEN='tk_3b9e2f1f5d194e46b4204f751acb9b27' \
cargo run -p tandem-ai -- serve --hostname 127.0.0.1 --port 39731
```

Then re-check:

```bash
curl -s http://127.0.0.1:39731/browser/status \
  -H "Authorization: Bearer $TANDEM_API_TOKEN" | jq .
```

Expected result after restart:

- `enabled: true`
- `sidecar.found: true`
- `browser.found: true`
- `runnable: true`

## Browser tool smoke test through the engine

### 1. Check browser tool readiness

```bash
cat <<'JSON' | cargo run -p tandem-ai -- tool --json -
{"tool":"browser_status","args":{}}
JSON
```

### 2. Open a page

```bash
cat <<'JSON' | cargo run -p tandem-ai -- tool --json -
{"tool":"browser_open","args":{"url":"https://example.com"}}
JSON
```

Copy the returned `session_id`.

### 3. Snapshot the page

```bash
cat <<'JSON' | cargo run -p tandem-ai -- tool --json -
{"tool":"browser_snapshot","args":{"session_id":"PASTE_SESSION_ID_HERE","include_screenshot":false}}
JSON
```

### 4. Close the session

```bash
cat <<'JSON' | cargo run -p tandem-ai -- tool --json -
{"tool":"browser_close","args":{"session_id":"PASTE_SESSION_ID_HERE"}}
JSON
```

Expected result:

- `browser_open` returns a valid `session_id`
- `browser_snapshot` returns page metadata and element data
- `browser_close` succeeds

## Engine-backed smoke test endpoint

If the engine service is already running, use the engine HTTP smoke test instead of running
`cargo run -p tandem-ai -- tool ...` locally.

Direct HTTP:

```bash
export TANDEM_API_TOKEN='tk_3b9e2f1f5d194e46b4204f751acb9b27'
curl -s http://127.0.0.1:39731/browser/smoke-test \
  -H "Authorization: Bearer $TANDEM_API_TOKEN" \
  -H "content-type: application/json" \
  -d '{"url":"https://example.com"}' | jq .
```

Expected result:

- `ok: true`
- page title and final URL are returned
- a short text excerpt is returned from the page
- `closed: true`

Control panel:

- open Settings
- go to Browser readiness
- click `Run smoke test`

That path validates the already-running engine service, not a separate one-shot CLI process.

## If you want the engine-managed install path

The default managed install location is:

```text
~/.tandem/binaries/tandem-browser
```

Install it with:

```bash
cargo run -p tandem-ai -- browser install
```

### Manual copy into the managed install path

If you want the engine to auto-detect the local browser sidecar without setting
`TANDEM_BROWSER_SIDECAR`, place the binary here:

```text
~/.tandem/binaries/tandem-browser
```

On this machine:

```bash
mkdir -p ~/.tandem/binaries
cp /home/evan/tandem/target/debug/tandem-browser ~/.tandem/binaries/tandem-browser
chmod +x ~/.tandem/binaries/tandem-browser
```

Then restart the engine with browser automation enabled, but without a sidecar path override:

```bash
TANDEM_BROWSER_ENABLED=true \
TANDEM_BROWSER_EXECUTABLE=/snap/bin/chromium \
TANDEM_API_TOKEN='tk_3b9e2f1f5d194e46b4204f751acb9b27' \
cargo run -p tandem-ai -- serve --hostname 127.0.0.1 --port 39731
```

Then verify:

```bash
export TANDEM_API_TOKEN='tk_3b9e2f1f5d194e46b4204f751acb9b27'
curl -s http://127.0.0.1:39731/browser/status \
  -H "Authorization: Bearer $TANDEM_API_TOKEN" | jq .
```

Expected result:

- the engine finds the sidecar automatically from `~/.tandem/binaries/tandem-browser`
- `enabled: true`
- `sidecar.found: true`
- `browser.found: true`
- `runnable: true`

## Notes

- Right now, your working local debug binary is `/home/evan/tandem/target/debug/tandem-browser`.
- Chromium from Snap is working correctly for the standalone doctor check.
- If engine checks fail while standalone doctor passes, debug the engine config/path resolution next.
