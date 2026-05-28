# Security Analysis — Tandem Rust Runtime (`crates/`)

**Scope:** the 21 Rust crates under `crates/` (~400K LoC) plus `Cargo.lock` dependency review.
**Date:** 2026-05-28 · **Method:** five parallel domain audits (command/PTY execution, HTTP/SSE API, secrets/crypto, permissions/governance/FS tools, external integrations) with source-verified findings.

---

## Executive Summary

Tandem is a local-first AI-agent runtime whose security model rests on three pillars: **loopback-only binding**, a **human approval gate** for risky tools, and **encrypted credential storage**. The audit found that each pillar has gaps, and they compound:

1. **Permissive-by-default.** The HTTP API is **unauthenticated by default** (`LocalSingleTenant` mode), file `write`/`edit`/`apply_patch` are **`allow` by default** (no prompt), and `.env.example` ships with tool-guard budgets disabled. Security depends almost entirely on the process never being reachable beyond `127.0.0.1`.
2. **The shell "sandbox" is not a real sandbox.** `bash` runs `sh -lc <raw command>` with no filesystem confinement; the only content control is a ~14-entry lowercase substring blocklist that is trivially evaded.
3. **Approval gates can be skipped** via `batch` sub-calls and via automation/routine `auto_approve` combined with an empty (= allow-all) tool allowlist.

Well-built areas (verified, not vulnerable): channel webhook authenticity (Slack HMAC / Discord Ed25519 / Telegram token, constant-time, replay windows), the enterprise governance engine (fails closed), SQL in `tandem-memory` (parameterized), the Google Drive connector, hosted/enterprise JWS tenant auth, and the `unsafe` blocks (all idiomatic/test-only). Dependencies are current (no `rsa`/Marvin; recent rustls/openssl/ring) — only unmaintained-crate advisories (`serde_yaml`, `yaml-rust`, `instant`, `proc-macro-error`).

---

## Findings by Severity

### CRITICAL

#### C1 — JWT signature is never verified (OpenAI Codex OAuth)
`crates/tandem-core/src/provider_auth_store.rs:559-603` (`decode_codex_jwt_claims`)
The code decodes JWT claims but deliberately does **not** verify the RS256 signature (in-code comment admits "any attacker can forge tokens by modifying payload and signature fields"). Only `alg:"none"` and structural errors are rejected; `alg` itself is read from the attacker-controlled header.
**Impact:** anyone who can place/modify `auth.json` can forge identity claims (`chatgpt_account_id`, `email`, `exp`) that are then trusted for account-id/expiry decisions.
**Fix:** fetch OpenAI's JWKS and verify the signature, or treat claims as untrusted hints that are never an identity/authorization boundary.

#### C2 — Unauthenticated `add_mcp` → arbitrary command execution
Route `crates/tandem-server/src/http/mcp.rs:509-528` · launcher `crates/tandem-runtime/src/mcp_parts/part02.rs:774-795` (`sh -lc <command_text>`) · auth gate `crates/tandem-server/src/http/middleware.rs:255-268` · default mode `crates/tandem-enterprise-contract/src/lib.rs:13-20`
`add_mcp` accepts `transport: "stdio:<arbitrary shell>"`. In the default `LocalSingleTenant` mode with no `api_token`, `auth_gate` lets the request through, and registering the server spawns `sh -lc <command>` → RCE. Loopback-only by default contains this to local callers, **but a non-loopback bind (`--hostname 0.0.0.0`, reverse proxy) without a token is unauthenticated remote RCE.**
**Fix:** require a token for any non-loopback bind (refuse to start otherwise in local mode); restrict `stdio:` transports to a server-side allowlist instead of accepting arbitrary shell over HTTP.

### HIGH

#### H1 — API is unauthenticated by default; loopback is the only protection
`crates/tandem-server/src/http/middleware.rs:248-268`, `app_state_impl_parts/part01.rs:229`
Default `LocalSingleTenant` + unset `api_token` ⇒ the entire API is open, including `POST /tool/execute` (arbitrary tool/shell/fetch, `global.rs:433-460`) and `PUT/DELETE /auth/token` (`config_providers_parts/part01.rs:1457-1484`). Any local process, other local user, misconfigured bind, or SSRF pivot gets full control.
**Fix:** generate-and-persist a token on first start even in local mode, or hard-refuse non-loopback binds without an explicit token.

#### H2 — Shell tool has no workspace confinement; "sandbox" is a bypassable substring blocklist
`crates/tandem-core/src/engine_loop/tool_execution.rs:148-159` · `crates/tandem-core/src/engine_loop/tool_output.rs:393-423` · `crates/tandem-tools/src/builtin_tools.rs:261`
Unlike `read`/`write`, `bash` is never confined to the workspace root; `current_dir` is only the *starting* cwd. The sole content check is `command.to_ascii_lowercase().contains(p)` over ~14 literal paths (`/.ssh/`, `id_rsa`, `.npmrc`, …).
**Bypasses:** `cat $HOME/.ss''h/id_rsa`, `f=.ss; cat ~/.${f}h/id_rsa`, `cat /etc/passwd`, `cat /proc/self/environ`, `curl 169.254.169.254/...`, `echo x > /root/.bashrc`. The list also omits `~/.ssh/config`, `~/.aws/config`, kube tokens, cloud metadata.
**Fix:** run shell-domain tools inside an OS sandbox confined to the workspace (namespaces/seccomp/landlock/container/chroot). Do not treat substring matching as a security control.

#### H3 — Automation/routine runs auto-approve shell with an empty (= allow-all) allowlist
`crates/tandem-server/src/app/tasks.rs:1106-1109` · `crates/tandem-core/src/engine_loop.rs:517-543, 679-719` · `crates/tandem-server/src/config/channels.rs:110`
Routine runs call `set_session_auto_approve_permissions(true)`, which silently grants `bash`'s default `Ask` verdict. The only remaining gate is `allowed_tools`, but the check is `if !allowed_tools.is_empty() && !any_policy_matches(...)` — **an empty list disables it entirely**, and `normalize_allowed_tools` does not forbid `bash`/`shell`/`exec`/`*`. With H2, an adversarial/prompt-injected automation gets unconstrained host command execution and zero approvals.
**Fix:** in auto-approve mode fail closed — require a non-empty allowlist, treat empty as deny-all, and never auto-approve `RequiresApproval`/shell tools (route to a real approval queue).

#### H4 — Permission `Ask` gate bypassed for `batch` sub-calls
`crates/tandem-core/src/engine_loop.rs:1022-1160` (governance) and `:659-836` (per-tool gate) · `crates/tandem-tools/src/lib_parts/part05.rs:88-185`
The top-level gate calls `permissions.evaluate()` and prompts on `Ask`. For `batch`, the engine governs each sub-call with only allowlist + write-policy + sandbox checks — it **never calls `permissions.evaluate()` on the sub-tools**. A model emitting `batch{[{tool:"bash",...}]}` runs the nested `bash` with no `Ask` prompt that a direct call would trigger.
**Fix:** evaluate `permissions.evaluate(sub_tool)` inside the batch loop; treat `Ask`/`Deny` as block or surface a real approval request.

#### H5 — Permissive-by-default file writes in the shipped client
`crates/tandem-core/src/permission_defaults.rs:63-83` (loaded at `tandem-tui/src/net/client_parts/part03.rs:15-26`)
`build_mode_permission_rules(None)` grants `write`, `edit`, `apply_patch` action `allow` for pattern `*`. Only `bash` defaults to `ask`. Arbitrary file create/overwrite anywhere in the workspace (source, CI config, `.git/hooks/*`) happens silently out of the box.
**Fix:** default these to `ask`; require explicit opt-in for silent writes.

#### H6 — Workspace sandbox silently disabled when no workspace root resolves (fails open)
`crates/tandem-core/src/engine_loop/tool_execution.rs:97-187`
`workspace_sandbox_violation`/`session_write_policy_violation` use `?` on `session_effective_workspace_root(...)?`; if no root resolves, they return `None` = "no violation" = allowed. `resolve_tool_path` also only blocks *absolute* paths when no root is set — relative paths join `effective_cwd` with no containment check.
**Fix:** fail closed — deny write/exec tools when no workspace root can be resolved.

#### H7 — SSRF via browser tool when host allowlist is empty
`crates/tandem-server/src/browser_parts/part02.rs:468-489` (`ensure_allowed_browser_url`), called from `part01.rs:754`
`if allow_hosts.is_empty() { return Ok(()); }` — empty (the permissive default) allows any host. The private/link-local block (`is_local_or_private_host`, `part02.rs:559`) is applied only on the *write*-action path, **not** on navigate+extract, so a prompt-injected agent can read `http://169.254.169.254/latest/meta-data/...` and exfiltrate via `browser_extract`.
**Fix:** when `allow_hosts` is empty, fail closed; always reject loopback/private/link-local + cloud-metadata IPs on the navigate/read path; re-check the resolved IP (DNS-rebinding).

#### H8 — Audit stream admin gate bypassable via spoofed headers; no tenant filter
`crates/tandem-server/src/http/audit_stream.rs:14-62`
`audit_admin_allowed()` grants admin if the client sends `x-tandem-capability-tier: admin` or `x-tandem-admin: 1` — pure client input. The stream then subscribes to the **global** event bus with no tenant filter. `curl -H 'x-tandem-admin: 1' .../audit/stream` leaks all tenants' tool effects, approvals, and action events.
**Fix:** derive capability tier from the verified principal/grants, not request headers; filter the broadcast by the caller's tenant.

#### H9 — IDOR on `/run/{id}/events`
`crates/tandem-server/src/http/global.rs:1394-1427` (routes `routes_global.rs:43-44`)
Handler takes only `State` + `Path(id)` — no `TenantContext`, no ownership check; it filters the global bus by `runID == id`. Anyone who knows/guesses a run ID streams that run's live events across tenants/sessions.
**Fix:** resolve the run's owning session/tenant and enforce `ensure_same_tenant` before streaming.

#### H10 — World-readable secret/token files (local privilege issue)
`crates/tandem-tui/src/crypto/vault.rs:75-79` and `keystore.rs:67-72` (`std::fs::write`) · `crates/tandem-core/src/engine_api_token.rs:56-63`
`vault.key`, `tandem.keystore`, and the engine API bearer token are written with default umask (typically `0o644`), unlike `provider_auth_store::write_secure_json` which correctly uses `mode(0o600)`. On multi-user hosts, other local users can read the encrypted vault (enabling H11's offline brute force) and the API token (full API access).
**Fix:** write all three with `OpenOptions...mode(0o600)`.

#### H11 — 4-digit PIN protects the AES-256 master key (trivial offline brute force)
`crates/tandem-tui/src/crypto/vault.rs:12-14, 31-72`
`MIN_PIN_LENGTH = MAX_PIN_LENGTH = 4`, digits only → 10,000 keys. With the (world-readable, H10) `vault.key` on disk and the AES-GCM tag as a definitive oracle, all PINs can be tried offline in seconds–minutes regardless of Argon2 cost.
**Fix:** allow/require longer high-entropy passphrases; raise Argon2 cost; consider a hardware-backed factor. Argon2 cannot rescue a 4-digit secret.

### MEDIUM

#### M1 — Path containment is lexical-first → symlink escape + TOCTOU
`crates/tandem-tools/src/lib_parts/part01.rs:968-982` · `crates/tandem-core/src/storage_paths.rs:51-72`
`is_within_workspace_root` does a string `starts_with` check that returns `true` **before** canonicalization, so a path lexically under the root with an intermediate symlink pointing outside is accepted. Checks also run on the path string, then `fs::write/read` happen later (`part01.rs:1255,1289`) — TOCTOU. `workspace/link -> /etc` then write `workspace/link/cron.d/x` escapes. Plain `starts_with` is also prefix-vulnerable (`/work` vs `/work-evil`).
**Fix:** canonicalize the parent first, compare component-wise with a trailing-separator boundary, open with `O_NOFOLLOW`/`openat2(RESOLVE_BENEATH)` (or re-validate the opened fd's canonical path).

#### M2 — Tenant identity is attacker-supplied in local mode
`crates/tandem-server/src/http/middleware.rs:379-400`
`resolve_local_enterprise_request_context` trusts raw `x-tandem-org-id` / `x-tandem-workspace-id` / `x-user-id` headers. The `ensure_same_tenant` checks that protect sessions are therefore defeated by sending matching headers. `list_projects` (`global.rs:1429-1441`) uses `SessionListScope::Global` with no tenant filter. (Hosted/Enterprise modes correctly require signed Ed25519 JWS — local-mode only.)
**Fix:** bind tenant to an authenticated identity in local mode; scope `list_projects` to the caller.

#### M3 — Token-management endpoints unauthenticated in default mode; non-constant-time token compare
`crates/tandem-server/src/http/config_providers_parts/part01.rs:1457-1484` · `crates/tandem-server/src/http/middleware.rs:267`
In the no-token default, `set/clear/generate_api_token` are reachable unauthenticated — an attacker can set their own token (lockout) or clear a configured one. Token check uses `==` (`extract_request_token(...).as_deref() == Some(expected)`), timing-variable, unlike `subtle::ConstantTimeEq` used in `tandem-channels/src/signing.rs:120,150`.
**Fix:** gate token management behind a bootstrap secret; use constant-time comparison.

#### M4 — Narrow SSRF guard, applied to one endpoint only
`crates/tandem-server/src/http/config_providers_parts/part01.rs:1737-1790` (`validate_provider_url`, called once at `:1943`)
Prefix/string matching only — bypassable via decimal/octal/hex IPs (`http://2130706433/`), IPv6 (`[::ffff:169.254.169.254]`), and DNS names resolving to internal IPs (rebinding). `169.254.169.254` blocked only by string prefix. `/tool/execute` fetch/browser/MCP URLs aren't covered.
**Fix:** parse the URL, resolve to IPs, reject all loopback/link-local/private/reserved post-resolution, and reconnect to the validated IP.

#### M5 — `apply_patch` targets invisible to the engine sandbox
`crates/tandem-core/src/engine_loop/prompt_helpers.rs:457-465` (returns `&[]` for `apply_patch`); enforcement only in `crates/tandem-tools/src/lib_parts/part04.rs:912-940`, skipped when no git root resolves.
`extract_tool_candidate_paths` yields no paths for `apply_patch`, so sandbox/sensitive-path containment never inspects patch targets; the only check is the tool's own logic, which is inert on non-git workspaces.
**Fix:** parse `patchText` (reuse `extract_apply_patch_paths`) so the engine checks apply_patch targets like any other write.

#### M6 — MCP sandbox exemption by display name + env var
`crates/tandem-core/src/engine_loop/prompt_helpers.rs:485-497`
Servers named in `TANDEM_MCP_SANDBOX_EXEMPT_SERVERS` (or matching built-in names) are fully exempt from path containment, matched by attacker-influenceable display name only.
**Fix:** bind exemptions to verified server identity; still apply sensitive-path denial to exempt servers.

#### M7 — Unbounded MCP HTTP response body (DoS)
`crates/tandem-runtime/src/mcp_parts/part02.rs:590-593` (and `:497`)
`response.text().await` has a timeout but no byte cap; a malicious/compromised (or redirecting) remote MCP server can return a multi-GB body and exhaust memory, then stress `serde_json::from_str::<Value>`.
**Fix:** enforce a hard byte cap (reject oversized `Content-Length`, cap the read) and bound JSON size/nesting before parsing.

#### M8 — Weak/incorrect redaction in bug-monitor log parser
`crates/tandem-server/src/bug_monitor/log_parser.rs:341-360` (`redact_text`)
Only the **first** occurrence of each needle is redacted (uses `find`, no loop); the `authorization: bearer ` branch computes the end offset inside the matched phrase, likely failing to redact the actual token. Narrower than the regex redaction in `tandem-channels/src/redaction.rs`.
**Fix:** reuse the regex patterns, redact all occurrences, fix the bearer offset.

#### M9 — Credential structs derive `Debug` with plaintext secrets
`crates/tandem-core/src/provider_auth_store.rs:202-220` (`ApiKeyProviderCredential`, `OAuthProviderCredential`)
Both `#[derive(Debug)]` with `token`/`access_token`/`refresh_token`/`api_key` in plaintext, unlike `ResolvedBearerToken` which redacts. Any `{:?}`/panic/error capture leaks the secret.
**Fix:** implement a redacting `Debug` (consider `zeroize` on drop).

### LOW

- **L1 — Permission precedence is last-match, not deny-wins.** `crates/tandem-core/src/permissions.rs:79-90` iterates `.rev()` and returns first match; a later `Allow *` shadows an earlier `Deny bash`, and `reply("always")` pushes `Allow` rules to top precedence. *Fix:* evaluate Deny first.
- **L2 — "Always allow bash" = standing arbitrary-shell approval.** `crates/tandem-core/src/permissions.rs:190-214` — rules match the tool name, not the command. *Fix:* scope persisted shell approvals by command prefix or disallow "always" for shell.
- **L3 — `read` basename fallback can surface unnamed files.** `crates/tandem-tools/src/lib_parts/part01.rs:1038-1096` — `ends_with` matching + auto-resolution; sensitive-path denial not re-applied to the fallback result (e.g. `.env`). *Fix:* re-run `is_sensitive_path_candidate` on the resolved path; require exact basename.
- **L4 — `git worktree add` lacks `--` before `base`.** `crates/tandem-server/src/runtime/worktrees.rs:188-199` — if `base` ever becomes caller-controlled and starts with `-`, argument injection. *Fix:* insert `--`, reject leading-`-` values.
- **L5 — Telegram secret-token compare leaks length.** `crates/tandem-channels/src/signing.rs:147-150` — early length check before `ct_eq`. *Fix:* hash both sides to equal length first.
- **L6 — CORS predicate uses `starts_with` for `*`-suffixed origins.** `crates/tandem-server/src/http/router.rs:11-54` — `https://app.example.com*` would also match `...com.attacker.com`. (No `allow_credentials`, localhost-only default — low impact.) *Fix:* exact origin / proper host parsing.
- **L7 — Provider base URLs not scheme-validated.** A user-configured remote provider with `http://` transmits API keys in cleartext. *Fix:* warn/reject non-`https` for non-loopback hosts.
- **L8 — Unmaintained dependencies.** `serde_yaml 0.9.34+deprecated`, `yaml-rust 0.4.5`, `instant 0.1.13`, `proc-macro-error 1.0.4` (advisory/maintenance only, no known exploit). *Fix:* migrate (`serde_yaml` → `serde_yml`/`serde_norway`; drop `instant`).

---

## Verified NOT vulnerable (defended correctly)

- **Channel webhook authenticity** — Slack HMAC-SHA256 (5-min replay window, constant-time), Discord Ed25519, Telegram secret token; all verified before body parse, fail closed when unconfigured, plus channel allowlist/capability checks (`tandem-channels/src/signing.rs`, `*_interactions.rs`).
- **SQL (tandem-memory)** — all queries use rusqlite `params!`; the few `format!` statements use hardcoded identifiers or `"`-escaped quoted identifiers; tenant scope via `?N`.
- **Google Drive connector** — fixed base URL, ID/token/mime validation, escaped `q` filter, URL-encoded IDs, writes to the parameterized DB (no path traversal).
- **Hosted/Enterprise auth** — signed Ed25519 JWS tenant assertions; raw tenant headers rejected; issuer/audience/expiry/kid/key-status validated.
- **Enterprise `EnvSecretResolver`** — validates env-var names against traversal/injection, enforces tenant scoping, avoids leaking secret names.
- **`unsafe` blocks** — sqlite-vec registration (documented fn-pointer transmute) and ripgrep mmap (idiomatic read-only); the rest are test-only env-var sets. No memory-safety issues.
- **LLM cwd spoofing** — the engine unconditionally overwrites `__workspace_root`/`__effective_cwd` with server-derived values before execution (`engine_loop.rs:860-877`); the override grant is capped at 10 minutes.
- **Crypto primitives** — AES-256-GCM with CSPRNG salts/nonces (12-byte, per-entry random); Argon2id KDF; secrets sent via headers not URLs; body capped at 10 MiB.
- **Path traversal in FS HTTP endpoints** — `sanitize_relative_subpath` rejects absolute/`..`/root/prefix (symlinks unresolved is the residual M1 risk).

---

## Recommended Remediation Order

1. **C1** — verify the Codex JWT signature (forgeable identity).
2. **C2 / H1 / M3** — make the API authenticated by default: generate+persist a local token, refuse non-loopback binds without one, constant-time compare, gate token-management.
3. **H2 / H3** — OS-sandbox shell tools to the workspace; make auto-approve fail closed and treat empty allowlists as deny-all.
4. **H10 / H11** — `0o600` on vault/keystore/token files; raise PIN entropy + Argon2 cost.
5. **H4 / H5 / H6** — evaluate permissions for batch sub-calls; default writes to `ask`; fail closed when no workspace root.
6. **H7 / H8 / H9 / M2** — browser SSRF fail-closed; derive audit-stream tier + tenant from verified principal; ownership check on `/run/{id}/events`; don't trust tenant headers in local mode.
7. **M1 / M4–M9** and the **Low** items as hardening.

> **Cross-cutting theme:** Tandem treats `127.0.0.1` + a single approval click as the trust boundary, but the shell tool isn't confined, automations auto-approve it, batch skips the gate, and the API has no auth by default. The highest-leverage investment is an **OS-level sandbox for tool execution** plus **secure-by-default** auth and write policies.
