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
- Source-available components use `BUSL-1.1` and may require a commercial license for some production, hosted-service, or competitive SaaS uses.
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
| `tandem-enterprise-server`    | `crates/tandem-enterprise-server/Cargo.toml`   | `MIT OR Apache-2.0` |
| `tandem-eval`                 | `crates/tandem-eval/Cargo.toml`                | `MIT OR Apache-2.0` |
| `tandem-graph-core`           | `crates/tandem-graph-core/Cargo.toml`          | `MIT OR Apache-2.0` |
| `tandem-incident-monitor`     | `crates/tandem-incident-monitor/Cargo.toml`    | `MIT OR Apache-2.0` |
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
