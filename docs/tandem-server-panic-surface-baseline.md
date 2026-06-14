# Tandem Server Panic Surface Baseline

TAN-200 records the production panic-surface baseline for `tandem-server` and
the first enforced hot spots.

Generated on 2026-06-14 with:

```bash
node scripts/check-rust-panic-surface.mjs
node scripts/check-rust-panic-surface.mjs --all-server --max-per-file=9999
```

## Enforced Hot Spots

The CI gate enforces zero production findings in:

| File                                                         | Production findings |
| ------------------------------------------------------------ | ------------------: |
| `crates/tandem-server/src/app/state/approval_message_map.rs` |                   0 |
| `crates/tandem-server/src/pack_manager.rs`                   |                   0 |
| `crates/tandem-server/src/bug_monitor/log_watcher.rs`        |                   0 |

These modules also deny `clippy::unwrap_used`, `clippy::expect_used`, and
`clippy::panic` outside test builds. CI runs `cargo clippy -p tandem-server
--lib --no-deps -- -A warnings` so those file-level denies fail the build
without turning the existing server-wide warning backlog into a hard gate.

## Server-Wide Follow-Up Baseline

The full non-test `crates/tandem-server/src` scan currently reports 42
production panic-surface findings outside the enforced TAN-200 hot spots.

The former crate-wide `#![allow(warnings)]` blanket has been narrowed to
`#![allow(unused)]` while the remaining lint fallout is paid down in follow-up
work. New panic-surface cleanup should reduce the server-wide count and then add
the cleaned module to the enforced target set in
`scripts/check-rust-panic-surface.mjs`.
