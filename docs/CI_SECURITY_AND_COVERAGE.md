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

- `cargo audit --config .config/audit.toml` fails on advisories unless an
  accepted advisory is listed in `.config/audit.toml`.
- `cargo deny --config .config/deny.toml check licenses bans advisories` fails
  on scheduled/manual policy violations.
- `cargo llvm-cov nextest` runs coverage for `tandem-tools`,
  `tandem-plan-compiler`, and `tandem-automation`, uploads `lcov.info`, and
  writes a per-crate summary artifact.

## Exception Process

Advisory, license, source, and ban exceptions must be temporary and auditable.

1. Add the smallest exception to `.config/audit.toml` or `.config/deny.toml`.
2. Include a comment next to the exception or in the PR body with the owner,
   reason, mitigation, and expiry date.
3. Link the upstream advisory, crate issue, or license evidence.
4. Add or update a Linear follow-up before merging the exception.

BUSL exceptions are allowed only for Tandem-owned source-available crates listed
in `docs/LICENSING.md`.

## Coverage Baselines

`.config/coverage-baseline.json` stores governance-critical baseline floors.
Initial floors are intentionally report-only. Raise a crate baseline only after
linking a passing `governance-coverage` artifact in the PR description.

Do not fail PRs on absolute coverage percentages yet. Once baselines are stable,
future work can make negative deltas fail for the governance-critical crates.
