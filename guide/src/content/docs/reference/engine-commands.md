---
title: Engine Commands
---

The `tandem-engine` binary supports several subcommands for running the server and executing tasks.

## `serve`

Starts the Tandem Engine server. This is the default mode for handling client connections.

```bash
tandem-engine serve [OPTIONS]
```

**Options:**

- `--hostname <HOSTNAME>` / `--host <HOSTNAME>`: The interface to bind to (default: `127.0.0.1`).
- `--port <PORT>`: The port to listen on (default: `39731`).
- `--state-dir <DIR>`: Custom directory for storing engine state (config, logs, storage).
- `--in-process`: Run in in-process mode (for development/debugging).
- `--provider <ID>`: Provider ID for this process (`openai`, `openrouter`, `anthropic`, `ollama`, `groq`, `mistral`, `together`, `azure`, `bedrock`, `vertex`, `copilot`, `cohere`).
- `--model <ID>`: Provider model override for this process.
- `--api-key <KEY>`: API key override for the selected provider for this process.
- `--config <PATH>`: Override config file path.
- `--api-token <TOKEN>`: Require token auth for HTTP endpoints (Authorization Bearer or `X-Tandem-Token`).

## `run`

Execute a single prompt and exit. Useful for quick CLI queries or scripting.

```bash
tandem-engine run "<PROMPT>"
```

**Options:**

- `--provider <ID>`: Provider for this run. Unknown IDs fail fast.
- `--model <ID>`: Provider model override for this run.
- `--api-key <KEY>`: API key override for this run's provider.
- `--config <PATH>`: Override config file path.

**Example:**

```bash
tandem-engine run "What is the capital of France?"
```

**Provider precedence:**

- `run --provider` uses that provider explicitly.
- If no explicit provider is passed, `default_provider` from config is used.
- If `default_provider` is missing or unavailable, Tandem falls back to the first configured provider.

**API key behavior:**

- `--api-key` applies only to the selected provider for that command invocation.
- Without `--api-key`, Tandem uses provider-specific config/env vars (for example `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`).

## `tool`

Execute a specific tool directly by passing a JSON payload.

```bash
tandem-engine tool --json '<JSON_PAYLOAD>'
```

**Options:**

- `--json <JSON>`: The JSON payload defining the tool and arguments. Can be a raw string, a file path (`@path/to/file.json`), or `-` for stdin.
- `--state-dir <DIR>`: Custom state directory.

**Example Payload:**

```json
{
  "tool": "read",
  "args": {
    "path": "README.md"
  }
}
```

## `chat`

(Experimental) Starts an interactive chat session directly in the terminal without looking for a client.

## `parallel`

Run multiple prompts concurrently and return a JSON summary.

```bash
tandem-engine parallel --json '<JSON_PAYLOAD>' --concurrency 4
```

**Options:**

- `--json <JSON>`: Array of prompts, array of objects, or `{ "tasks": [...] }` wrapper. Accepts raw JSON, `@file`, or `-` for stdin.
- `--concurrency <N>`: Max concurrent tasks (default: `4`).
- `--provider <ID>`: Default provider for tasks without explicit provider.
- `--model <ID>`: Default model override for the provider.
- `--api-key <KEY>`: API key override for this batch.
- `--config <PATH>`: Override config file path.

## `providers`

List supported provider IDs for `--provider`.

```bash
tandem-engine providers
```

## `token`

API token utilities (used with `--api-token`).

```bash
tandem-engine token generate
```
