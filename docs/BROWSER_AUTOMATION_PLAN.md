# Feature Spec: Browser Automation ("The Eye")

## Executive Summary

This document specifies the implementation of a **Browser Automation** capability for Tandem.
Unlike the existing `webfetch` tool (which only downloads static HTML), this feature enables full interaction with modern, JavaScript-heavy web applications.

**Key Use Cases:**
- **End-to-End Testing:** Creating agents that can login to a staging environment and click buttons to verify workflows.
- **Deep Research:** Accessing information behind login screens or on Single Page Applications (SPAs) that require rendering.
- **Visual Validation:** Taking screenshots of the UI to verify CSS changes.

## Architecture

### 1. The Browser Sidecar (Optional & Detached)

To ensure `tandem-core` remains lightweight and portable (CLI/TUI friendly), the browser automation will be implemented as a **strictly optional sidecar**.

- **Desktop App (`tandem`)**: Will bundle the browser sidecar binary, but the sidecar will prefer using system Chrome/Chromium if installed.
- **TUI/CLI**: Will only support browser automation if the sidecar binary is present in the path.
- **Engine (`tandem-core`)**: Will stay lightweight and only define tool schemas, sidecar discovery, and JSON-RPC wiring. No browser dependencies are linked.

**Technology Stack:**
- **Driver:** `chromiumoxide` (Rust equivalent of Puppeteer).
- **Communication:** JSON-RPC over Stdio.
- **Isolation:** Runs in a separate process tree.
- **Runtime Browser:** Use system Chrome/Chromium by default; do not bundle Chromium in the app build.

### 2. Lightweight Engine Integration

The engine keeps only the portable pieces:

- Tool schemas and result types in `tandem-tools`
- Sidecar discovery (binary present in PATH or bundled by Desktop)
- JSON-RPC client wiring and timeout handling
- Permission gating and telemetry events

### 3. Integration with Tandem Tools

The capability will be exposed as a set of Tools in `tandem-tools`.

**Wiring flow:**
- On startup, `tandem-core` runs sidecar discovery and records availability
- If available, `tandem-tools` registers `browser_open`, `browser_act`, and `browser_extract`
- Each tool call routes to the sidecar via JSON-RPC and returns structured results
- If unavailable, tools are omitted and the agent sees no browser capability

**Wiring details:**
- The engine owns a `BrowserSidecarClient` that starts/stops the sidecar process and manages a single JSON-RPC connection
- The tools call into the client, which adds timeouts, retries, and response validation
- `browser_open` returns a `session_id` that is cached per run and reused by `browser_act` and `browser_extract`
- `browser_close` is called on run completion or when the agent is cancelled
- All calls emit telemetry events with latency and error codes for observability

**Tool: `browser_open`**
- `url`: string
- `headless`: boolean (default true)
- `user_data_dir`: string (optional, for persistent login sessions)
- `viewport`: { width: number, height: number } (optional)

**Tool: `browser_act`**
- `action`: "click" | "type" | "scroll" | "screenshot" | "evaluate" | "wait_for_selector" | "wait_for_navigation"
- `selector`: string (CSS or XPath)
- `value`: string (for type)
- `timeout_ms`: number (optional)

**Tool: `browser_extract`**
- `format`: "markdown" | "html" | "screenshot_base64"

## Workflow Example

**User:** "Log into my localhost:3000 app and verify the dashboard loads."

**Agent Action Plan:**
1. `browser_open(url="http://localhost:3000", headless=true)`
2. `browser_act(action="type", selector="#email", value="admin@test.com")`
3. `browser_act(action="type", selector="#password", value="password123")`
4. `browser_act(action="click", selector="button[type='submit']")`
5. `browser_act(action="wait_for_selector", selector=".dashboard-header")`
6. `browser_extract(format="screenshot_base64")` -> Returns image for user verification.

## Implementation Roadmap

### Phase 1: Proof of Concept Crate (`crates/tandem-browser`)
- [ ] Create a standalone Rust CLI that uses `chromiumoxide` to take a screenshot of a URL using system Chrome/Chromium.
- [ ] Implement a simple JSON-RPC loop to accept commands from Stdio.
- [ ] Define JSON-RPC request/response types and error codes.

### Phase 2: Engine Integration (Lightweight)
- [ ] Register `BrowserTool` in `tandem-tools`.
- [ ] Implement sidecar discovery and spawning in `tandem-core` (using the `SidecarProvider` trait defined in `MULTI_AGENTS.md`).
- [ ] Add timeouts, retries, and structured error mapping (tool errors vs sidecar errors).

### Phase 2.5: Frontend Add-Ons
- [ ] Desktop bundles the sidecar and exposes browser settings in UI (including Chrome path discovery/override).
- [ ] TUI/CLI checks PATH for the sidecar and provides a clear install hint when missing.

### Phase 3: "Look at this" (Vision)
- [ ] When `browser_extract` returns a screenshot, automatically pass it to a Vision-capable LLM (e.g., GPT-4o or Claude 3.5 Sonnet) if configured.
- [ ] Allow the agent to "see" the page and self-correct selectors.

## Sidecar Discovery Draft

The engine resolves the sidecar and browser paths without pulling in heavy browser dependencies.

**Sidecar binary resolution:**
- Desktop: prefer bundled sidecar in the app resources directory
- TUI/CLI: search PATH for `tandem-browser` (or configured path override)
- If missing, disable tools and surface a clear install hint

**Browser executable resolution:**
- First: explicit `browser_executable_path` in settings
- Second: well-known system install locations (platform-specific)
- Last: PATH lookup for `chrome`, `chromium`, `google-chrome`, `msedge`

## Sidecar Protocol Draft

JSON-RPC over stdio with a small set of request types. Every response returns either `result` or `error`.

**Lifecycle:**
- `browser.version`: returns sidecar version + protocol version
- `browser.open`: launches a browser session and returns a session id
- `browser.close`: closes a session id and frees resources

**Interaction:**
- `browser.act`: click/type/scroll/evaluate/wait with timeout
- `browser.extract`: return html/markdown/screenshot_base64

**Errors:**
- `ERR_SIDECAR_NOT_FOUND`
- `ERR_BROWSER_NOT_FOUND`
- `ERR_TIMEOUT`
- `ERR_SELECTOR_NOT_FOUND`
- `ERR_NAVIGATION_FAILED`

## Security Considerations

- **Sandboxing:** Browser should run in a restricted sandbox.
- **Network Permissions:** User should explicitly allow domains (or "Allow All" mode).
- **Session Data:** Warning user that `user_data_dir` typically stores cookies/passwords.
- **Process Limits:** Enforce a single active browser session per agent by default.

## Comparison to Alternatives
Tandem's implementation will be **Rust-native** (`chromiumoxide`), offering:
- Zero Python dependency deployment.
- Faster startup time.
- Tighter integration with the Tandem `EventBus`.
