# Tandem Licensing

This repository uses a mixed licensing strategy.

## Rust SDK and Runtime Packages

The Rust SDK/runtime surface is dual-licensed under:

- `MIT`
- `Apache-2.0`

Consumers may choose either license (`MIT OR Apache-2.0`) for the packages below.

| Package                | Path                                     | License             |
| ---------------------- | ---------------------------------------- | ------------------- |
| `tandem-engine`        | `engine/Cargo.toml`                      | `MIT OR Apache-2.0` |
| `tandem-core`          | `crates/tandem-core/Cargo.toml`          | `MIT OR Apache-2.0` |
| `tandem-wire`          | `crates/tandem-wire/Cargo.toml`          | `MIT OR Apache-2.0` |
| `tandem-server`        | `crates/tandem-server/Cargo.toml`        | `MIT OR Apache-2.0` |
| `tandem-providers`     | `crates/tandem-providers/Cargo.toml`     | `MIT OR Apache-2.0` |
| `tandem-types`         | `crates/tandem-types/Cargo.toml`         | `MIT OR Apache-2.0` |
| `tandem-observability` | `crates/tandem-observability/Cargo.toml` | `MIT OR Apache-2.0` |
| `tandem-runtime`       | `crates/tandem-runtime/Cargo.toml`       | `MIT OR Apache-2.0` |
| `tandem-tools`         | `crates/tandem-tools/Cargo.toml`         | `MIT OR Apache-2.0` |
| `tandem-tui`           | `crates/tandem-tui/Cargo.toml`           | `MIT OR Apache-2.0` |

## App/Desktop/Web Scope

Desktop/web app licensing is unchanged in this pass. This document only changes and clarifies the Rust SDK/runtime package licensing listed above.

## License Texts

- MIT text: `LICENSE`
- Apache 2.0 text: `LICENSE-APACHE`

## NOTICE Guidance (Apache-2.0 users)

Apache-2.0 does not require a `NOTICE` file unless one is distributed with the work. If downstream redistributors add Apache attribution notices, they should preserve any applicable notices consistent with Apache-2.0 Section 4.
