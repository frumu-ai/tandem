---
title: Agent Runtime Contracts
description: Canonical boundaries agents should preserve when explaining or operating Tandem.
---

Use this page when an agent needs a compact, reliable model of Tandem before answering questions, creating workflows, debugging runs, or deciding which API/tool surface to use.

Tandem has several systems that sound similar in conversation but are intentionally separate in the runtime. Do not flatten them into one bucket.

## Core objects

| Object        | What it is                                                     | What it is not                                    |
| ------------- | -------------------------------------------------------------- | ------------------------------------------------- |
| Session       | Durable conversation record with messages and metadata         | The active execution                              |
| Run           | One execution attached to a session                            | The whole conversation history                    |
| Message       | A user or assistant turn made of ordered parts                 | A reusable memory record                          |
| Part          | Text, reasoning, tool call, or tool result inside a message    | A standalone workflow step                        |
| Event         | Streamed runtime state change over SSE                         | A replacement for persisted state                 |
| Tool          | Policy-controlled action the model may request                 | Proof that the tool is allowed everywhere         |
| Memory        | Reusable retrieval store in `memory.sqlite`                    | The raw transcript                                |
| Artifact      | Durable output file, upload, handoff, or blackboard product    | Semantic memory unless explicitly stored/promoted |
| Knowledge     | Promoted reusable project/global fact used by preflight        | Raw notes or unvalidated output                   |
| Workflow plan | Planner draft/session that can be previewed and applied        | A running automation by itself                    |
| V2 automation | Persistent runnable DAG, optionally scheduled                  | A loose chat conversation                         |
| Mission       | Higher-level staged operating loop or work tracker             | A single LLM call                                 |
| Context run   | Durable execution state for blackboard/checkpoint/replay flows | User memory                                       |

## Request lifecycle

The default chat path is:

1. A client appends a user message to a session.
2. The engine creates or attaches to a run.
3. The engine builds a bounded provider payload from session history, runtime context hooks, memory, tool policy, and model/provider config.
4. The provider streams model output.
5. Tool calls are parsed, policy-checked, executed, and appended as message parts.
6. Runtime progress streams as events.
7. The run finishes, and the session remains available for later turns.

This means the model does not receive "everything Tandem knows." It receives a bounded, derived payload for the current run.

## Tool access

Tool visibility and tool execution are policy surfaces.

Agents should:

- inspect available tools instead of inventing names
- treat MCP catalog visibility as different from MCP execution access
- use explicit allowlists for autonomous work
- consider `memory_delete`, shell, file writes, browser mutation, and remote MCP mutation as narrower capabilities than read/search
- stop and ask for capability setup when a required connector or tool is absent

Related pages:

- [Tools Reference](./reference/tools/)
- [MCP Capability Discovery And Request Flow](./mcp-capability-discovery-and-request-flow/)
- [MCP Automated Agents](./mcp-automated-agents/)

## Memory contract

Tandem has multiple memory-like layers:

- session history stores the raw transcript
- vector-backed retrieval chunks support semantic recall
- FTS/BM25-backed governed records support user-keyed memory tools
- knowledge records store promoted reusable facts
- artifacts store inspectable outputs
- context runs store replay/checkpoint state

An agent should say which layer it means. For example, "search project memory" is different from "read the session transcript" and different again from "inspect run artifacts."

For details, use [Memory Internals](./memory-internals/).

## Workflow contract

Use the right abstraction:

- **Workflow plan** when the engine should generate or revise a workflow from natural language.
- **V2 automation** when the runnable DAG is known or should be scheduled.
- **Mission builder** when work spans staged dependent workstreams.
- **Missions runtime** when a mission already exists and work state needs to move forward.
- **Context run / blackboard** when execution state, artifacts, checkpoints, or replay matter.

Agents should preview before apply, preserve provenance, schedule only after the workflow is durable, and repair failed stages before recreating the whole system.

Related pages:

- [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)
- [Agent Workflow Operating Manual](./agent-workflow-operating-manual/)
- [Prompting Workflows And Missions](./prompting-workflows-and-missions/)

## Failure and repair contract

When something fails, distinguish:

- provider/model failure
- tool permission denial
- MCP auth or connector gap
- validation or artifact contract failure
- workflow node failure
- schedule/misfire behavior
- stale or missing memory
- replay/checkpoint inconsistency

Do not default to recreating the workflow. Inspect the run, checkpoint, artifacts, events, and governance/audit surfaces first.

When a failure, safety signal, policy gap, or recurring runtime issue needs durable evidence and governed routing, use [Incident Monitor Agent Runtime Guide](./incident-monitor/agent-runtime-guide/) instead of mutating GitHub, Linear, webhooks, memory, telemetry, or MCP destinations directly.

## Answering questions from docs

When using these docs through MCP or a published index:

1. Prefer specific runtime nouns over generic words like "memory," "context," or "agent."
2. Mention whether a surface is durable, derived, streamed, or policy-gated.
3. Separate what is available in the UI, SDK, HTTP API, CLI, and agent tool registry.
4. If the docs describe a capability but the current engine/tool inventory does not expose it, treat that as unavailable until verified.
5. Link to the most specific page rather than summarizing from memory.

## See also

- [How Tandem Works Under the Hood](./how-tandem-works/)
- [Architecture](./architecture/)
- [Agents & Sessions](./agents-and-sessions/)
- [Memory Internals](./memory-internals/)
- [Storage Maintenance](./storage-maintenance/)
- [Incident Monitor Agent Runtime Guide](./incident-monitor/agent-runtime-guide/)
