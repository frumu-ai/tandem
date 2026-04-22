---
title: Engine Authentication For Agents
description: How agents and external clients obtain a Tandem engine token and authorize SDK or HTTP calls safely.
---

Any agent, script, SDK client, or external service talking to `tandem-engine` over HTTP/SSE needs an engine token when auth is enabled.

This page explains:

- how the token is created
- where an operator may store it
- how agents should use it
- how to avoid common auth mistakes

## The short version

To start a local engine with a token:

```bash
tandem-engine serve \
  --hostname 127.0.0.1 \
  --port 39731 \
  --api-token "$(tandem-engine token generate)"
```

Then authorize requests with either:

- `X-Agent-Token: <token>`
- or an SDK client configured with `token: "<token>"`

## How tokens are usually created

### CLI-generated token

The most common path is:

```bash
tandem-engine token generate
```

That generated token is then passed into:

- `--api-token "<token>"`
- or `TANDEM_API_TOKEN=<token>`

### Control panel-managed token

When using the packaged control panel flow:

```bash
tandem panel init
```

Tandem creates or updates the local `.env` and ensures an engine token is available for the panel.

Important environment variable:

- `TANDEM_CONTROL_PANEL_ENGINE_TOKEN`

The panel uses that token to talk to the engine.

## Where an agent might get the token

Only use a token source that has been intentionally provided to the agent.

Common legitimate sources are:

- an explicit environment variable such as `TANDEM_API_TOKEN`
- a process launch command that includes `--api-token`
- the control panel environment file containing `TANDEM_CONTROL_PANEL_ENGINE_TOKEN`
- a secrets manager or operator-provided config layer
- an SDK constructor that already has the token passed in

Agents should **not** assume they are allowed to scan arbitrary files or shell history for secrets unless the task explicitly permits that.

## How to authenticate HTTP calls

### Header form used in the docs

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Agent-Token: tk_your_token"
```

Use the same header for mission, workflow, automation, and memory routes.

### Example: mission builder preview

```bash
curl -sS -X POST http://127.0.0.1:39731/mission-builder/compile-preview \
  -H "X-Agent-Token: tk_your_token" \
  -H "content-type: application/json" \
  -d @mission-blueprint.json
```

### Example: workflow plan preview

```bash
curl -sS -X POST http://127.0.0.1:39731/workflow-plans/preview \
  -H "X-Agent-Token: tk_your_token" \
  -H "content-type: application/json" \
  -d '{"prompt":"Create a staged automation for recurring intake and verification."}'
```

### Example: V2 automation create

```bash
curl -sS -X POST http://127.0.0.1:39731/automations/v2 \
  -H "X-Agent-Token: tk_your_token" \
  -H "content-type: application/json" \
  -d @automation.json
```

## How to authenticate SDK calls

### TypeScript

```ts
import { TandemClient } from "@frumu/tandem-client";

const client = new TandemClient({
  baseUrl: "http://localhost:39731",
  token: process.env.TANDEM_API_TOKEN || "",
});
```

### Python

```python
from tandem_client import AsyncTandemClient
import os

client = AsyncTandemClient(
    base_url="http://localhost:39731",
    token=os.environ["TANDEM_API_TOKEN"],
)
```

In the SDK path, the client handles the request header for you.

## How an agent should decide what to call

After authentication is set up:

- use workflow plans when intent must be compiled into an automation
- use mission builder when you want a staged mission blueprint compiled and applied
- use V2 automations when the DAG is already known
- use missions runtime when you are updating mission work state

See [Creating And Running Workflows And Missions](https://docs.tandem.ac/creating-and-running-workflows-and-missions/) for the path selection guide.

## Safe patterns for agents

Good agent behavior:

- read token from an explicitly provided environment variable or config source
- send authenticated requests only to the intended engine URL
- fail clearly when the token is missing
- treat the token as a secret and avoid echoing it back in logs or artifacts

Bad agent behavior:

- hardcoding a token into source files
- writing the token into workflow artifacts or mission outputs
- printing the token in terminal transcripts or chat replies
- assuming a control panel `.env` is always the right credential source

## How to verify the token works

Use a health check first:

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Agent-Token: tk_your_token"
```

If that succeeds, the same token should work for:

- workflow plan routes
- mission builder routes
- automations routes
- missions routes
- memory routes

## Common failure cases

### 401 or unauthorized

Usually means:

- token is missing
- wrong token
- wrong header
- wrong engine instance

### Control panel works but your script does not

Usually means:

- the panel has `TANDEM_CONTROL_PANEL_ENGINE_TOKEN`, but your script does not
- your script is pointing at the wrong `baseUrl`

### Local engine restart broke your script

Check whether:

- the engine was restarted with a new token
- `TANDEM_API_TOKEN` changed
- your script cached an old token

## Recommended operating pattern

For agents that need to create and run workflows or missions:

1. verify engine URL
2. obtain token from an explicitly provided secure source
3. call `/global/health`
4. choose the right authoring path
5. preview before apply
6. apply and schedule
7. inspect runs with the same authenticated client

Import preview is read-only, but durable import, apply, and repair all mutate engine state, so they should use the same authenticated client path as the rest of the workflow lifecycle.

## See also

- [Agent Workflow And Mission Quickstart](https://docs.tandem.ac/agent-workflow-mission-quickstart/)
- [Choosing Providers And Models For Agents](https://docs.tandem.ac/choosing-providers-and-models-for-agents/)
- [Creating And Running Workflows And Missions](https://docs.tandem.ac/creating-and-running-workflows-and-missions/)
- [Prompting Workflows And Missions](https://docs.tandem.ac/prompting-workflows-and-missions/)
- [Headless Service](https://docs.tandem.ac/headless-service/)
- [TypeScript SDK](https://docs.tandem.ac/sdk/typescript/)
- [Python SDK](https://docs.tandem.ac/sdk/python/)
