# tandem-channels v0.3.8 — Kanban Board

## 🔵 In Progress

_(nothing — Phase 1 complete, Phase 2 queued)_

---

## ✅ Done

| Task | Phase |
|------|-------|
| Write `CHANNELS_INTEGRATION_PLAN.md` | Planning |
| Write `HEADLESS_SERVER_PLAN.md` | Planning |
| Add UI Integration section to plan | Planning |
| Add in-channel slash commands section | Planning |
| `crates/tandem-channels/Cargo.toml` | Phase 1 |
| `src/lib.rs` (crate root + re-exports) | Phase 1 |
| `src/traits.rs` — `Channel` trait, `ChannelMessage`, `SendMessage` | Phase 1 |
| `src/config.rs` — `ChannelsConfig` + env-var loading + unit tests | Phase 1 |
| `src/telegram.rs` — full long-poll adapter, typing, 4096-char chunking | Phase 1 |
| `src/discord.rs` — stub | Phase 1 |
| `src/slack.rs` — stub | Phase 1 |
| `src/dispatcher.rs` — supervisor, session mapping, slash commands | Phase 1 |
| Add `tandem-channels` to workspace `Cargo.toml` | Phase 1 |
| `cargo check -p tandem-channels` ✅ | Phase 1 |
| `src/discord.rs` — full WebSocket gateway, Identify, heartbeat, reconnect (op 7/9), typing | Phase 2 |
| `src/discord.rs` — 2000-char chunking, mention normalization, allowlist, bot self-filter | Phase 2 |
| `src/slack.rs` — `conversations.history` poll, `last_ts` dedup, `auth.test` self-filter, `chat.postMessage` | Phase 2 |
| `cargo test -p tandem-channels` ✅ all passed (Phase 2 additions) | Phase 2 |


---

## 📋 To Do

### Phase 3 — Session Dispatcher Improvements
- [ ] SSE streaming for `run_in_session` (replace polling)
- [ ] Session map persistence to JSON file
- [ ] `/sessions` shows message count + last activity

### Phase 4 — Upgrade `run_in_session` to use real Tandem API path
- [ ] Map to correct `POST /sessions/{id}/messages` + `POST /sessions/{id}/run` endpoints
- [ ] Verify field names (`runID`, `assistantText`) match `tandem-server` schema

### Phase 5 — `tandem-server` Integration
- [ ] Add optional `channels` feature to `tandem-server/Cargo.toml`
- [ ] Hook `start_channel_listeners` into `serve()` in `http.rs`
- [ ] `GET /channels/status` endpoint
- [ ] `PUT /channels/{name}` endpoint (enable/configure)
- [ ] `DELETE /channels/{name}` endpoint (disable)

### Phase 6 — Verification
- [ ] Manual Telegram end-to-end test
- [ ] Manual Discord end-to-end test (Phase 2)
