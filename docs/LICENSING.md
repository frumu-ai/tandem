# Tandem Licensing

This repository uses a mixed licensing strategy. This document is the canonical package-by-package map for Tandem's public repository.

In plain language:

- Open-source components use permissive MIT, Apache-2.0, or MIT OR Apache-2.0 terms.
- Source-available components use the Business Source License 1.1 (`BUSL-1.1`) and may require a commercial license for some production or hosted-service uses.
- The root [LICENSE](../LICENSE) file is a repository-level notice, not a blanket license for every file.

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

## Business Source Licensed Components

| Package                    | Path                                         | License    |
| -------------------------- | -------------------------------------------- | ---------- |
| `tandem-plan-compiler`     | `crates/tandem-plan-compiler/Cargo.toml`     | `BUSL-1.1` |
| `tandem-governance-engine` | `crates/tandem-governance-engine/Cargo.toml` | `BUSL-1.1` |

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

## NOTICE Guidance (Apache-2.0 users)

Apache-2.0 does not require a `NOTICE` file unless one is distributed with the work. If downstream redistributors add Apache attribution notices, they should preserve any applicable notices consistent with Apache-2.0 Section 4.

## Tandem TUI Adaptation Notes

`tandem-tui` includes tandem-local implementations adapted from design/code patterns in `codex` (Apache-2.0), including composer/editor behavior and markdown rendering strategy.

Primary adapted source references:

- `codex/codex-rs/tui/src/public_widgets/composer_input.rs`
- `codex/codex-rs/tui/src/bottom_pane/textarea.rs`
- `codex/codex-rs/tui/src/markdown_render.rs`

These adaptations are rewrites for Tandem architecture and are not line-for-line copies.
