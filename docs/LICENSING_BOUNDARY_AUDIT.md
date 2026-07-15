# Licensing Boundary and Contributor-Risk Audit

**Scope:** repository snapshot prepared for the prospective 0.7.0 BUSL
Additional Use Grant correction. This is a technical/documentation audit, not
legal advice or a conclusion that Frumu LTD can relicense any contribution.

## Boundary findings

The protected components declared as `BUSL-1.1` are:

- `tandem-plan-compiler`
- `tandem-governance-engine`
- `tandem-enterprise-server`
- `tandem-incident-monitor`
- `tandem-server` (relicensed from `MIT OR Apache-2.0` effective 0.7.0,
  resolving the highest-risk boundary finding below; earlier releases remain
  under their shipped permissive terms)

The following packages were identified as exposing functionality near the
intended commercial boundary. The `tandem-server` finding was resolved by
relicensing it for 0.7.0; the remaining rows are findings for a separate
relicensing or architecture review and their licenses are unchanged.

| Package | Path | Current license | Relevant functionality | Boundary assessment | Separate review |
| --- | --- | --- | --- | --- | --- |
| `tandem-enterprise-contract` | `crates/tandem-enterprise-contract` | `MIT OR Apache-2.0` | Public tenant, authority, approval-receipt, protected-action, policy-inheritance/predicate, source-ACL, and verifier-keyring contracts. | It intentionally exposes interoperable enterprise vocabulary, but some deterministic policy and keyring semantics may be commercially sensitive. | **Recommended, targeted.** Preserve genuinely public contracts if desired; review whether executable policy/keyring logic should stay with them. |
| `tandem-server` | `crates/tandem-server` | `BUSL-1.1` (since 0.7.0) | Engine HTTP routes, audit and approval handling, enterprise connector modules, multi-tenant state, automation scheduling, and optional `premium-governance`; directly depends on protected plan-compiler and incident-monitor crates. | Previously the highest-risk permissive boundary: it contains substantial production governance plumbing and composes protected components. | **Resolved for 0.7.0 by relicensing to `BUSL-1.1`.** Versions released before 0.7.0 remain under their shipped permissive terms. A future extraction of genuinely public server interfaces into a permissive crate remains optional. |
| `tandem-automation` | `crates/tandem-automation` | `MIT OR Apache-2.0` | Durable, scheduled, unattended workflow/orchestration support and a dependency on `tandem-plan-compiler`. | Persistent workflow execution may be commercially sensitive even though the package is also useful as a general automation contract. | **Recommended.** Review the package-to-BUSL dependency boundary and release composition. |
| `tandem-memory` and `tandem-data-boundary` | `crates/tandem-memory`, `crates/tandem-data-boundary` | `MIT OR Apache-2.0` | Tenant/subject-scoped memory, protected data classification, and boundary contracts used by hosted and enterprise flows. | Multi-tenant data isolation and data-boundary code can be differentiated enterprise value, but can also be useful public infrastructure. | **Recommended, product-led.** Decide which portions are foundational versus commercial controls before relicensing. |
| `@frumu/tandem-panel` | `packages/tandem-control-panel` | `MIT OR Apache-2.0` | Enterprise admin UI for org units, grants, connector credentials, source bindings, ingestion quarantine, and policy authoring. | The UI exposes substantial enterprise administration, while the server remains the enforcement point. | **Recommended.** Review the distribution and product-boundary rationale; do not relicense automatically. |
| `@frumu/tandem` and `@frumu/tandem-enterprise` | `packages/tandem-engine`, `packages/tandem-enterprise` | `MIT OR Apache-2.0` | Installers/launchers that distribute engine binaries compiled with all five protected components. | The installer source is not itself the protected implementation, but its distributed binary is mixed-license. | **Packaging review only.** Keep the mixed-license notice accurate; a source-license change is not indicated by this audit. |
| `tandem-ai` / `tandem-engine` | `engine` | `MIT OR Apache-2.0` | Release composition enables enterprise routes, premium governance, and connectors in every engine artifact. | The binary is correctly mixed-license, but the assembly crate makes the package-to-binary boundary easy to misunderstand. | **Recommended, release-boundary review.** Continue conspicuous binary disclosure and test it on every release. |

No separately named fleet-management package was identified in the current
workspace map. Fleet, hosted-control-plane, production-identity, audit,
compliance, and connector governance behavior is primarily distributed across
the server, enterprise contract, control panel, and protected enterprise-server
crates. A future product-boundary review should evaluate those features by
behavior, not only by package name.

## Corrected public statements

The prospective policy removes broad no-charge commercial internal-production
claims from the root notice, licensing guide, enterprise installer README, and
current release material. Historical changelog and release-note entries are
now explicitly identified as applying only to the releases that distributed
them; they do not override the 0.7.0-and-later terms.

The public grant does not make an Enterprise Pilot mandatory. Frumu LTD may use
an Enterprise Pilot, Community Production License, or a later commercial plan
to authorize production use, subject to separate terms.

## Contributor and copyright-risk summary

### Evidence reviewed

- `git log` for each of the protected component paths.
- Author identities recorded in that history.
- Copyright/SPDX-style headers in those paths.
- Repository contributor-policy files and references to a CLA, copyright
  assignment, or DCO.

### Factual observations

- The protected-path history is predominantly attributed to `tacshade` or
  `TacShade` using `198919272+tacshade@users.noreply.github.com`, with an older
  `tacshade@users.noreply.github.com` identity.
- It also includes automated release commits attributed to
  `github-actions[bot]` and commits attributed to `Claude <noreply@anthropic.com>`.
- Many source files in the protected paths contain `Copyright (c) 2026 Frumu
  LTD` headers, but headers are not present uniformly across every file.
- The repository contains `CONTRIBUTING.md`, but this audit found no repository
  CLA, copyright-assignment agreement, DCO/sign-off policy, or equivalent
  contributor-rights statement.

### Risk and required follow-up

Git authorship and file headers do not establish ownership, work-for-hire
status, assignment, or the right to change terms for every contribution. Before
publishing a release with the corrected grant, Frumu LTD should have qualified
counsel confirm contributor provenance and rights for the protected
components, including the source and authorization basis for automated or
AI-attributed commits. If any non-Frumu contributor retains relevant rights,
their approval or a valid contributor agreement may be required.

## Legal questions to resolve before release

1. Confirm the final Additional Use Grant and the scope of "commercial
   organization," "personal," "non-commercial," and "Production Use."
2. Confirm that personal and agency evaluation language does not create an
   employer, client, managed-service, OEM, or reseller loophole in the intended
   jurisdictions.
3. Confirm the commercial authorization needed for client deployments and the
   form of future Community Production, partner, OEM, and hosted agreements.
4. Confirm ownership/provenance and any contributor approval requirement for
   all protected-component contributions.
5. Confirm that 0.7.0 is the correct release vehicle and that its pre-release
   Change Date is verified against the final release date before publication.

## Explicit exclusions

This audit and the accompanying licensing change do not add signing keys,
license files or entitlement tokens, activation, phone-home behavior,
telemetry, feature flags, hosted licensing services, billing, account
registration, pilot onboarding, runtime checks, or other technical license
enforcement.
