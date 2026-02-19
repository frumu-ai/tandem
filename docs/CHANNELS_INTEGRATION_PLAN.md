# tandem-channels: External Messaging Integration

Allow users to chat with Tandem agents directly from Telegram, Discord, or Slack. Incoming messages map to Tandem sessions; responses are delivered back to the sender's chat.

## Proposed Changes

---

### New Crate — `tandem-channels`

#### `crates/tandem-channels/Cargo.toml` [NEW]

New crate with these dependencies:
- `anyhow`, `tokio = { features = ["full"] }`, `tracing`
- `reqwest = { version = "0.12", features = ["json"] }` (already in workspace)
- `serde`, `serde_json`, `uuid = { features = ["v4"] }`
- `async-trait = "0.1"` (needed for `dyn Channel`)
- `tokio-tungstenite = "0.24"`, `futures-util = "0.3"` (Discord WebSocket)
- `parking_lot = "0.12"` (typing handle mutex)

#### `src/traits.rs` [NEW]

Port directly from Zeroclaw `src/channels/traits.rs`. Defines:
```rust
pub struct ChannelMessage { id, sender, reply_target, content, channel, timestamp }
pub struct SendMessage { content, recipient }

#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    async fn send(&self, message: &SendMessage) -> anyhow::Result<()>;
    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()>;
    async fn health_check(&self) -> bool { true }
    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> { Ok(()) }
    async fn stop_typing(&self, recipient: &str) -> anyhow::Result<()> { Ok(()) }
    fn supports_draft_updates(&self) -> bool { false }
}
```

#### `src/telegram.rs` [NEW]

Port from Zeroclaw `src/channels/telegram.rs`. Key pieces:
- `TelegramChannel { bot_token, allowed_users: Arc<RwLock<Vec<String>>>, client, typing_handle }`
- `listen()` — long-polls `getUpdates` with `timeout=25`, parses `ChannelMessage`
- `send()` — `sendMessage` with 4096-char chunking (`split_message_for_telegram`)
- `start_typing()` / `stop_typing()` — spawns/aborts a loop calling `sendChatAction`
- **Simplified**: no attachment markers, no draft streaming, no Zeroclaw-specific pairing (use `allowed_users` allowlist only)

#### `src/discord.rs` [NEW]

Port from Zeroclaw `src/channels/discord.rs`. Key pieces:
- `DiscordChannel { bot_token, guild_id, allowed_users, mention_only, typing_handle }`
- `listen()` — opens Discord Gateway WebSocket, sends Identify opcode, handles heartbeats, dispatches `MESSAGE_CREATE`
- `send()` — `POST /channels/{id}/messages` with 2000-char chunking
- `start_typing()` — loop calling `POST /channels/{id}/typing` every 8s

#### `src/slack.rs` [NEW]

Port from Zeroclaw `src/channels/slack.rs`:
- `SlackChannel { bot_token, channel_id, allowed_users }`
- `listen()` — polls `conversations.history` every 3s, tracks `last_ts` for deduplication
- `send()` — `chat.postMessage`

#### `src/dispatcher.rs` [NEW]

Core glue between channel messages and Tandem sessions:

```rust
/// Spawns a supervised listener for one channel with exponential backoff.
pub async fn spawn_supervised_listener(
    channel: Arc<dyn Channel>,
    base_url: String,   // e.g. "http://127.0.0.1:3000"
    api_token: String,
    session_map: SessionMap,  // Arc<Mutex<HashMap<String, String>>>
)

/// Called for each incoming ChannelMessage.
async fn process_channel_message(
    msg: ChannelMessage,
    channel: Arc<dyn Channel>,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
)
```

**Session mapping**: each `{channel_name}:{sender_id}` pair maps to a Tandem `session_id`. First message creates the session via `POST /sessions`; subsequent messages reuse it (persistent conversation per user per channel).

**Message flow**:
1. `channel.start_typing(reply_target)`
2. `POST {base_url}/sessions/{session_id}/run` with `{ "message": msg.content }`
3. Stream `GET /sessions/{id}/events` (existing SSE endpoint) until `session.run.finished`
4. Collect final assistant text
5. `channel.stop_typing(reply_target)`
6. `channel.send(SendMessage { content: response_text, recipient: reply_target })`

#### `src/config.rs` [NEW]

```rust
pub struct ChannelsConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    pub server_base_url: String,  // default: "http://127.0.0.1:3000"
    pub api_token: String,
}
```

Loaded from environment variables at startup:
| Env var | Purpose |
|---------|---------|
| `TANDEM_TELEGRAM_BOT_TOKEN` | Bot token from @BotFather |
| `TANDEM_TELEGRAM_ALLOWED_USERS` | Comma-separated usernames or IDs (`*` = anyone) |
| `TANDEM_DISCORD_BOT_TOKEN` | Discord bot token |
| `TANDEM_DISCORD_GUILD_ID` | Optional guild filter |
| `TANDEM_DISCORD_ALLOWED_USERS` | Comma-separated Discord user IDs |
| `TANDEM_SLACK_BOT_TOKEN` | Slack bot token (`xoxb-...`) |
| `TANDEM_SLACK_CHANNEL_ID` | Slack channel to poll |
| `TANDEM_SLACK_ALLOWED_USERS` | Comma-separated Slack user IDs |
| `TANDEM_API_TOKEN` | Already exists — reused for internal auth |
| `TANDEM_SERVER_URL` | Defaults to `http://127.0.0.1:3000` |

#### `src/lib.rs` [NEW]

```rust
pub use dispatcher::start_channel_listeners;
```

`start_channel_listeners(config)` — reads config, builds enabled channels, spawns one supervised listener per channel, returns a `JoinSet<()>`.

---

### Modified — `tandem-server`

#### `crates/tandem-server/Cargo.toml` [MODIFY]

```toml
tandem-channels = { path = "../tandem-channels", version = "0.3.7", optional = true }

[features]
channels = ["tandem-channels"]
```

Keeping channels opt-in via a Cargo feature keeps the binary lean when not configured.

#### `crates/tandem-server/src/http.rs` [MODIFY]

In `serve()`, after the existing `routine_scheduler` spawn (~line 435), add:

```rust
#[cfg(feature = "channels")]
let channel_listeners = {
    if let Ok(cfg) = tandem_channels::config::ChannelsConfig::from_env() {
        tracing::info!("Starting channel listeners...");
        Some(tokio::spawn(tandem_channels::start_channel_listeners(cfg)))
    } else {
        None
    }
};
```

On shutdown, abort alongside the existing `reaper.abort()` calls.

---

### Modified — Workspace `Cargo.toml`

#### `Cargo.toml` [MODIFY]

Add `"crates/tandem-channels"` to `[workspace] members`.

---

---

## UI Integration

### Desktop GUI — "Connections" Settings Tab

The existing `Settings.tsx` uses a two-tab pill switcher (`settings` | `logs`). We extend this to a three-tab bar:

```
[ ⚙ Settings ]  [ 🔗 Connections ]  [ 📜 Logs ]
```

#### `src/components/settings/ConnectionsSettings.tsx` [NEW]

A new panel following the `Card / CardHeader / CardContent` design system already used by `ProviderCard`, `MemoryStats`, etc.

**Layout — one card per channel platform:**

```
┌─────────────────────────────────────────────────────┐
│ 🟢 Telegram                              [Connected] │
│  Bot: @mytandembot                                   │
│  Allowed users: @bob, *                             │
│  Sessions active: 3          [Configure]  [Disable]  │
└─────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────┐
│ ⚫ Discord                             [Not set up]  │
│  [Bot token ________________]                        │
│  [Guild ID    ________________]    [Enable]          │
└─────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────┐
│ ⚫ Slack                               [Not set up]  │
│  [Bot token ________________]                        │
│  [Channel ID  ________________]    [Enable]          │
└─────────────────────────────────────────────────────┘
```

**Per-channel card states:**
- **Not configured** — token input fields + Enable button
- **Connected** — green indicator, bot username, active session count, Configure / Disable actions
- **Error** — red indicator with last error message and Retry button

**Data flow:**

```
ConnectionsSettings.tsx
  → useConnections() hook
    → GET /channels/status  (new engine endpoint — returns per-channel health + session count)
  → save handlers
    → invoke("set_channel_config", { channel: "telegram", config: {...} })
      → Tauri command → writes to config file / env / keystore
      → signals tandem-server to reload channel listeners
```

Channel credentials (bot tokens) are stored via the existing **vault/keystore** Tauri command (`storeApiKey`) so they are never written to plain text files.

#### `src/components/settings/Settings.tsx` [MODIFY]

Extend `activeTab` type and the tab bar:

```tsx
// Before:
const [activeTab, setActiveTab] = useState<"settings" | "logs">("settings");

// After:
const [activeTab, setActiveTab] = useState<"settings" | "connections" | "logs">("settings");
```

Add a third pill button (with `Link` icon from lucide-react) between Settings and Logs.

Render `<ConnectionsSettings />` when `activeTab === "connections"`.

---

### TUI — Connections Panel

The TUI's `ui/mod.rs` (ratatui-based) should gain a **Connections** view accessible from the navigation. The simplest integration is a new entry in the left nav alongside Sessions, Projects, Memory, etc.

#### Layout

```
╔═ Connections ══════════════════════════════════════╗
║                                                    ║
║  [●] Telegram    @mytandembot    3 active sessions ║
║  [○] Discord     not configured                    ║
║  [○] Slack       not configured                    ║
║                                                    ║
║  Press [e] to configure  [d] to disable  [?] help  ║
╚════════════════════════════════════════════════════╝
```

Inline configuration uses a modal overlay (same pattern as the existing wizard dialogs in the TUI) to enter bot tokens, allowed users, etc. — saved via the engine API.

---

### How Connections Trigger Sessions

When an external message arrives the dispatcher follows this flow:

```
Incoming message (Telegram / Discord / Slack)
         │
         ▼
  Look up session_map["{channel}:{sender_id}"]
         │
    ┌────┴────┐
    │ Exists? │
    └────┬────┘
    No   │   Yes
    ▼    │    ▼
  POST   │  Use existing session_id
  /sessions  │
  → new      │
  session_id │
         └───┤
             ▼
     POST /sessions/{id}/run
      { "message": content }
             │
             ▼
       Stream SSE events
    until session.run.finished
             │
             ▼
      channel.send(response)
```

The session created by a channel connection is a **standard Tandem session** — it appears in the Sessions list in the GUI/TUI with a `source` tag (e.g. `telegram:@bob`) so the user can see and interact with it from the desktop too.

The session mapping (`{channel}:{sender_id}` → `session_id`) is persisted to a JSON file (e.g. `~/.tandem/channel_sessions.json`) so mappings survive server restarts.

---

### In-Channel Slash Commands

Users can control their Tandem sessions from within the chat client itself using slash commands. The dispatcher intercepts these before sending to the LLM.

#### Session Management Commands

| Command | Action |
|---------|--------|
| `/new` | Start a fresh session (old one is preserved but a new one becomes active) |
| `/new <name>` | Start a fresh named session |
| `/sessions` | List your recent sessions (shows last 5 with names/timestamps) |
| `/resume <id_or_name>` | Switch back to a previous session by ID prefix or name |
| `/rename <name>` | Rename the current session |
| `/status` | Show current session ID, name, message count, active project |
| `/help` | List available commands |

#### Example Interaction

```
You:    /sessions
Bot:    📋 Your sessions:
        1. abc123  "Fix auth bug"           2h ago  ← current
        2. def456  "Refactor API layer"     1d ago
        3. ghi789  "Untitled"               3d ago

You:    /resume def456
Bot:    ✅ Resumed session "Refactor API layer" (def456)
        Last message: "Can you show me the current router structure?"
        → Ready to continue.

You:    /new Homepage redesign
Bot:    ✅ Started new session "Homepage redesign" (jkl012)
        Fresh context — what would you like to work on?
```

#### Implementation

Command parsing lives in `dispatcher.rs` as a pre-processing step before `process_channel_message()`:

```rust
fn parse_slash_command(content: &str) -> Option<ChannelCommand> {
    match content.trim() {
        "/new" | c if c.starts_with("/new ") => Some(ChannelCommand::NewSession { ... }),
        "/sessions" => Some(ChannelCommand::ListSessions),
        c if c.starts_with("/resume ") => Some(ChannelCommand::ResumeSession { ... }),
        "/status" => Some(ChannelCommand::SessionStatus),
        "/help" => Some(ChannelCommand::Help),
        _ => None,
    }
}
```

Each command calls existing `tandem-server` HTTP endpoints:
- `POST /sessions` — new session
- `GET /sessions` — list sessions (filtered by `source` tag)
- `PATCH /sessions/{id}` — rename
- `GET /sessions/{id}` — status

The response is formatted as a short text message and sent back through the channel's `send()`.

---

## Open Questions

1. **Feature flag vs always-on**: channels opt-in via `--features channels` (lean binary) vs always compiled in but gracefully skipped when no env vars are set.

2. **Session mapping model**: one session per `{channel}:{sender_id}` (separate histories per user) vs one shared session per channel.

3. **Phased rollout**: implement all three adapters in one go vs Telegram-only first.

---

## Verification Plan

### Automated Tests

Port Zeroclaw's unit tests into `tandem-channels`:

```bash
cargo test -p tandem-channels split_message   # Telegram 4096-char & Discord 2000-char chunking
cargo test -p tandem-channels is_user_allowed # allowlist logic
cargo test -p tandem-channels bot_user_id     # Discord token parsing

cargo check -p tandem-channels
cargo check -p tandem-server --features channels
```

### Manual Verification — Telegram End-to-End

1. Get a bot token from [@BotFather](https://t.me/BotFather)
2. Set env vars:
   ```
   TANDEM_TELEGRAM_BOT_TOKEN=<token>
   TANDEM_TELEGRAM_ALLOWED_USERS=<your-username>
   TANDEM_API_TOKEN=<tandem-api-token>
   ```
3. Run: `cargo run -p tandem-server --features channels`
4. Send a message to the bot in Telegram → expect typing indicator → agent response
5. Send a follow-up → conversation should continue (same session, shared context)
