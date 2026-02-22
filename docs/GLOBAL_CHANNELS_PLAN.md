# Global Channel Awareness Feature Plan

This document outlines the architecture for allowing external chat channels (Slack, Discord, Telegram) to interact with and monitor the global state of the Tandem Engine, beyond just single-session isolation.

## The Goal
Currently, when a user interacts via a chat channel, that interaction is scoped strictly to a single `Session`. The agent does not inherently know about background `Routines`, parallel `Swarms`, or long-running `Missions` happening elsewhere on the engine. 

We want to allow a chat channel to:
1.  **Monitor Global State:** Receive notifications when a routine finishes, a mission fails, or a swarm requires approval.
2.  **Act as an Orchestrator:** Allow the user, via Slack/Discord, to query active jobs ("What is running right now?") or instruct the engine to spawn new background tasks.

## Current Architecture Analysis (`dispatcher.rs`)

Right now, `tandem-channels/src/dispatcher.rs` works exclusively via polling HTTP endpoints relative to a specific session:
1. Maps a `{channel_name}:{sender_id}` to a single Tandem `session_id`.
2. When a channel says something, it calls `POST /session/{id}/prompt_sync`.
3. It extracts the raw `text` and pushes it back to the channel.

It lacks two critical things for "Global" awareness:
1.  **Event Subscription:** It does not listen to `/event` or `/routines/events` (the Server-Sent Events streams). Thus, it has no way to know when external things happen unless the user actively queries it.
2.  **Global Tool Access:** Because it routes directly to a `prompt_sync` chat session, the agent's context is limited to that session's tools. It cannot view global routines or missions natively.

## Implementation Strategy

To give chat channels global awareness, we need a two-pronged approach:

### 1. The Global "Watcher" (Push Notifications to Chat)
Instead of just routing *incoming* messages, the channel dispatcher needs a background thread that listens to global engine events.

**Implementation Steps:**
1.  **Subscribe to SSE:** In `start_channel_listeners` (in `dispatcher.rs`), spawn a persistent asynchronous task that connects to the engine's global event streams (`/event` and `/routines/events`).
2.  **Event Filtering:** Define which events are "chat-worthy" (e.g., `mission.failed`, `routine.completed`, `swarm.requires_approval`).
3.  **Broadcast Selection:** Determine *which* channel(s) or user(s) should receive these notifications. We likely need a "Global Admin Channel" configuration setting, where system-wide updates are pushed (e.g., `#tandem-alerts` in Slack).
4.  **Format and Push:** When a `mission.failed` event is received via SSE, format it into a nice markdown string and use `Channel::send` to push it proactively to the designated admin channel.

### 2. The "Global CLI" Sessions (Pulling Data from Chat)
To let users control swarms/routines from Slack, we can introduce special `/` commands, or we can use dedicated "Admin Agent Sessions."

**Implementation Steps:**
1.  **New Slash Commands:** Expand `parse_slash_command` in `dispatcher.rs` to include orchestration commands:
    *   `/routines` -> Queries `GET /routines` and prints the active cronjobs to Chat.
    *   `/missions` -> Queries `GET /mission` and prints long-running jobs to Chat.
2.  **Special Admin Sessions:** Alternatively, we could create a specialized agent that is inherently seeded with global API access. When a user in chat types `/admin What is running?`, the dispatcher routes this to a specific predefined Admin session that possesses tools allowing it to read the `GET /mission` and `GET /routines` endpoints directly.

## Next Steps

To build this feature:
1.  We need to modify the `ChannelsConfig` to accept a `global_notification_channel` (e.g., a specific Slack channel ID).
2.  We need to introduce an `sse_listener` module inside `tandem-channels` that uses `reqwest-eventsource` to listen to the engine.
3.  We need to map JSON payload events (like `routine.completed`) into human-readable Slack/Discord messages.

> **Note for User:** Since this requires modifying the core Rust `tandem-channels` crate (and potentially `tandem-server`), this is a backend engineering task, isolated from the Vite/VPS examples we are building on the frontend. We can leave this plan here as a roadmap for when you want to dive back into the Rust backend!
