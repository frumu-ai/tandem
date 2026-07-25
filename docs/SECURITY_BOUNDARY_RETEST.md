# Focused security-boundary retest

The focused retest is an executable evidence index for TA-01 through TA-17. The
authoritative matrix is `SECURITY_BOUNDARY_RETEST_MATRIX.json`; it maps every
finding to at least one denied exploit control and one intended-behavior control
using exact checked-in Rust test symbols.

The fixtures cover two tenants and three authority roles, temporary workspaces,
Git repositories and pack roots, marker credentials/audit values, protected
audit and persistence failure injection, fake private/cloud-metadata DNS
answers, a redirect-to-metadata service, a chunked over-budget response, and two
replay-store instances.

## Local focused run

Install cargo-nextest, then run:

```bash
bash scripts/run-security-boundary-retest.sh
```

The script validates the matrix and runs only its named tests with the same
browser and premium-governance features as the full workspace CI job. It does
not use production credentials, repositories, pack roots, or endpoints.

## CI evidence

The normal Workspace Tests job remains authoritative because it executes the
entire non-desktop Rust workspace. After nextest completes, CI validates that
every matrix symbol was present in both nextest discovery output and the JUnit
execution report. A renamed, deleted, skipped, filtered-out, or unexecuted
security test therefore fails the gate even if the JSON still references it.

The desktop crate remains covered by its dedicated CI job. TA-14/TA-15 use the
shared artifact-integrity verifier in the focused matrix and the desktop job
executes the desktop installer tests that consume that verifier.

Passing this matrix closes focused implementation validation; it does not by
itself authorize a shared/hosted release. The assurance/dependency/deployment
batch and final release-decision issue remain separate gates.
