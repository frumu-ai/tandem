# Automated Agents Architecture Improvements

This document is a detailed, agent-executable plan for improving Tandem's automated channel bots (Discord, Slack, Telegram) and their interaction with the MCP/Composio/Arcade subsystem.

## Baseline Facts (Discovered by Code Investigation)

| Item | Current State |
|---|---|
| Session prompt route | `POST /session/{id}/prompt_sync` — **synchronous, 5-min timeout** |
| Async prompt route | `POST /session/{id}/prompt_async` — **exists but unused by dispatcher** |
| Global SSE event stream | `GET /event` and `GET /global/event` — **fully operational** |
| Tool approve/deny | `POST /sessions/{sid}/tools/{tcid}/approve` and `/deny` — **already in tandem-server** |
| Routine/cron scheduler | `RoutineSpec` + `RoutineSchedule` + `run_routine_scheduler` — **fully built in tandem-server** |
| Channel slash commands | `/new`, `/sessions`, `/resume`, `/rename`, `/status`, `/help` — defined in `dispatcher.rs:SlashCommand` |
| Memory DB | `tandem-memory/src/db.rs`, WAL mode on, `rusqlite = "0.32"` bundled |
| Memory tables | `session_memory_chunks`, `project_memory_chunks`, `global_memory_chunks` — all have `created_at TEXT` |
| Memory retention config | `memory_config` table has `session_retention_days INTEGER DEFAULT 30` — but no enforcement task |
| Response cache | **Does not exist** |
| `sha2` crate | Already in `tandem-server/Cargo.toml` and `tandem-memory/Cargo.toml` |
| `tokio-stream` | Already in `tandem-server/Cargo.toml` |

---

## Improvement A: Switch Dispatcher from `prompt_sync` to `prompt_async` + SSE

**Why:** The dispatcher blocks on `prompt_sync` with a 5-minute timeout. `prompt_async` already exists and returns immediately; the channel listener can stream `GET /event` to receive results.

**Files to modify:**

### `crates/tandem-channels/src/dispatcher.rs`

**Step 1** — Find and replace the `run_in_session` function (the one that calls `prompt_sync`). Replace the `reqwest` call from:
```
POST {base_url}/session/{session_id}/prompt_sync
```
with:
```
POST {base_url}/session/{session_id}/prompt_async
```
This returns immediately with `{ "runID": "..." }`.

**Step 2** — After sending `prompt_async`, open an SSE connection to receive the result:
```
GET {base_url}/event?sessionID={session_id}&runID={run_id}
```
Use `tokio-stream` (already a dependency) to consume the `text/event-stream` response. Accumulate `session.message.delta` events into a string buffer. Stop consuming on `session.run.finished` event. The accumulated text is the agent's reply.

**Step 3** — During the SSE stream loop, if a `session.permission.requested` event arrives (tool approval needed), call `prompt_approval_in_channel` (see Improvement B).

**Step 4** — Send progress updates back to the channel every N seconds of streaming (or on each `session.tool.started` event):
```rust
// Telegram example
bot.send_message(chat_id, "🔧 Agent is using tool: {tool_name}...").await
```

**Step 5** — Replace the 5-minute hardcoded `reqwest::Client` timeout with a per-channel configurable `TANDEM_{CHANNEL}_MAX_WAIT_SECONDS` env var (default: 600). Apply this as the outer `tokio::time::timeout` around the SSE stream loop, not on the HTTP connection itself.

---

## Improvement B: Route Tool Approvals Back to the Channel

**Why:** `tool_proxy.rs` and the server route `POST /sessions/{sid}/tools/{tcid}/approve|deny` already exist. The dispatcher just needs to listen for permission events and relay them to the channel.

**Files to modify:**

### `crates/tandem-channels/src/dispatcher.rs`

**Step 1** — In `SlashCommand` (line ~101), add two new variants:
```rust
Approve { operation_id: String },
Deny { operation_id: String },
```

**Step 2** — In `parse_slash_command` (line ~110), add parsing:
```rust
if let Some(id) = trimmed.strip_prefix("/approve ") {
    return Some(SlashCommand::Approve { operation_id: id.trim().to_string() });
}
if let Some(id) = trimmed.strip_prefix("/deny ") {
    return Some(SlashCommand::Deny { operation_id: id.trim().to_string() });
}
```

**Step 3** — Add a `handle_approve` / `handle_deny` function that calls the tandem-server:
```rust
async fn relay_tool_decision(
    base_url: &str, token: &str, session_id: &str,
    tool_call_id: &str, approved: bool,
    client: &reqwest::Client,
) -> anyhow::Result<()> {
    let action = if approved { "approve" } else { "deny" };
    let url = format!("{base_url}/sessions/{session_id}/tools/{tool_call_id}/{action}");
    add_auth(client.post(&url), token).send().await?;
    Ok(())
}
```

**Step 4** — In the SSE stream loop (from Improvement A), when `session.permission.requested` fires, extract `toolCallID` and `tool` from the event payload, then send a message to the channel:
```
⚠️ Agent wants to use tool `{tool}`.
Reply /approve {toolCallID} or /deny {toolCallID}
```

---

## Improvement C: Add Autonomy & Tool Policy to `ChannelsConfig`

**Why:** All channel bots currently share the same tool policy as the desktop app. Adding per-channel autonomy levels enables bots to auto-approve safe tools without blocking.

**Files to modify:**

### `crates/tandem-channels/src/config.rs`

**Step 1** — Add these types above the existing structs (around line 20):
```rust
#[derive(Debug, Clone, Default)]
pub enum ChannelAutonomy {
    #[default]
    Supervised,  // All tool calls require /approve from the channel
    SemiAuto,    // Auto-approve read-only tools; require approval for writes
    Full,        // Execute all tools without gates
}

#[derive(Debug, Clone, Default)]
pub struct ChannelToolPolicy {
    pub autonomy: ChannelAutonomy,
    pub auto_approve: Vec<String>,
    pub block: Vec<String>,
}
```

**Step 2** — Add `pub tool_policy: ChannelToolPolicy` field to each of `TelegramConfig`, `DiscordConfig`, and `SlackConfig`.

**Step 3** — In each `*_from_env()` function (e.g., `discord_from_env` at line ~115), add parsing:
```rust
let autonomy = match std::env::var("TANDEM_DISCORD_AUTONOMY").as_deref() {
    Ok("full") => ChannelAutonomy::Full,
    Ok("semi_auto") => ChannelAutonomy::SemiAuto,
    _ => ChannelAutonomy::Supervised,
};
let auto_approve = std::env::var("TANDEM_DISCORD_AUTO_APPROVE")
    .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
    .unwrap_or_default();
let block = std::env::var("TANDEM_DISCORD_BLOCK")
    .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
    .unwrap_or_default();
```

**Step 4** — In the dispatcher's SSE stream loop, before sending a `/approve` prompt to the channel, first check `tool_policy`:
- If tool is in `block` → auto-deny immediately
- If tool is in `auto_approve` OR `autonomy == Full` → auto-approve immediately  
- If tool is in `auto_approve` + `autonomy == SemiAuto` → auto-approve
- Otherwise → send approval request to channel

---

## Improvement D: Extend Session Persistence with Policy State

**Why:** Currently `SessionMap` only maps `{channel}:{sender} → session_id`. After a bot restart, pending approvals and tool policies are lost.

**Files to modify:**

### `crates/tandem-channels/src/dispatcher.rs`

**Step 1** — Replace the `SessionMap` type alias and the value type stored in the map. Change the value from `String` to a new struct:
```rust
#[derive(Serialize, Deserialize, Clone)]
struct SessionRecord {
    session_id: String,
    pending_approvals: Vec<String>,  // tool_call_ids awaiting /approve
}

pub type SessionMap = Arc<Mutex<HashMap<String, SessionRecord>>>;
```

**Step 2** — Update `load_session_map` and `save_session_map` to serialize/deserialize `SessionRecord` instead of `String`.

**Step 3** — All code that previously read/wrote `session_id` directly from the map now reads `record.session_id`.

---

## Improvement E: LLM Response Cache

**Why:** Channel bots often receive the same common questions from multiple users. A response cache avoids burning tokens on identical prompts.

**Files to create / modify:**

### New file: `crates/tandem-memory/src/response_cache.rs`

Create a new Rust source file implementing a `ResponseCache` struct backed by a SQLite table in a **separate** database file (`response_cache.db`) so it can be wiped without touching memory chunks.

**Dependencies:** `rusqlite = "0.32"` (already in `tandem-memory/Cargo.toml`), `sha2 = "0.10"` (already present), `parking_lot` (if available, otherwise `std::sync::Mutex`).

**Schema to create on `ResponseCache::new`:**
```sql
CREATE TABLE IF NOT EXISTS response_cache (
    prompt_hash  TEXT PRIMARY KEY,
    model        TEXT NOT NULL,
    response     TEXT NOT NULL,
    token_count  INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL,
    accessed_at  TEXT NOT NULL,
    hit_count    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_rc_accessed ON response_cache(accessed_at);
CREATE INDEX IF NOT EXISTS idx_rc_created  ON response_cache(created_at);
```

**Cache key computation:**
```rust
pub fn cache_key(model: &str, system_prompt: Option<&str>, user_prompt: &str) -> String {
    let mut hasher = Sha256::new(); // sha2::Sha256
    hasher.update(model.as_bytes());
    hasher.update(b"|");
    if let Some(s) = system_prompt { hasher.update(s.as_bytes()); }
    hasher.update(b"|");
    hasher.update(user_prompt.as_bytes());
    format!("{:064x}", hasher.finalize())
}
```

**`get` method:** Query `WHERE prompt_hash = ?1 AND created_at > ?2` (cutoff = now - TTL). On hit, `UPDATE` the `accessed_at` and `hit_count`. Return `Option<String>`.

**`put` method:** `INSERT OR REPLACE`. After insert, `DELETE` expired rows, then LRU-evict oldest `accessed_at` if `COUNT(*) > max_entries`.

**`stats` method:** Return `(total_entries, total_hits, estimated_tokens_saved)`.

### `crates/tandem-memory/src/lib.rs`

Add `pub mod response_cache;` and re-export `ResponseCache`.

### `crates/tandem-server/src/lib.rs` (AppState initialization)

Add an `Option<ResponseCache>` field to `AppState`. Initialize it from env vars:
```
TANDEM_RESPONSE_CACHE_ENABLED=true
TANDEM_RESPONSE_CACHE_TTL_MINUTES=60    (default)
TANDEM_RESPONSE_CACHE_MAX_ENTRIES=500   (default)
```

The cache file lives next to `memory.sqlite` as `response_cache.db`.

### `crates/tandem-core/src/engine_loop.rs`

Before calling the LLM provider, compute the cache key from the current model + system prompt + final user message. Check `app_state.response_cache.get(&key)`. If hit, skip the LLM call and return the cached response directly into the message stream. On miss, call the LLM normally and store the response on completion.

---

## Improvement F: SQLite Memory Hygiene

**Why:** The `session_memory_chunks` table in `tandem-memory/src/db.rs` grows unbounded. The `memory_config` table already stores `session_retention_days INTEGER DEFAULT 30` — this just needs to be enforced.

**Files to modify:**

### `crates/tandem-memory/src/db.rs`

**Step 1** — Add a new `async fn prune_old_session_chunks(&self, retention_days: u32) -> MemoryResult<u64>` method to `MemoryDatabase`:

```rust
pub async fn prune_old_session_chunks(&self, retention_days: u32) -> MemoryResult<u64> {
    if retention_days == 0 { return Ok(0); }
    let conn = self.conn.lock().await;
    // WAL mode is already set in new(); no need to set it again here
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(retention_days as i64))
        .to_rfc3339();
    let deleted = conn.execute(
        "DELETE FROM session_memory_chunks WHERE created_at < ?1",
        rusqlite::params![cutoff],
    )?;
    // Also clean up orphaned vectors
    conn.execute(
        "DELETE FROM session_memory_vectors 
         WHERE chunk_id NOT IN (SELECT id FROM session_memory_chunks)",
        [],
    )?;
    Ok(deleted as u64)
}
```

**Step 2** — Expose this via a public `run_hygiene` method on `MemoryDatabase` that reads `session_retention_days` from the `memory_config` table (using project_id = `NULL` for global config or a special sentinel) and calls `prune_old_session_chunks`.

### `crates/tandem-server/src/http.rs` — `serve()` function (around line 556)

**Step 3** — After the existing `tokio::spawn`s for the reaper and routine scheduler, add a hygiene task:
```rust
let hygiene_state = state.clone();
tokio::spawn(async move {
    loop {
        tokio::time::sleep(Duration::from_secs(12 * 60 * 60)).await; // every 12 hours
        let retention = std::env::var("TANDEM_MEMORY_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(30);
        if let Err(e) = hygiene_state.memory_db.run_hygiene(retention).await {
            tracing::warn!("Memory hygiene failed: {}", e);
        }
    }
});
```

The env var `TANDEM_MEMORY_RETENTION_DAYS=0` disables pruning.

---

## Improvement G: Middleware Hook Pipeline

**Why:** There is currently no way to intercept LLM calls, tool calls, or session events from outside the core engine without editing core code.

**Files to create / modify:**

### New file: `crates/tandem-core/src/hooks.rs`

```rust
use async_trait::async_trait;
use std::time::Duration;
use serde_json::Value;

pub enum HookResult<T> {
    Continue(T),
    Cancel(String),
}

#[async_trait]
pub trait HookHandler: Send + Sync {
    fn name(&self) -> &str;
    fn priority(&self) -> i32 { 0 }

    // Modifying hooks — return Continue(modified) or Cancel(reason)
    async fn before_tool_call(&self, name: String, args: Value) -> HookResult<(String, Value)> {
        HookResult::Continue((name, args))
    }
    async fn before_llm_call(&self, messages: Vec<serde_json::Value>) -> HookResult<Vec<serde_json::Value>> {
        HookResult::Continue(messages)
    }

    // Observable hooks — fire and forget
    async fn on_after_tool_call(&self, _tool: &str, _duration: Duration) {}
    async fn on_session_start(&self, _session_id: &str, _channel: &str) {}
    async fn on_session_end(&self, _session_id: &str) {}
}

/// Registry that runs hooks in priority order.
pub struct HookRegistry {
    hooks: Vec<Box<dyn HookHandler>>,
}

impl HookRegistry {
    pub fn new() -> Self { Self { hooks: vec![] } }
    pub fn register(&mut self, h: Box<dyn HookHandler>) {
        self.hooks.push(h);
        self.hooks.sort_by_key(|h| h.priority());
    }
    pub async fn run_before_tool_call(&self, name: String, args: Value) -> Option<(String, Value)> {
        let mut current = (name, args);
        for hook in &self.hooks {
            match hook.before_tool_call(current.0.clone(), current.1.clone()).await {
                HookResult::Continue(next) => current = next,
                HookResult::Cancel(reason) => {
                    tracing::info!("Hook {} cancelled tool call: {}", hook.name(), reason);
                    return None;
                }
            }
        }
        Some(current)
    }
}
```

### `crates/tandem-core/src/lib.rs`

Add `pub mod hooks;` and re-export `HookHandler`, `HookRegistry`, `HookResult`.

### `crates/tandem-server/src/lib.rs`

Add `pub hook_registry: Arc<RwLock<HookRegistry>>` to `AppState`. Initialize with `HookRegistry::new()`. Built-in hooks (e.g., rate limiting, audit logging) can be registered at startup.

---

## Implementation Priority Order

| # | Task | Primary file(s) | Complexity |
|---|---|---|---|
| 1 | Switch dispatcher to `prompt_async` | `dispatcher.rs` | Medium |
| 2 | SSE result streaming in dispatcher | `dispatcher.rs` | Medium |
| 3 | Add `/approve`, `/deny` slash commands | `dispatcher.rs` | Low |
| 4 | Route permission events back to channel | `dispatcher.rs` | Low |
| 5 | Add `ChannelToolPolicy` to config | `config.rs` | Low |
| 6 | Parse new autonomy env vars | `config.rs` | Low |
| 7 | Extend `SessionMap` → `SessionRecord` | `dispatcher.rs` | Low |
| 8 | Create `response_cache.rs` in tandem-memory | `tandem-memory/src/response_cache.rs` (new) | Medium |
| 9 | Wire cache into engine loop | `tandem-core/src/engine_loop.rs` | Medium |
| 10 | Add `prune_old_session_chunks` to MemoryDatabase | `tandem-memory/src/db.rs` | Low |
| 11 | Add hygiene background task to server | `tandem-server/src/http.rs` | Low |
| 12 | Create `hooks.rs` and `HookRegistry` | `tandem-core/src/hooks.rs` (new) | Medium |
| 13 | Wire `HookRegistry` into `AppState` | `tandem-server/src/lib.rs` | Low |
