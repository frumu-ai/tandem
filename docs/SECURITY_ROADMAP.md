# Security Roadmap

This file tracks security hardening work as an explicit, auditable backlog.

## 1) Path canonicalization checks

- **Risk**: path traversal / symlink tricks bypass allowlists.
- **Code**: `src-tauri/src/state.rs` (and any path allowlist logic)
- **Done when**:
  - target paths and workspace roots are canonicalized before prefix checks
  - Windows path normalization is handled consistently

## 2) Asset protocol scope restriction

- **Risk**: a frontend bug (XSS) could allow loading arbitrary local files via `asset://`.
- **Config**: `src-tauri/tauri.conf.json` (`app.security.assetProtocol.scope`)
- **Done when**:
  - asset scope is restricted to app resources + approved workspace folders

## 3) Sidecar IPC authentication (local-only still benefits)

- **Risk**: other local processes can call a local HTTP port if discoverable.
- **Code**: `src-tauri/src/sidecar.rs` and sidecar API calls
- **Done when**:
  - a per-launch bearer token is generated and required on requests

## 4) CSP tightening

- **Risk**: `unsafe-inline` makes XSS significantly worse.
- **Config**: `src-tauri/tauri.conf.json` (`app.security.csp`)
- **Done when**:
  - inline scripts removed or nonce/hash-based policy added
