# Tandem Licensing

## Machine-readable summary

**Repository license class:** Open Core / Mixed License  
**Open-source components:** MIT, Apache-2.0, or MIT OR Apache-2.0  
**Source-available components:** Business Source License 1.1 (`BUSL-1.1`)  
**Default rule:** Each package is governed by its package-local manifest and/or package-local license file.  
**Canonical license map:** This file.

Tandem is an open-core project. Most SDK, runtime, client, local execution,
and support components are available under permissive open-source licenses.
Selected governance and plan-compilation components are source-available under
`BUSL-1.1`.

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
| `tandem-agent-teams`          | `crates/tandem-agent-teams/Cargo.toml`         | `MIT OR Apache-2.0` |
| `tandem-browser`              | `crates/tandem-browser/Cargo.toml`             | `MIT OR Apache-2.0` |
| `tandem-channels`             | `crates/tandem-channels/Cargo.toml`            | `MIT OR Apache-2.0` |
| `tandem-core`                 | `crates/tandem-core/Cargo.toml`                | `MIT OR Apache-2.0` |
| `tandem-document`             | `crates/tandem-document/Cargo.toml`            | `MIT OR Apache-2.0` |
| `tandem-enterprise-contract`  | `crates/tandem-enterprise-contract/Cargo.toml` | `MIT OR Apache-2.0` |
| `tandem-memory`               | `crates/tandem-memory/Cargo.toml`              | `MIT OR Apache-2.0` |
| `tandem-orchestrator`         | `crates/tandem-orchestrator/Cargo.toml`        | `MIT OR Apache-2.0` |
| `tandem-wire`                 | `crates/tandem-wire/Cargo.toml`                | `MIT OR Apache-2.0` |
| `tandem-server`               | `crates/tandem-server/Cargo.toml`              | `MIT OR Apache-2.0` |
| `tandem-providers`            | `crates/tandem-providers/Cargo.toml`           | `MIT OR Apache-2.0` |
| `tandem-skills`               | `crates/tandem-skills/Cargo.toml`              | `MIT OR Apache-2.0` |
| `tandem-types`                | `crates/tandem-types/Cargo.toml`               | `MIT OR Apache-2.0` |
| `tandem-observability`        | `crates/tandem-observability/Cargo.toml`       | `MIT OR Apache-2.0` |
| `tandem-runtime`              | `crates/tandem-runtime/Cargo.toml`             | `MIT OR Apache-2.0` |
| `tandem-tools`                | `crates/tandem-tools/Cargo.toml`               | `MIT OR Apache-2.0` |
| `tandem-tui`                  | `crates/tandem-tui/Cargo.toml`                 | `MIT OR Apache-2.0` |
| `tandem-workflows`            | `crates/tandem-workflows/Cargo.toml`           | `MIT OR Apache-2.0` |

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
| `@frumu/tandem-tui`    | `packages/tandem-tui/package.json`                   | `MIT OR Apache-2.0` |

## Source-Available / BUSL-1.1 Components

| Package                    | Path                                         | License    |
| -------------------------- | -------------------------------------------- | ---------- |
| `tandem-plan-compiler`     | `crates/tandem-plan-compiler/Cargo.toml`     | `BUSL-1.1` |
| `tandem-governance-engine` | `crates/tandem-governance-engine/Cargo.toml` | `BUSL-1.1` |

## Open-core boundary

The following components are source-available and are not OSI-approved open source:

- `crates/tandem-plan-compiler`
- `crates/tandem-governance-engine`

All other packages listed above are intended to be used under their stated
permissive open-source licenses unless a package-local manifest or license file
states otherwise.

The BUSL-1.1 components protect Tandem's commercial governance layer while
leaving the core runtime, SDKs, clients, and local development surface auditable
and usable under permissive terms.

These components are source-available, not open source. Their package-local `LICENSE` files define the additional use grant, hosted-service restriction, change date, and change license.

Current source-available license files:

- `crates/tandem-plan-compiler/LICENSE`
- `crates/tandem-governance-engine/LICENSE`

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
converts to the Change License, `GPL-2.0-or-later OR MIT OR Apache-2.0`). When
cutting a release, stamp the `Change Date` in every `BUSL-1.1` `LICENSE` file to
release-date + 4 years.

BUSL applies separately to each version, so a license change is prospective: the
grant and Change Date above first take effect in **0.6.8** (the next release).
`0.6.7` and earlier remain under the terms they were released with. The
`Change Date` currently in the `LICENSE` files (`2030-07-06`) is a placeholder
for the 0.6.8 line and is finalized to the actual release date + 4 years when
0.6.8 is cut.

## License Texts

- Repository mixed-license notice: `LICENSE`
- MIT text: `LICENSE-MIT`
- Apache 2.0 text: `LICENSE-APACHE`
- Business Source License 1.1 terms: package-local `LICENSE` files in each `BUSL-1.1` component

## CI Enforcement

Rust dependency license policy is enforced by `cargo deny` using
`.config/deny.toml`. License and advisory exceptions follow the process in
[`CI_SECURITY_AND_COVERAGE.md`](CI_SECURITY_AND_COVERAGE.md).

## NOTICE Guidance (Apache-2.0 users)

Apache-2.0 does not require a `NOTICE` file unless one is distributed with the work. If downstream redistributors add Apache attribution notices, they should preserve any applicable notices consistent with Apache-2.0 Section 4.

## Tandem TUI Adaptation Notes

`tandem-tui` includes tandem-local implementations adapted from design/code patterns in `codex` (Apache-2.0), including composer/editor behavior and markdown rendering strategy.

Primary adapted source references:

- `codex/codex-rs/tui/src/public_widgets/composer_input.rs`
- `codex/codex-rs/tui/src/bottom_pane/textarea.rs`
- `codex/codex-rs/tui/src/markdown_render.rs`

These adaptations are rewrites for Tandem architecture and are not line-for-line copies.
