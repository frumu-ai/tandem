---
title: Tools Reference
---

The following tools are available in the Tandem Engine environment.

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

## Web

- **`webfetch`**: Fetch raw URL text.
  - Input: `url` (string)
- **`webfetch_document`**: Fetch URL and return structured Markdown.
  - Input: `url` (string)

## System

- **`bash`**: Run shell commands (PowerShell on Windows, Bash on Linux/Mac).
  - Input: `command` (string)
- **`mcp_debug`**: Call an MCP tool directly.
- **`todowrite`**: Update the Todo list.
- **`task`**: Update the current task status.

## Specialized

- **`skill`**: Execute a skill.
- **`apply_patch`**: Apply a unified diff patch.
- **`batch`**: Execute multiple tools in a batch.
- **`lsp`**: Interact with the Language Server Protocol.
