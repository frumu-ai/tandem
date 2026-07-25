# CI Security And Coverage

TAN-221 adds a Rust quality lane for supply-chain checks and governance-critical
coverage reporting.

## Pull Request Checks

- `Cargo Deny` is a required PR gate for Rust dependency licenses, duplicate
  dependency bans, source policy, and reviewed advisory exceptions.
- `Cargo Audit` is a required PR gate. The ignore-free verifier rejects
  advisories that are not explicitly owned, controlled, documented, and
  unexpired; the configured scan then rejects anything outside that reviewed
  exception set.
- `node scripts/audit-javascript-workspaces.mjs` is a required PR gate. It
  rejects untracked lockfile additions and any production or development
  advisory in the desktop, guide, TypeScript SDK, control panel, or benchmark
  workspace.
- The desktop blackboard suite is required and must execute its compiled test
  files; a zero-test result is not accepted as coverage evidence.
- The `Security Assurance` workflow scans all Git refs with a digest-pinned,
  network-isolated Gitleaks image and explicit Tandem token patterns. Exact
  historical fixture fingerprints live in `.gitleaksignore`; a changed copy
  receives a new fingerprint and fails.
- The same workflow rejects dangerous secret-file paths across every Git ref,
  runs CodeQL `security-extended` JavaScript/TypeScript analysis, verifies the
  complete deployment-asset inventory, builds both images, emits SPDX JSON
  SBOMs, and fails on fixable high/critical image vulnerabilities.
- Container images must use supported digest-pinned bases, exact package
  versions, non-root users, read-only filesystems, dropped capabilities, and a
  single read-only API-token mount. `scripts/verify-container-hardening.mjs`
  makes those controls and the no-Kubernetes/no-Terraform inventory explicit.

## Nightly And Manual Checks

The `Rust Security and Coverage` workflow runs nightly and by
`workflow_dispatch`.

- `node scripts/verify-rustsec-report.mjs` runs Cargo Audit without repository
  ignores and fails unless every reported advisory is present in the reviewed,
  unexpired exception table below. It also rejects yanked packages.
- `cargo audit` then fails on every advisory not listed in `.cargo/audit.toml`.
- `cargo deny --config .config/deny.toml check licenses bans sources` and
  `cargo deny --config .config/deny.toml check advisories` fail on
  scheduled/manual policy violations (cargo-deny ≥ 0.20 takes `--config` on
  the root command; the version is pinned in the workflow).
- `cargo llvm-cov nextest` runs coverage for `tandem-tools`,
  `tandem-plan-compiler`, and `tandem-automation`, uploads `lcov.info`, and
  writes a per-crate summary artifact.

## Exception Process

Advisory, license, source, and ban exceptions must be temporary and auditable.

1. Add the smallest exception to `.cargo/audit.toml` or `.config/deny.toml`.
2. Include a comment next to the exception or in the PR body with the owner,
   reason, mitigation, and expiry date.
3. Link the upstream advisory, crate issue, or license evidence.
4. Add or update a Linear follow-up before merging the exception.

BUSL exceptions are allowed only for Tandem-owned source-available crates listed
in `docs/LICENSING.md`.

### Current Advisory Exceptions

The verifier requires every ignored ID to appear exactly once in this table,
with a non-empty owner, reachability/compensating-control statement, and a
future expiry. Each ID maps to `https://rustsec.org/advisories/<ID>.html`.

| Advisory IDs | Crate family | Owner | Reachability / compensating control | Expires |
| --- | --- | --- | --- | --- |
| `RUSTSEC-2024-0411`, `RUSTSEC-2024-0412`, `RUSTSEC-2024-0413`, `RUSTSEC-2024-0414`, `RUSTSEC-2024-0415`, `RUSTSEC-2024-0416`, `RUSTSEC-2024-0417`, `RUSTSEC-2024-0418`, `RUSTSEC-2024-0419`, `RUSTSEC-2024-0420`, `RUSTSEC-2024-0429` | GTK3/Tauri Linux stack | Desktop runtime | Reachable only in the Linux desktop GTK runtime. Tandem does not directly call the affected archived APIs or `VariantStrIter`; keep Tauri patched, exercise Linux desktop CI, and replace this stack before expiry. | 2026-09-30 |
| `RUSTSEC-2024-0320`, `RUSTSEC-2025-0141` | `yaml-rust`, `bincode` via `syntect`/`ppt-rs` | Desktop document preview | Reachable only while parsing local document-preview input. The preview remains local/user-initiated; replace or isolate PowerPoint preview parsing before expiry. | 2026-09-30 |
| `RUSTSEC-2024-0370`, `RUSTSEC-2024-0388` | `proc-macro-error`, `derivative` | Desktop runtime | Compile-time/macro or generated helper paths through GTK/D-Bus; no attacker-controlled runtime entry was identified. Remove through upstream desktop dependency refresh. | 2026-09-30 |
| `RUSTSEC-2024-0384`, `RUSTSEC-2024-0436`, `RUSTSEC-2025-0057`, `RUSTSEC-2025-0119` | Utility transitive crates | Runtime dependencies | Unmaintained helpers with no identified Tandem call path that crosses an untrusted boundary. CI pins the lockfile and will reject any new advisory; prefer upstream removal over a direct fork. | 2026-09-30 |
| `RUSTSEC-2025-0075`, `RUSTSEC-2025-0080`, `RUSTSEC-2025-0081`, `RUSTSEC-2025-0098`, `RUSTSEC-2025-0100` | `rust-unic` via Tauri `urlpattern` | Desktop runtime | Limited to Tauri URL-pattern parsing; application navigation and deep links remain allowlisted. Remove through upstream Tauri/urlpattern replacement. | 2026-09-30 |
| `RUSTSEC-2025-0134` | `rustls-pemfile` 1.x | Runtime dependencies | Transitive through the legacy `reqwest` 0.11 chain. TLS-sensitive Tandem paths use the newer pinned reqwest/rustls clients; replace the legacy dependency before expiry. | 2026-09-30 |
| `RUSTSEC-2026-0097` | `rand` 0.7 via `selectors` code generation | Desktop build | Build-time-only path through Tauri HTML selector code generation. The advisory requires a custom logger that re-enters `rand::thread_rng()` during reseed; that precondition is absent from the generator. | 2026-09-30 |
| `RUSTSEC-2026-0192` | `ttf-parser` via `lopdf`/`pdf-extract` | Desktop document preview | Reachable only for local document-preview font parsing. Updated parser parents remain pinned; preview input is local/user-initiated and the parser must be replaced before expiry. | 2026-09-30 |

### Current License Exceptions

| Crate                      | License               | Owner                | Reason                                                                                                                                                | Expires    |
| -------------------------- | --------------------- | -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| `tandem-plan-compiler`     | `BUSL-1.1`            | Runtime governance   | Tandem-owned source-available compiler crate documented in `docs/LICENSING.md`.                                                                       | 2027-06-30 |
| `tandem-governance-engine` | `BUSL-1.1`            | Runtime governance   | Tandem-owned source-available governance crate documented in `docs/LICENSING.md`.                                                                     | 2027-06-30 |
| `tandem-incident-monitor`  | `BUSL-1.1`            | Runtime governance   | Tandem-owned source-available incident-monitor crate documented in `docs/LICENSING.md`.                                                               | 2027-06-30 |
| `tandem-enterprise-server` | `BUSL-1.1`            | Runtime governance   | Tandem-owned source-available enterprise-server crate documented in `docs/LICENSING.md`.                                                              | 2027-06-30 |
| `tandem-server`            | `BUSL-1.1`            | Runtime governance   | Tandem-owned source-available engine server crate, relicensed for 0.7.0, documented in `docs/LICENSING.md`.                                           | 2027-06-30 |
| `auto_generate_cdp`        | `GPL-3.0-or-later`    | Browser runtime      | `headless_chrome`'s CDP protocol codegen; confirmed (TAN-628) to be a build-dependency only — it runs at compile time and is never linked into a shipped binary, so its own code is not part of any distributed artifact. Re-verify with `cargo tree -i auto_generate_cdp` on `headless_chrome` upgrades. | 2027-06-30 |
| `libfuzzer-sys`            | `NCSA`                | Runtime dependencies | OSI-approved permissive transitive dependency through `rav1e`/`image`; keep scoped by crate name.                                                     | 2027-06-30 |
| `webpki-root-certs`        | `CDLA-Permissive-2.0` | Runtime dependencies | Permissive root certificate data dependency through `rustls-platform-verifier`/`reqwest`; keep scoped by crate name.                                  | 2027-06-30 |
| `webpki-roots`             | `CDLA-Permissive-2.0` | Runtime dependencies | Permissive Mozilla root certificate data dependency through TLS clients; keep scoped by crate name.                                                   | 2027-06-30 |

## Coverage Baselines

`.config/coverage-baseline.json` stores governance-critical baseline floors.
Initial floors are intentionally report-only. Raise a crate baseline only after
linking a passing `governance-coverage` artifact in the PR description.

Do not fail PRs on absolute coverage percentages yet. Once baselines are stable,
future work can make negative deltas fail for the governance-critical crates.

## Deployment evidence boundary

Repository CI proves the local Compose profile described in
`docs/SECURITY_ASSURANCE_PROFILE.md`. There is currently no hosted enterprise
environment from which PostgreSQL, KMS/IAM, reverse-proxy, multi-replica, or
egress evidence can be collected. `.github/workflows/security-release-environment.yml`
therefore fails closed unless fresh, exact-commit evidence is supplied through
the protected `hosted-production-security` environment. This gate applies to a
future hosted-enterprise deployment, not to standalone engines.
