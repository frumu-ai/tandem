# MCP Catalog (Generated)

- Sources:
  - Anthropic MCP registry (https://api.anthropic.com/mcp-registry/v0/servers)
  - Curated additions (curated-mcp-overrides)
- Generated at: 2026-03-02T23:03:19.833Z
- Version: latest
- Visibility: commercial
- Servers: 162

Regenerate:

`node scripts/generate-mcp-catalog.mjs`

## Tool Security Overrides

Operators can mount JSON or YAML security overrides with
`TANDEM_MCP_TOOL_SECURITY_OVERRIDES_PATH`. The file is applied at runtime before
catalog security metadata is normalized.

```yaml
schema_version: 1
servers:
  slack:
    security:
      required_permissions: [read]
      resource_kinds: [document_collection]
      data_classes: [confidential, customer_data]
    tools:
      slack_send_message:
        required_permissions: [admin, execute]
        resource_kinds: [mcp_tool]
        data_classes: [confidential]
        admin_surface: true
        external_side_effect: true
        default_visibility: hidden
tools:
  mcp.notion.search:
    required_permissions: [read]
    resource_kinds: [knowledge_space]
    data_classes: [internal]
```
