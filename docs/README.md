# Tandem Documentation

This folder contains deep technical and process references.

For end-user onboarding journeys (install, first run, desktop/CLI paths), use:

- `tandem/guide/src/content/docs/`

## User Guides

- [Tandem TUI Guide](./TANDEM_TUI_GUIDE.md) - Deep/legacy TUI reference.
- [Ollama Guide](./OLLAMA_GUIDE.md) - Provider-specific setup notes.
- [Ralph Mode](./RALPH_MODE.md) - Specialized mode behavior.

## Technical Documentation

- [Design System](./DESIGN_SYSTEM.md) - Detailed style/system notes.
- [Engine Protocol Matrix](./ENGINE_PROTOCOL_MATRIX.md) - Wire contracts and status.
- [Engine Context-Driving Runtime](./ENGINE_CONTEXT_DRIVING_RUNTIME.md) - Engine source-of-truth runtime for long-running context, replay, and guardrails.
- [MCP Improvements](./MCP_IMPROVEMENTS.md) - Connector tools, MCP discovery, and allowlist design.
- [GitHub Projects via MCP](./MCP_IMPROVEMENTS.md#implementation-note-github-projects-via-mcp) - Tandem auto-registers the official GitHub MCP server when a PAT is available, so GitHub Projects work without manual `mcp add`.
- [Memory Tiers and Governance](./design/MEMORY_TIERS.md) - Global memory model, safety boundaries, and API/event contracts.
- [Workflow Automation Runtime](./WORKFLOW_RUNTIME.md) - How Tandem's workflow runtime produces verifiable, trustworthy artifacts across multi-stage AI pipelines.
- [Workflow Bug Replay Guide](./WORKFLOW_BUG_REPLAY.md) - How to turn live workflow failures into deterministic replay regressions and release gates.

## SDK & Development

- [Tandem SDK Vision](./TANDEM_SDK_VISION.md)
- [Tandem CLI Vision](./TANDEM_CLI_VISION.md)
- [Engine CLI Guide](./ENGINE_CLI.md)
- [Engine Testing](./ENGINE_TESTING.md)

## Release Notes

- Canonical: [Release Notes](../RELEASE_NOTES.md)
- Compatibility pointer: `./RELEASE_NOTES.md`
