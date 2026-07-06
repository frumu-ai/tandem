# @frumu/tandem-enterprise

Hosted Tandem engine binary distribution for Linux x64, with local embeddings.

This package installs a `tandem-engine` command backed by the `tandem-engine-enterprise-linux-x64.tar.gz` GitHub release asset. Since TAN-632, the standard `tandem-engine` release already includes enterprise routes and premium governance on every platform; this asset differs only by additionally bundling the local-embedding stack (fastembed/ort) for hosted `tandem-agents` deployments.

## Licensing

This npm package (the installer/launcher scripts) is `MIT OR Apache-2.0`. The
engine binary it downloads and runs includes source-available components
licensed under the Business Source License 1.1 (`tandem-enterprise-server`,
`tandem-governance-engine`, `tandem-plan-compiler`,
`tandem-incident-monitor`) — as does every Tandem engine binary. Internal
production use is free; offering it to third parties as a
hosted/SaaS/white-label/embedded commercial service requires a commercial
license. See `docs/LICENSING.md` in the
[tandem repository](https://github.com/frumu-ai/tandem) for the full terms.
