---
name: cowork-mcp-config-assistant
description: Configure Model Context Protocol (MCP) servers and customize Tandem skills for your organization.
requires: [python, node]
---

# MCP Configuration & Skill Customization

Adapt Tandem skills to your specific organization by replacing customization points with actual tool names, configuring MCP servers, and applying organization-specific customizations.

## Overview

Some skills use placeholders (e.g., `~~Jira`, `~~your-team-channel`) that need to be replaced with your actual tools and values. This assistant helps you identify these points and configure the necessary MCP servers to connect Tandem to your data.

## Workflow

### 1. Identify Customization Needs

- **Gather Context**: What tools does your organization use? (Jira, Asana, GitHub, GitLab, etc.)
- **Scan Skills**: Look for placeholders in skill templates that need replacement.

### 2. Configure MCP Servers

To connect Tandem to your tools, you need to configure MCP servers.

**Common MCP Servers:**

- **Filesystem**: Access local directories.
- **PostgreSQL / SQLite**: Query databases directly.
- **GitHub / GitLab**: Access repositories and issues.
- **Slack / Discord**: Access communication channels.
- **Google Drive / Notion**: Access documentation and notes.

### 3. Apply Customizations

- **Replace Placeholders**: Update `SKILL.md` files to use your specific tool names.
- **Update URL Patterns**: Change example URLs to match your organization's instances (e.g., `jira.company.com`).
- **Set Context**: Add organization-specific values like Workspace IDs or Project names.

## How to Configure MCP

1. **Find the Server**: Check the [MCP Registry](https://github.com/modelcontextprotocol/servers) for available servers.
2. **Install**: Follow the installation instructions for the server (usually `npm install` or `pip install`).
3. **Configure**: Add the server to your Tandem configuration file (usually in settings or a config JSON).

Example Config Entry:

```json
"github": {
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-github"],
  "env": {
    "GITHUB_PERSONAL_ACCESS_TOKEN": "your-token-here"
  }
}
```

## Tips

- **Start Simple**: Connect one key tool (like your issue tracker or knowledge base) first.
- **Verify**: Test the connection by asking Tandem to list items from the tool.
- **Iterate**: Add more tools and customizations as you discover needs.
