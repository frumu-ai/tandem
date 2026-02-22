---
title: WebMCP for Agents
---

[WebMCP](https://webmcp.dev/) is a browser-first MCP environment that is useful for testing and teaching agent tool workflows.

For Tandem teams, it is a fast way to:

- prototype MCP server behavior before production wiring
- validate tool contracts and payload shape
- demonstrate agent-safe tool patterns to contributors

## Why It Helps Tandem Engine Users

When you are building agent flows around `tandem-engine`, WebMCP can shorten iteration loops:

- verify MCP tool schemas and outputs quickly
- catch naming and argument mismatches before routines/run plans depend on them
- onboard new teammates by showing live tool-call behavior

## Suggested Tandem Workflow

1. Build and validate MCP tools in WebMCP.
2. Register/connect the same MCP server in Tandem (`/mcp`, `/mcp/{name}/connect`).
3. Confirm namespaced tool IDs via `/mcp/tools` and `/tool/ids`.
4. Restrict production automation with explicit `allowed_tools` on automations (`routines/*` is still supported).

## Good Practices

- Keep tool names stable once routines depend on them.
- Prefer narrow, explicit `allowed_tools` for autonomous runs.
- Treat WebMCP as a rapid validation environment, then enforce policy in Tandem.

## See Also

- [MCP Automated Agents](./mcp-automated-agents/)
- [Headless Service](./headless-service/)
- [Tools Reference](./reference/tools/)
- [WebMCP](https://webmcp.dev/)
