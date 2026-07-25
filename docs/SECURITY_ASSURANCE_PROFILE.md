# Security Assurance Profiles

This document separates controls that are proven by repository and local-container evidence from controls that require a real hosted environment. It prevents a passing source audit from being represented as proof of a deployment that does not exist.

## Reviewed deployment-asset inventory

As of TAN-822, the repository contains exactly three deployment assets:

- `packages/tandem-control-panel/docker-compose.yml`
- `packages/tandem-control-panel/docker/control-panel.Dockerfile`
- `packages/tandem-control-panel/docker/engine.Dockerfile`

There are no checked-in Kubernetes, Helm, Kustomize, or Terraform assets. `scripts/verify-container-hardening.mjs` fails when a new deployment asset appears without being added to the reviewed inventory and scanner coverage.

## Local Compose profile

The Compose profile is supported for local or single-host, self-managed use. Its verified controls are:

- a supported Node base pinned to one multi-architecture digest;
- an exact Tandem engine npm version (floating `latest`, `next`, alpha, and beta values fail the image build);
- reproducible control-panel and TypeScript-client builds from the checked-in lockfile;
- unprivileged `node` users in both images;
- read-only root filesystems, all Linux capabilities dropped, `no-new-privileges`, and a constrained temporary filesystem;
- named writable volumes scoped to engine and panel state;
- host-generated mode-`0600` API-token material mounted as one read-only file only into the engine;
- no host-published engine port; the panel is the network entry point.

This profile does not claim PostgreSQL, cloud KMS/IAM, reverse-proxy TLS, multi-replica, or default-deny egress controls. Those are hosted-environment properties, not properties of a standalone Tandem engine.

## Hosted-enterprise profile

No enterprise/shared Tandem server is deployed at the time of this review. Hosted-enterprise readiness is therefore **not validated** and remains fail-closed. This does not expose or block a user's standalone engine, and it does not convert absent infrastructure into a security finding against that local runtime.

Before any hosted-enterprise deployment is released, the exact release commit must pass `.github/workflows/security-release-environment.yml` in the protected `hosted-production-security` environment. The workflow rejects missing, stale, mismatched, or secret-bearing evidence and requires all of the following:

| Control group | Required evidence |
| --- | --- |
| PostgreSQL | TLS connection with full certificate verification |
| KMS/IAM | Envelope encryption, workload identity, and zero static cloud credentials |
| Reverse proxy | TLS 1.2 or newer, HSTS, CSP, frame restrictions, and content-type protection |
| Multi-replica | At least two replicas, failover pass, and cross-replica authorization pass |
| Egress | Default deny, a successful denied probe, and a reviewed allowlist |

Evidence is valid for at most 30 days and is bound to the exact 40-character commit SHA. The validator emits only a redacted summary; raw tokens, credentials, passwords, or private keys are forbidden. Future hosted deployment workflows must consume a passing summary for their exact commit instead of bypassing this gate.

## Release interpretation

- A clean local/container retest may support a standalone or self-managed release decision.
- It cannot support a hosted-enterprise release decision by itself.
- Until real hosted evidence passes, the hosted-enterprise decision remains **NO-GO** while local remediation and PR merging continue normally.

## TAN-822 pre-publication evidence

The following local evidence was collected on 2026-07-25. Pull-request CI must reproduce the repository-enforced checks for the exact published commit before merge.

| Area | Result |
| --- | --- |
| Rust supply chain | `cargo audit` reconciled 1,113 dependencies to 27 owned, expiring exceptions with zero unexpected advisories or yanked packages; Cargo Deny passed advisories, licenses, bans, and sources. |
| JavaScript supply chain | All five checked-in lockfiles passed their native audit with zero reported vulnerabilities. |
| Secret history | Gitleaks 8.28 scanned all 3,406 reachable commits and 110.35 MB of history with zero unreviewed findings; the separate path-policy scan inspected every reachable object. |
| Runtime images | Representative engine and control-panel runtime images produced SPDX SBOMs with Syft 1.42.3 and zero fixable vulnerabilities with Grype 0.110.0; the live Compose smoke verified UID 1000, read-only roots, writable named state volumes, capability removal, `no-new-privileges`, and a read-only engine token. |
| Rust regression | The TUI passed 140 unit tests plus its integration targets; the desktop/Tauri manifest passed 137 unit tests plus binary, integration, and doc-test targets. |
| JavaScript regression | The desktop passed typecheck, lint, production build, and 18 blackboard tests; the TypeScript client passed 61 tests; the control panel passed 137 unit tests. All 166 distinct Playwright desktop/mobile cases passed, with three local high-concurrency timeouts rerun serially. |
| Documentation | Astro 7 type checking completed with zero diagnostics and the production guide built 79 pages. |

This table is development evidence, not a substitute for exact-head CI, CodeQL, container scanning, review-thread clearance, or hosted-environment evidence.
