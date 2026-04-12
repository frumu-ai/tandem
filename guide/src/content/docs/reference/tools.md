---
title: Tools Reference
---

The Tandem Engine tool registry currently exposes the following tools.

## File Operations

- **`read`**: Read file contents.
  - Input: `path` (string)
- **`write`**: Write file contents (overwrites).
  - Input: `path` (string), `content` (string)
- **`edit`**: String replacement in a file.
  - Input: `path` (string), `old` (string), `new` (string)
- **`glob`**: Find files by pattern.
  - Input: `pattern` (string, e.g., `src/**/*.rs`)

For coding tasks, agents usually read first, use `edit` for narrow replacements, `write` for new or rewritten files, and `apply_patch` when the change is easier to review as a unified diff.

## Search

- **`grep`**: Regex search in files.
  - Input: `pattern` (string), `path` (string, root directory)
- **`websearch`**: Search the web (powered by Exa.ai).
  - Input: `query` (string), `limit` (integer)
- **`codesearch`**: Semantic code search (if configured).
- **`memory_list`**: List stored memory chunks from the current session, project, or global scope.
  - Input: optional scope controls such as `tier`, `session_id`, `project_id`, `limit`, `allow_global`
- **`memory_search`**: Search memory by query across the current session, project, or global scope.
  - Input: `query` plus optional scope controls such as `tier`, `session_id`, `project_id`, `limit`, `allow_global`
- **`memory_store`**: Persist content into session, project, or global memory.
  - Input: `content` plus optional scope and metadata such as `tier`, `session_id`, `project_id`, `source`, `metadata`, `allow_global`
- **`memory_delete`**: Delete a stored memory chunk within the current allowed scope.
  - Input: `chunk_id` (or `id`) plus optional scope controls such as `tier`, `session_id`, `project_id`, `allow_global`

## Web

- **`webfetch`**: Fetch URL and return structured Markdown/JSON output.
  - Input: `url` (string), optional `mode`, `return`, `max_bytes`, `timeout_ms`, `max_redirects`
- **`webfetch_html`**: Fetch URL and return raw HTML text.
  - Input: `url` (string), optional `max_bytes`, `timeout_ms`, `max_redirects`

## System

- **`bash`**: Run shell commands (PowerShell on Windows, Bash on Linux/Mac).
  - Input: `command` (string)
- **`mcp_list`**: List the configured and connected MCP servers and their discovered tools.
  - Input: none
  - Use this first when an agent needs to discover MCP access.
  - It returns a structured inventory in one result, which keeps the agent from carrying a full MCP tool dump in context unless it actually needs one.
  - If the needed server or tool is missing, stop and tell the user to add or connect the MCP instead of inventing a capability.
  - If a workflow request depends on an unavailable MCP, the right response is to ask for that MCP, not to silently switch to a different tool.
- **`mcp_debug`**: Call an MCP tool directly by URL.
  - Input: `url` (string), `tool` (string), optional `args` (object), `headers` (object), `timeout_ms` (integer), `max_bytes` (integer)
  - Use this when you already know the MCP server URL and the tool name you want to invoke.
  - This tool does not discover tools or search the registry; it only calls a named MCP tool.
- **`todo_write`**: Update the Todo/task list.
  - Aliases: `todowrite`, `update_todo_list`
- **`task`**: Update the current task status.
- **`question`**: Ask a structured question to the user and wait for input.
- **`spawn_agent`**: Spawn an agent-team worker instance (runtime/policy gated).
  - Input: mission/spawn payload (e.g., `missionID`, `role`, `templateID`, `source`)
- **`teamcreate`**: Create/register an agent-team context for coordinated teammate tasks.
  - Input: team metadata (e.g., `team_name`, `description`, `agent_type`)
- **`taskcreate`**: Create teammate task records in a team context.
  - Input: task payload (e.g., `team_name`, `name`, `description`)
- **`taskupdate`**: Update teammate task status/notes/progress in a team context.
  - Input: task update payload (e.g., `team_name`, `task_id`, `status`, `notes`)
- **`tasklist`**: List tasks for a team context.
  - Input: optional filters (e.g., `team_name`, status filter)
- **`sendmessage`**: Send mailbox-style message/task prompt to one or more teammates.
  - Input: message payload (e.g., `team_name`, `to`, `content`, `summary`)

## MCP Discovery Endpoints

These are HTTP endpoints, not engine tools:

- `mcp_list` - Engine tool that returns the current MCP inventory snapshot.
- `GET /mcp/tools` - List discovered tools from connected MCP servers.
- `GET /tool/ids` - List all engine tool IDs, including built-ins and MCP tools.

If you need to find a tool by keyword, fetch one of those lists and filter locally. Tandem does not provide a separate public "search the installed registry" endpoint, but `mcp_list` does give the agent a structured inventory of connected servers and tools, which is the lower-context first step before selecting a specific MCP tool.

## Specialized

- **`skill`**: Execute a skill.
- **`apply_patch`**: Apply a unified diff patch.
- **`batch`**: Execute multiple tools in a batch.
- **`lsp`**: Interact with the Language Server Protocol.
