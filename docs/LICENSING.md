# Tandem Licensing

## Machine-readable summary

**Repository license class:** Open Core / Mixed License  
**Open-source components:** MIT, Apache-2.0, or MIT OR Apache-2.0  
**Source-available components:** Business Source License 1.1 (`BUSL-1.1`)  
**Default rule:** Each package is governed by its package-local manifest and/or package-local license file.  
**Canonical license map:** This file.

Tandem is an open-core project. Most SDK, runtime, client, local execution,
and support components are available under permissive open-source licenses.
Selected governance, plan-compilation, incident-monitoring, and enterprise
components are source-available under `BUSL-1.1`.

This repository is therefore **not governed by one blanket root license**.
The root [`LICENSE`](../LICENSE) file is a repository-level notice only. For
exact terms, use the package-by-package table below.

## Plain-language summary

- Permissive open-source components use `MIT`, `Apache-2.0`, or `MIT OR Apache-2.0`.
- Source-available components use `BUSL-1.1`. You may use, modify, and self-host them in production for your organization's own internal use for free, regardless of revenue. A commercial license is required only to offer them (or a substantially similar product) to third parties as a managed, hosted, SaaS, white-label, embedded, or other commercial offering.
- If this document, the root `LICENSE`, and a package-local manifest or license file disagree, the package-local manifest and package-local license file control for that package.

## Rust SDK and Runtime Packages

The Rust SDK/runtime surface is dual-licensed under:

- `MIT`
- `Apache-2.0`

Consumers may choose either license (`MIT OR Apache-2.0`) for the packages below, unless a package-local manifest or license file states otherwise.

| Package                       | Path                                           | License             |
| ----------------------------- | ---------------------------------------------- | ------------------- |
| `tandem-ai` / `tandem-engine` | `engine/Cargo.toml`                            | `MIT OR Apache-2.0` |
| `tandem` (desktop)            | `apps/tandem-desktop/src-tauri/Cargo.toml`     | `MIT OR Apache-2.0` |
| `tandem-agent-teams`          | `crates/tandem-agent-teams/Cargo.toml`         | `MIT OR Apache-2.0` |
| `tandem-automation`           | `crates/tandem-automation/Cargo.toml`          | `MIT OR Apache-2.0` |
| `tandem-browser`              | `crates/tandem-browser/Cargo.toml`             | `MIT OR Apache-2.0` |
| `tandem-channels`             | `crates/tandem-channels/Cargo.toml`            | `MIT OR Apache-2.0` |
| `tandem-core`                 | `crates/tandem-core/Cargo.toml`                | `MIT OR Apache-2.0` |
| `tandem-data-boundary`        | `crates/tandem-data-boundary/Cargo.toml`       | `MIT OR Apache-2.0` |
| `tandem-document`             | `crates/tandem-document/Cargo.toml`            | `MIT OR Apache-2.0` |
| `tandem-enterprise-contract`  | `crates/tandem-enterprise-contract/Cargo.toml` | `MIT OR Apache-2.0` |
| `tandem-eval`                 | `crates/tandem-eval/Cargo.toml`                | `MIT OR Apache-2.0` |
| `tandem-graph-core`           | `crates/tandem-graph-core/Cargo.toml`          | `MIT OR Apache-2.0` |
| `tandem-memory`               | `crates/tandem-memory/Cargo.toml`              | `MIT OR Apache-2.0` |
| `tandem-meta-harness-eval`    | `crates/tandem-meta-harness-eval/Cargo.toml`   | `MIT OR Apache-2.0` |
| `tandem-orchestrator`         | `crates/tandem-orchestrator/Cargo.toml`        | `MIT OR Apache-2.0` |
| `tandem-wire`                 | `crates/tandem-wire/Cargo.toml`                | `MIT OR Apache-2.0` |
| `tandem-repo-intelligence`    | `crates/tandem-repo-intelligence/Cargo.toml`   | `MIT OR Apache-2.0` |
| `tandem-server`               | `crates/tandem-server/Cargo.toml`              | `MIT OR Apache-2.0` |
| `tandem-providers`            | `crates/tandem-providers/Cargo.toml`           | `MIT OR Apache-2.0` |
| `tandem-skills`               | `crates/tandem-skills/Cargo.toml`              | `MIT OR Apache-2.0` |
| `tandem-types`                | `crates/tandem-types/Cargo.toml`               | `MIT OR Apache-2.0` |
| `tandem-observability`        | `crates/tandem-observability/Cargo.toml`       | `MIT OR Apache-2.0` |
| `tandem-runtime`              | `crates/tandem-runtime/Cargo.toml`             | `MIT OR Apache-2.0` |
| `tandem-tools`                | `crates/tandem-tools/Cargo.toml`               | `MIT OR Apache-2.0` |
| `tandem-tui`                  | `crates/tandem-tui/Cargo.toml`                 | `MIT OR Apache-2.0` |
| `tandem-workflows`            | `crates/tandem-workflows/Cargo.toml`           | `MIT OR Apache-2.0` |

`tandem-meta-harness-eval` is an internal crate (`publish = false`) and is not
distributed on crates.io; it is listed for completeness.

## JavaScript and Python Packages

| Package                | Path                                                 | License             |
| ---------------------- | ---------------------------------------------------- | ------------------- |
| `tandem-ai`            | `packages/tandem-ai/package.json`                    | `MIT`               |
| `@frumu/tandem-client` | `packages/tandem-client-ts/package.json`             | `MIT`               |
| `tandem-client`        | `packages/tandem-client-py/pyproject.toml`           | `MIT`               |
| `create-tandem-panel`  | `packages/create-tandem-panel/package.json`          | `MIT OR Apache-2.0` |
| Tandem panel scaffold  | `packages/create-tandem-panel/template/package.json` | `MIT OR Apache-2.0` |
| `@frumu/tandem-panel`  | `packages/tandem-control-panel/package.json`         | `MIT OR Apache-2.0` |
| `@frumu/tandem`        | `packages/tandem-engine/package.json`                | `MIT OR Apache-2.0` |
| `@frumu/tandem-enterprise` | `packages/tandem-enterprise/package.json`        | `MIT OR Apache-2.0` |
| `@frumu/tandem-tui`    | `packages/tandem-tui/package.json`                   | `MIT OR Apache-2.0` |

The `@frumu/tandem-desktop` app package (`apps/tandem-desktop/package.json`) is
`private` and not published to npm, so it is not listed here.

## Source-Available / BUSL-1.1 Components

| Package                    | Path                                         | License    |
| -------------------------- | -------------------------------------------- | ---------- |
| `tandem-plan-compiler`     | `crates/tandem-plan-compiler/Cargo.toml`     | `BUSL-1.1` |
| `tandem-governance-engine` | `crates/tandem-governance-engine/Cargo.toml` | `BUSL-1.1` |
| `tandem-enterprise-server` | `crates/tandem-enterprise-server/Cargo.toml` | `BUSL-1.1` |
| `tandem-incident-monitor`  | `crates/tandem-incident-monitor/Cargo.toml`  | `BUSL-1.1` |

## Open-core boundary

The following components are source-available and are not OSI-approved open source:

- `crates/tandem-plan-compiler`
- `crates/tandem-governance-engine`
- `crates/tandem-enterprise-server`
- `crates/tandem-incident-monitor`

All other packages listed above are intended to be used under their stated
permissive open-source licenses unless a package-local manifest or license file
states otherwise.

The BUSL-1.1 components protect Tandem's commercial governance layer. The
permissive licenses keep the SDKs, clients, schemas, and the rest of the
engine source auditable and reusable under open-source terms.

### Engine binaries are mixed-license

Every distributed Tandem engine binary includes **all four** `BUSL-1.1`
components — there is no BUSL-free or governance-free engine binary. All
release artifacts (`tandem-engine` on every platform, the desktop sidecar,
`tandem-tui`, `tandem-browser`) are built with `tandem-ai/enterprise`:
enterprise routes, premium governance, and Google Drive connectors compiled
in. The hosted Linux asset (`tandem-engine-enterprise-*`, built with
`tandem-ai/enterprise-full`) differs only by adding the local-embedding
stack (fastembed/ort) — a build-weight difference, not a licensing one.

Several permissive crates depend on source-available crates directly:

- `tandem-server` (MIT) depends on `tandem-plan-compiler` and
  `tandem-incident-monitor` (both `BUSL-1.1`)
- `tandem-automation` (MIT) depends on `tandem-plan-compiler`
- `tandem-eval` (MIT) depends on `tandem-incident-monitor`

What this means in practice:

- **Running a distributed binary** is governed by the BUSL Additional Use
  Grant for the source-available components it contains: internal production
  use is free for any organization; offering the binary (or a substantially
  similar product) to third parties as a managed, hosted, SaaS, white-label,
  or embedded commercial service requires a commercial license from Frumu LTD.
- **Using an individual permissive crate as a library** stays under that
  crate's MIT/Apache terms. If the crate depends on a `BUSL-1.1` crate, Cargo
  pulls that dependency with its own license, and the BUSL terms apply to that
  part of your build. Each crate's manifest declares its license accurately;
  nothing is relicensed by inclusion.

This is a deliberate choice: rather than maintaining feature-stripped
BUSL-free binaries, the source-available components ship in every engine
artifact, and this section plus the per-crate license map is the disclosure
that makes the boundary clear.

These components are source-available, not open source. Their package-local `LICENSE` files define the additional use grant, hosted-service restriction, change date, and change license.

Current source-available license files:

- `crates/tandem-plan-compiler/LICENSE`
- `crates/tandem-governance-engine/LICENSE`
- `crates/tandem-enterprise-server/LICENSE`
- `crates/tandem-incident-monitor/LICENSE`

The source-available governance layer authorizes recursive and Self-Operator behavior such as agent-authored automation creation, approval-bound capability requests, lineage enforcement, and spend/review guardrails.

### Additional Use Grant (what is free vs. licensed)

The `BUSL-1.1` components may be used, modified, and self-hosted in production
for an organization's own **internal use** at no cost, regardless of revenue.
"Internal use" covers the organization, its affiliates, employees, contractors,
and customers, but only where the Licensed Work is operated as part of that
organization's own internal software development, agent governance, incident
response, or automation workflows — not as the product being sold.

A **commercial license** is required to provide the Licensed Work, or any
product or service whose primary value is substantially similar to it, to third
parties as a managed, hosted, software-as-a-service, white-label, embedded, or
other commercial offering.

### Change Date policy

Each released version's `Change Date` is set to **four years after that
version's release date** (a rolling window; on the Change Date the version
converts to the Change License, `GPL-2.0-or-later OR MIT OR Apache-2.0`).
`scripts/bump-version.sh` (and the PowerShell twin) stamps `Change Date` to
run-date + 4 years in every `BUSL-1.1` `LICENSE` file automatically as part of
the release version bump; the LICENSE files it discovers are any
`crates/*/LICENSE` containing the BUSL-1.1 text, so newly relicensed crates
are covered without script changes.

BUSL applies separately to each version, so a license change is prospective:
the grant and Change Date above first took effect in **0.6.8** (released
2026-07-09). `0.6.7` and earlier remain under the terms they were released
with. The `Change Date` in the shipped 0.6.8 `LICENSE` files is `2030-07-09` —
the 0.6.8 release date plus four years, per the rolling-window policy above.

## License Texts

- Repository mixed-license notice: `LICENSE`
- MIT text: `LICENSE-MIT`
- Apache 2.0 text: `LICENSE-APACHE`
- Business Source License 1.1 terms: package-local `LICENSE` files in each `BUSL-1.1` component

## CI Enforcement

Rust dependency license policy is enforced by `cargo deny` using
`.config/deny.toml`. License and advisory exceptions follow the process in
[`CI_SECURITY_AND_COVERAGE.md`](CI_SECURITY_AND_COVERAGE.md).

The package-by-package tables above are enforced by
`scripts/verify-license-map.mjs`, which runs in the "Validate Docs" CI job. It
fails the build if any workspace package (Rust workspace member, published
`packages/*` npm package, or `pyproject.toml`) is missing from this file,
carries a license that disagrees with its manifest, or is mapped to a path that
no longer exists. When you add a package or change a `license` field, update the
matching table here in the same change.

## NOTICE Guidance (Apache-2.0 users)

Apache-2.0 does not require a `NOTICE` file unless one is distributed with the work. If downstream redistributors add Apache attribution notices, they should preserve any applicable notices consistent with Apache-2.0 Section 4.

## Tandem TUI Adaptation Notes

`tandem-tui` includes tandem-local implementations adapted from design/code patterns in `codex` (Apache-2.0), including composer/editor behavior and markdown rendering strategy.

Primary adapted source references:

- `codex/codex-rs/tui/src/public_widgets/composer_input.rs`
- `codex/codex-rs/tui/src/bottom_pane/textarea.rs`
- `codex/codex-rs/tui/src/markdown_render.rs`

These adaptations are rewrites for Tandem architecture and are not line-for-line copies.
