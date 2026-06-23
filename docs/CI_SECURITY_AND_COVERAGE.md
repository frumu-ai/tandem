# CI Security And Coverage

TAN-221 adds a Rust quality lane for supply-chain checks and governance-critical
coverage reporting.

## Pull Request Checks

- `Cargo Deny` is a required PR gate for Rust dependency licenses, duplicate
  dependency bans, and source policy. Its advisory check follows the same
  warning-on-PR policy as `cargo audit`.
- `Cargo Audit` runs on PRs in warning mode. A PR can continue while an advisory
  is triaged, but the same scan fails scheduled and manual runs.

## Nightly And Manual Checks

The `Rust Security and Coverage` workflow runs nightly and by
`workflow_dispatch`.

- `cargo audit` fails on advisories unless an accepted advisory is listed in
  `.cargo/audit.toml`.
- `cargo deny check --config .config/deny.toml licenses bans sources` and
  `cargo deny check --config .config/deny.toml advisories` fail on
  scheduled/manual policy violations.
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

| Advisory                                                                                                | Crate family                                  | Owner                    | Reason / mitigation                                                                                                                                 | Expires    |
| ------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| `RUSTSEC-2024-0411` - `RUSTSEC-2024-0420`                                                               | GTK3/Tauri Linux stack                        | Desktop runtime          | Tauri's current Linux stack still pulls archived gtk-rs GTK3 crates. Keep desktop builds patched through Tauri updates; revisit on Tauri/GTK4 path. | 2026-09-30 |
| `RUSTSEC-2024-0320`, `RUSTSEC-2025-0141`                                                                | `yaml-rust`, `bincode` via `syntect`/`ppt-rs` | Desktop document preview | No direct safe upgrade in the current `ppt-rs`/`syntect` chain. Replace or isolate PowerPoint preview parsing before expiry.                        | 2026-09-30 |
| `RUSTSEC-2024-0370`, `RUSTSEC-2024-0388`                                                                | `proc-macro-error`, `derivative`              | Desktop runtime          | Transitive macro/helper crates through GTK/D-Bus dependencies. Remove with upstream desktop dependency refresh or replacement.                      | 2026-09-30 |
| `RUSTSEC-2024-0384`, `RUSTSEC-2025-0057`, `RUSTSEC-2025-0119`, `RUSTSEC-2024-0436`                      | Utility transitive crates                     | Runtime dependencies     | Unmaintained transitive helpers with no direct security exploit in Tandem paths. Prefer upstream dependency updates before adding direct forks.     | 2026-09-30 |
| `RUSTSEC-2025-0075`, `RUSTSEC-2025-0080`, `RUSTSEC-2025-0081`, `RUSTSEC-2025-0098`, `RUSTSEC-2025-0100` | `rust-unic` via Tauri `urlpattern`            | Desktop runtime          | Tauri URL pattern parsing currently depends on unmaintained `rust-unic` crates. Remove through upstream Tauri/urlpattern update.                    | 2026-09-30 |
| `RUSTSEC-2025-0134`                                                                                     | `rustls-pemfile` 1.x                          | Runtime dependencies     | Transitive through legacy `reqwest` 0.11 chain. Keep TLS-sensitive paths on newer reqwest/rustls where possible; replace legacy dependency.         | 2026-09-30 |
| `RUSTSEC-2026-0105`                                                                                     | `core2` via `image`/`rav1e`                   | Runtime dependencies     | Yanked/unmaintained transitive crate through image encoding dependencies. Track upstream image/rav1e replacement or version cleanup.                | 2026-09-30 |

### Current License Exceptions

| Crate                      | License               | Owner                | Reason                                                                                                                                                | Expires    |
| -------------------------- | --------------------- | -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| `tandem-plan-compiler`     | `BUSL-1.1`            | Runtime governance   | Tandem-owned source-available compiler crate documented in `docs/LICENSING.md`.                                                                       | 2027-06-30 |
| `tandem-governance-engine` | `BUSL-1.1`            | Runtime governance   | Tandem-owned source-available governance crate documented in `docs/LICENSING.md`.                                                                     | 2027-06-30 |
| `auto_generate_cdp`        | `GPL-3.0-or-later`    | Browser runtime      | Existing `headless_chrome` build dependency for CDP protocol generation; replace or re-validate before raising license gates further.                 | 2026-09-30 |
| `html2md`                  | `GPL-3.0-or-later`    | Tools runtime        | Existing HTML-to-Markdown converter used by browser/tools extraction paths; replace with a permissive converter before hardening distribution policy. | 2026-09-30 |
| `libfuzzer-sys`            | `NCSA`                | Runtime dependencies | OSI-approved permissive transitive dependency through `rav1e`/`image`; keep scoped by crate name.                                                     | 2027-06-30 |
| `webpki-root-certs`        | `CDLA-Permissive-2.0` | Runtime dependencies | Permissive root certificate data dependency through `rustls-platform-verifier`/`reqwest`; keep scoped by crate name.                                  | 2027-06-30 |
| `webpki-roots`             | `CDLA-Permissive-2.0` | Runtime dependencies | Permissive Mozilla root certificate data dependency through TLS clients; keep scoped by crate name.                                                   | 2027-06-30 |

## Coverage Baselines

`.config/coverage-baseline.json` stores governance-critical baseline floors.
Initial floors are intentionally report-only. Raise a crate baseline only after
linking a passing `governance-coverage` artifact in the PR description.

Do not fail PRs on absolute coverage percentages yet. Once baselines are stable,
future work can make negative deltas fail for the governance-critical crates.
