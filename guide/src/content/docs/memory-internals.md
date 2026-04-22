---
title: Memory Internals
description: The storage-level model behind Tandem memory, retrieval, replay, and knowledge reuse.
---

Tandem keeps four different things separate:

- the raw transcript
- the retrieval memory
- the run replay state
- the artifacts and checkpoints

If you are answering "how does Tandem remember things?", this page is the storage-level version of that answer.

## The short version

- **History** is what happened in a session.
- **Memory** is reusable recall stored in `memory.sqlite`.
- **Replay state** is how a run can be reconstructed later.
- **Artifacts** are the files and outputs created by a run.
- **Knowledge reuse** is promoted memory that later runs can preflight and reuse.

The engine uses these layers so it can stay bounded in context without pretending that the whole transcript is the prompt forever.

## Storage surfaces

| Surface                | Backing store            | What it holds                                                                | Scope                 |
| ---------------------- | ------------------------ | ---------------------------------------------------------------------------- | --------------------- |
| Working memory         | Prompt context only      | The current turn, bounded memory context, and runtime state                  | Per turn              |
| Session history        | Session storage          | Messages, turns, and the source-of-truth conversation record                 | Per session           |
| Retrieval memory       | `memory.sqlite`          | Session/project/global memory chunks, retrieval state, and knowledge records | Cross-session, scoped |
| Knowledge reuse        | `memory.sqlite`          | Reusable project or global knowledge that workflows can preflight            | Project / global      |
| Artifact memory        | Files and uploads        | Output files, handoffs, attachments, blackboard artifacts                    | Per run or workspace  |
| Execution memory       | `context_runs/<run_id>/` | Run state, events, patches, and checkpoints                                  | Per run               |
| Cache / derived memory | Cache files              | Response caches, embedding caches, and other acceleration layers             | Derived               |

## Memory layers

### Working memory

Working memory is the prompt-time context the model sees right now.

It includes:

- the current conversation window
- the bounded memory context block
- temporary runtime state

This is not durable storage. If it is not written into a store, it disappears when the turn ends.

### Session history

Session history is the raw transcript.

It answers:

- what the user said
- what the agent replied
- what tool calls happened
- what the run status was

Session history is the source of truth for the conversation, but it is not the same thing as reusable memory.

### Retrieval memory

Retrieval memory lives in `memory.sqlite`.

It is tiered:

- `session` memory is ephemeral and session-scoped
- `project` memory is reusable inside a project
- `global` memory is cross-session and cross-project

This is the memory that gets searched when Tandem wants reusable facts without replaying the whole transcript.

### Knowledge reuse

Tandem also keeps structured reusable knowledge in the same memory store.

That layer exists so later runs can:

- preflight prior knowledge before recomputing
- reuse validated facts instead of starting over
- keep workflow memory project-scoped instead of flat and global

Think of this as promoted memory, not raw chat history.

### Artifact memory

Artifacts are durable outputs, not the same thing as memory.

Examples include:

- file outputs and handoffs
- channel uploads
- blackboard artifacts
- run-generated docs and reports

Artifacts are important because they are inspectable and durable, but they do not become semantic memory unless Tandem explicitly stores or promotes them.

### Execution memory / resumability

Execution memory is the run-state layer.

It lets Tandem reconstruct or replay work using:

- `run_state.json`
- `events.jsonl`
- `blackboard.json`
- `blackboard_patches.jsonl`
- `checkpoints/`

This is different from knowledge memory. It answers "how did the run evolve?" rather than "what should the model remember?"

### Cache / derived memory

Cache layers make memory faster, but they are not the source of truth.

Examples include:

- response caches
- embedding caches
- derived indexes

If a cache is lost, Tandem should still be able to rebuild the real memory from the underlying stores.

## How memory gets written

### Session writes

Each new message is appended to the session history.

That keeps the conversation record intact even when the prompt window is truncated later.

### Retrieval memory writes

Tandem stores retrieval memory when something is worth reusing later.

Typical write paths include:

- `memory.put` for a new governed memory record
- `memory.promote` to move something into a more reusable tier
- `memory.demote` to reduce visibility or scope
- `contextDistill` to extract durable memories from a session conversation

### Channel archival writes

Channel integrations keep raw transcripts and retrieval memory separate.

After a successful channel reply, Tandem archives the completed user + assistant exchange into global retrieval memory, so future channel sessions can recall it without loading the full transcript every time.

### Run-state writes

During execution, Tandem writes run state atomically and appends event and patch records.

That is what makes replay possible later.

## How memory gets read back

### Prompt injection

When Tandem builds context for a run, it injects bounded memory into a structured block such as:

```text
<memory_context>
  <current_session>...</current_session>
  <relevant_history>...</relevant_history>
  <project_facts>...</project_facts>
</memory_context>
```

This keeps the model focused on reusable facts rather than forcing it to reread the full transcript.

### Search

The memory APIs let you search stored memory by query and scope.

Typical uses:

- search prior work by project or user
- find reusable facts from earlier runs
- inspect what has already been promoted

### Replay

The run replay path reconstructs execution from the persisted run state plus later events and blackboard patches.

This is useful when you want to answer:

- what happened
- what changed
- why a run ended the way it did

## What to teach agents

Use these distinctions consistently:

- **History** tells you what happened.
- **Memory** tells you what should be reusable.
- **Artifacts** tell you what was produced.
- **Replay state** tells you how the execution unfolded.

Do not flatten them into one "memory" bucket.

The most common mistakes are:

- treating session history as long-term memory
- treating artifacts as semantic recall
- treating cache as durable knowledge
- treating replay state as if it were user knowledge

## Public APIs worth knowing

- `client.memory.put`
- `client.memory.search`
- `client.memory.list`
- `client.memory.promote`
- `client.memory.demote`
- `client.memory.delete`
- `client.memory.audit`
- `client.memory.contextResolveUri`
- `client.memory.contextTree`
- `client.memory.contextGenerateLayers`
- `client.memory.contextDistill`

## Related docs

- [How Tandem Works Under the Hood](https://docs.tandem.ac/how-tandem-works/)
- [Agents & Sessions](https://docs.tandem.ac/agents-and-sessions/)
- [Channel Integrations](https://docs.tandem.ac/channel-integrations/)
- [Headless Service](https://docs.tandem.ac/headless-service/)
- [TypeScript SDK](https://docs.tandem.ac/sdk/typescript/)
- [Python SDK](https://docs.tandem.ac/sdk/python/)
