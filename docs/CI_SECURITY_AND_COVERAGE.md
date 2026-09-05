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
| `RUSTSEC-2024-0370`, `RUSTSEC-2024-0388` | `proc-macro-error`, `derivative` | Desktop runtime | Compile-time/macro or generated helper paths through GTK/D-Bus; no attacker-controlled runtime entry was identified. Remove through upstream desktop dependency refresh. | 2026-09-30 |
| `RUSTSEC-2024-0384`, `RUSTSEC-2024-0436`, `RUSTSEC-2025-0057`, `RUSTSEC-2025-0119` | Utility transitive crates | Runtime dependencies | Unmaintained helpers with no identified Tandem call path that crosses an untrusted boundary. CI pins the lockfile and will reject any new advisory; prefer upstream removal over a direct fork. | 2026-09-30 |
| `RUSTSEC-2025-0075`, `RUSTSEC-2025-0080`, `RUSTSEC-2025-0081`, `RUSTSEC-2025-0098`, `RUSTSEC-2025-0100` | `rust-unic` via Tauri `urlpattern` | Desktop runtime | Limited to Tauri URL-pattern parsing; application navigation and deep links remain allowlisted. Remove through upstream Tauri/urlpattern replacement. | 2026-09-30 |
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

### Foundation dependency repair (TAN-843)

The unused `ppt-rs` dependency was removed; the desktop presentation exporter
already writes its OOXML ZIP directly. This removes the legacy reqwest 0.11 /
h2 0.3 chain and the obsolete yaml-rust, bincode and rustls-pemfile exceptions.
The remaining h2 is pinned to 0.4.16 for
[RUSTSEC-2026-0258](https://rustsec.org/advisories/RUSTSEC-2026-0258.html).
The lockfile also updates lru to 0.18.2 for
[RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html)
and replaces the yanked chacha20 0.10.1 with 0.10.2.
Existing audit gates and severity thresholds remain unchanged.

Engine and panel runtime images now use the 2026-09-04 Debian snapshots and
explicitly pin OpenSSL 3.5.7-1~deb13u2. Package availability was checked against
both snapshot indexes; the [Debian security tracker](https://security-tracker.debian.org/tracker/source-package/openssl)
identifies this as the patched trixie-security package. Runtime and migration
images use the official Node 24.20.0 trixie-slim multi-architecture digest
`sha256:50c3b2f6988dfc307b86e5301d69611af31f4789bdf232863b07d3b02fe55ae0`,
verified against [Docker's image inventory](https://github.com/docker-library/repo-info/blob/master/repos/node/remote/24.20.0-trixie-slim.md).
This replaces the Node 24.18.0 binary flagged by the second container scanner.
Non-root users, artifact verification and vulnerability gates remain enforced.

### Pull request release-pin checks

The shared release producer uses `scripts/ci-engine-pin-scope.mjs` to compare only
the engine release version and binary SHA-256. Updating the Node base, Debian
snapshot or OS packages does not require a newly built candidate to match a
previously published engine binary. Version or binary-pin changes still enable
that comparison; missing, malformed or duplicate pins fail classification.
Candidate images verify their computed candidate digest, and tagged releases
retain their unconditional release-pin verification.

The main PR workflows cancel superseded runs for the same PR. Push, scheduled
and manually dispatched runs use independent groups. This reduces stale work
without removing test suites, advisory gates or release checks.


### Shared CI builds and enforcing gates (TAN-843)

`Security Assurance` owns the `Linux Enterprise Release Composition` job. It
invokes the existing network-isolated, pinned Rust 1.95.0 builder once for the
standard and enterprise-full compositions. `Engine CI` no longer duplicates
this release build. Run `Security Assurance` manually when validating release
composition; manual `Engine CI` covers runtime checks and workspace tests.

`Container engine` downloads the producer's immutable artifact ID from the
same workflow run. Before copying or executing the binary, it checks the
producer-supplied manifest and binary SHA-256 values, checkout SHA (the merge
commit on a PR), workflow run, producer attempt, target/features, Cargo.lock,
Docker builder definition and build script. It then passes that same binary
SHA-256 into the existing Docker verification. A consumer-only rerun may use
the successful producer attempt from that same run; it cannot select another
run or a mutable artifact name. Missing artifacts, mismatched provenance and
failed producer jobs stop validation. This is CI artifact integrity evidence,
not a production release signature or permission to publish an artifact.

The panel and builder scans run independently of the Rust producer. All three
images use `.github/actions/container-assurance` for the same SBOM generation
and fixable high/critical vulnerability enforcement. `Security Assurance
Result` fails when any assurance job fails, is cancelled, or is skipped. It is
an aggregate check available for branch protection; this change does not edit
repository protection settings or remove the existing named checks.

PR quality jobs use Rust 1.98.0, the compiler on the verified foundation runs.
The default is in `.github/actions/setup-rust-ci/action.yml`; direct installs
in Rust Security, Enforcement Model Drift and generated coverage are pinned
to the same version. The scheduled/manual `Rust Toolchain Canary` deliberately
uses current stable and fails visibly on new compiler/lint problems without
changing a PR gate overnight. Upgrade the quality pins in a reviewed PR after
the canary and full quality suites pass. This does not change the documented
minimum supported Rust version or the separate tagged-release toolchains.

Eval jobs now cache the workspace's actual `target` directory through the
shared `eval-release` profile. The email approval demo and runtime smoke use
`engine-default`; the ACME feature composition has its own `acme-demo` cache.
Caches remain an optimization, never a substitute for executing a check.
Desktop Clippy now enforces `-D warnings`; the three existing lint errors were
fixed before removing its `|| true` fallback.

The verified baseline before this cleanup was:

| Foundation PR | Checks | Sum of reported check durations | Enterprise build | Engine build + scan |
| --- | --- | --- | --- | --- |
| #1931 at `4b37048e` | 31 passed, 3 intentionally skipped | 209 minutes | 34m 55s | 22m 10s |
| #1932 at `2f1676dc` | 31 passed, 3 intentionally skipped | 192 minutes | 27m 50s | 20m 33s |

These are observed check durations, including review checks, not wall-clock
latency or a billing estimate. Compare the cleanup run against these numbers,
especially release/container execution and queue time; do not claim measured
savings before the shared artifact has been built and scanned successfully.
The engine scan now waits for the shared producer, which can slightly extend
that path even while removing a complete duplicate release build.

The first cleanup slice keeps conservative execution: standard and
enterprise-full builds run on every Security Assurance invocation (all PRs,
main/feat-engine pushes and manual runs). This expands enterprise coverage for
changes that previously only built the standard image. Component-based
selection and separating fast PR checks from expensive full-platform checks
remain TAN-843 follow-ups; they need an explicit, tested result gate before
any security or runtime suite may be skipped. The full workspace, migration,
isolation, approvals, browser and advisory suites remain in place.
