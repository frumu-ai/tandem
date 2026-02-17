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

## Search

- **`grep`**: Regex search in files.
  - Input: `pattern` (string), `path` (string, root directory)
- **`websearch`**: Search the web (powered by Exa.ai).
  - Input: `query` (string), `limit` (integer)
- **`codesearch`**: Semantic code search (if configured).
- **`memory_search`**: Search persisted memory by query and scope.
  - Input: `query` plus one or more scopes (e.g., session/workspace).

## Web

- **`webfetch`**: Fetch raw URL text.
  - Input: `url` (string)
- **`webfetch_document`**: Fetch URL and return structured Markdown.
  - Input: `url` (string)

## System

- **`bash`**: Run shell commands (PowerShell on Windows, Bash on Linux/Mac).
  - Input: `command` (string)
- **`mcp_debug`**: Call an MCP tool directly.
- **`todo_write`**: Update the Todo/task list.
  - Aliases: `todowrite`, `update_todo_list`
- **`task`**: Update the current task status.
- **`question`**: Ask a structured question to the user and wait for input.

## Specialized

- **`skill`**: Execute a skill.
- **`apply_patch`**: Apply a unified diff patch.
- **`batch`**: Execute multiple tools in a batch.
- **`lsp`**: Interact with the Language Server Protocol.
