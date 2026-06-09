# Tandem Documentation

This folder contains public technical references.

For end-user onboarding journeys (install, first run, desktop/CLI paths), use:

- `../guide/src/content/docs/`

## User Guides

- [Ollama Guide](./OLLAMA_GUIDE.md) - Provider-specific setup notes.

## Technical Documentation

- [Design System](./DESIGN_SYSTEM.md) - Detailed style/system notes.
- [EU AI Act Readiness](./EU_AI_ACT_COMPLIANCE.md) - CISO-facing control mapping, current Tandem coverage, and roadmap gaps.
- [Compliance Starter Pack](./compliance/README.md) - Public EU AI Act starter docs for deployers, CISOs, and compliance teams.
- [Cross-Tenant Grants Design](./CROSS_TENANT_GRANTS_DESIGN.md) - Signed grant envelope, inbound lookup, trust root, and enforcement design for governed tenant-to-tenant sharing.
- [Engine Protocol Matrix](./ENGINE_PROTOCOL_MATRIX.md) - Wire contracts and status.
- [AI Runtime Infrastructure](./AI_RUNTIME_INFRASTRUCTURE.md) - Engine source-of-truth runtime for long-running context, replay, and guardrails.
- [MCP Improvements](./MCP_IMPROVEMENTS.md) - Connector tools, MCP discovery, and allowlist design.
- [GitHub Projects via MCP](./MCP_IMPROVEMENTS.md#implementation-note-github-projects-via-mcp) - Tandem auto-registers the official GitHub MCP server when a PAT is available, so GitHub Projects work without manual `mcp add`.
- [Workflow Automation Runtime](./WORKFLOW_RUNTIME.md) - How Tandem's workflow runtime produces verifiable, trustworthy artifacts across multi-stage AI pipelines.
- [Workflow Bug Replay Guide](./WORKFLOW_BUG_REPLAY.md) - How to turn live workflow failures into deterministic replay regressions and release gates.
- [Workflow Generated Variation Coverage](./WORKFLOW_GENERATED_VARIATIONS.md) - Constrained generator design and nightly workflow-variation coverage.
- [Channel Lifecycle and Diagnostics](./CHANNELS_LIFECYCLE_AND_DIAGNOSTICS.md) - Registry-based channel runtime lifecycle, endpoint behavior, and required config keys.

## SDK & Development

- [Engine CLI Guide](./ENGINE_CLI.md)
- [Engine Testing](./ENGINE_TESTING.md)

For archived planning notes, internal design docs, and working reports, see [docs/internal/README.md](./internal/README.md).

## Release Notes

- Canonical: [Release Notes](../RELEASE_NOTES.md)
- Compatibility pointer: `./RELEASE_NOTES.md`
