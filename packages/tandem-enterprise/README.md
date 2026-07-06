# @frumu/tandem-enterprise

Hosted enterprise Tandem engine binary distribution for Linux x64.

This package installs a `tandem-engine` command backed by the `tandem-engine-enterprise-linux-x64.tar.gz` GitHub release asset. It is intended for hosted `tandem-agents` deployments that need enterprise routes compiled into the engine.

## Licensing

This npm package (the installer/launcher scripts) is `MIT OR Apache-2.0`. The
engine binary it downloads and runs includes source-available components
licensed under the Business Source License 1.1 (`tandem-enterprise-server`,
`tandem-governance-engine`, `tandem-plan-compiler`) — internal production use
is free; offering it to third parties as a hosted/SaaS/white-label/embedded
commercial service requires a commercial license. See `docs/LICENSING.md` in
the [tandem repository](https://github.com/frumu-ai/tandem) for the full terms.
