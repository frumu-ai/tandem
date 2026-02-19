# tandem-channels v0.3.8 — Kanban Board

## 🔵 In Progress

_(nothing — all phases complete)_

---

## ✅ Done

| Task                                                                                                        | Phase     |
| ----------------------------------------------------------------------------------------------------------- | --------- |
| Write `CHANNELS_INTEGRATION_PLAN.md`                                                                        | Planning  |
| Write `HEADLESS_SERVER_PLAN.md`                                                                             | Planning  |
| Add UI Integration section to plan                                                                          | Planning  |
| Add in-channel slash commands section                                                                       | Planning  |
| `crates/tandem-channels/Cargo.toml`                                                                         | Phase 1   |
| `src/lib.rs` (crate root + re-exports)                                                                      | Phase 1   |
| `src/traits.rs` — `Channel` trait, `ChannelMessage`, `SendMessage`                                          | Phase 1   |
| `src/config.rs` — `ChannelsConfig` + env-var loading + unit tests                                           | Phase 1   |
| `src/telegram.rs` — full long-poll adapter, typing, 4096-char chunking                                      | Phase 1   |
| `src/discord.rs` — stub                                                                                     | Phase 1   |
| `src/slack.rs` — stub                                                                                       | Phase 1   |
| `src/dispatcher.rs` — supervisor, session mapping, slash commands                                           | Phase 1   |
| Add `tandem-channels` to workspace `Cargo.toml`                                                             | Phase 1   |
| `cargo check -p tandem-channels` ✅                                                                         | Phase 1   |
| `src/discord.rs` — full WebSocket gateway, Identify, heartbeat, reconnect (op 7/9), typing                  | Phase 2   |
| `src/discord.rs` — 2000-char chunking, mention normalization, allowlist, bot self-filter                    | Phase 2   |
| `src/slack.rs` — `conversations.history` poll, `last_ts` dedup, `auth.test` self-filter, `chat.postMessage` | Phase 2   |
| `cargo test -p tandem-channels` ✅ all passed (Phase 2 additions)                                           | Phase 2   |
| Correct API paths: singular `/session` (not `/sessions/`)                                                   | Phase 3+4 |
| `prompt_sync` with `{parts:[{type:text,text}]}` body (real `SendMessageRequest`)                            | Phase 3+4 |
| `WireSessionMessage` parsing — `info.role=="assistant"`, `parts[].type=="text"`                             | Phase 3+4 |
| JSON session map persistence to `channel_sessions.json` on every write                                      | Phase 3+4 |
| Load persisted map on startup in `start_channel_listeners`                                                  | Phase 3+4 |
| Full slash commands: `/new`, `/sessions`, `/resume`, `/rename`, `/status`, `/help`                          | Phase 3+4 |
| `PATCH /session/{id}` for rename, `GET /session/{id}` for status                                            | Phase 3+4 |
| `use tandem_channels::start_channel_listeners` in `tandem-server/src/http.rs`                               | Phase 5   |
| `serve()` loads `ChannelsConfig::from_env()` — starts or skips gracefully                                   | Phase 5   |
| `serve()` aborts channel `JoinSet` on graceful shutdown                                                     | Phase 5   |
| `cargo check -p tandem-server` ✅                                                                           | Phase 5   |
| `cargo check -p tandem-channels -p tandem-server` ✅                                                        | Phase 6   |
| `cargo test -p tandem-channels` ✅ **50/50 tests pass**                                                     | Phase 6   |

---

## 📋 To Do

### Phase 6 (remaining) — Manual End-to-End Tests

- [ ] Manual Telegram end-to-end test (live bot token)
- [ ] Manual Discord end-to-end test (live bot token)
- [ ] Manual Slack end-to-end test (live bot token)
