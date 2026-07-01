# Runtime Architecture Audit

## TAN-200 panic-surface baseline

Recorded: 2026-06-13

The first TAN-200 guard focuses on the production hot files called out in Linear:

| File                                                         | Production `.unwrap()` / `.expect()` / `panic!` count |
| ------------------------------------------------------------ | ----------------------------------------------------: |
| `crates/tandem-server/src/app/state/approval_message_map.rs` |                                                     0 |
| `crates/tandem-server/src/pack_manager.rs`                   |                                                     0 |
| `crates/tandem-server/src/incident_monitor/log_watcher.rs`   |                                                     0 |

The same scanner reports 32 production panic-surface findings across
`crates/tandem-server/src` when run with `--all-server --max-per-file=999999`.
Those wider findings are baseline data for follow-up cleanup, not part of the
initial CI gate.

The guard strips `#[cfg(test)]` blocks before counting, so tests can keep using
assertive unwraps while production code cannot reintroduce panicking calls in
these modules.

Run locally:

```sh
node scripts/check-rust-panic-surface.mjs --self-test
node scripts/check-rust-panic-surface.mjs
```

For a wider report without changing the CI gate:

```sh
node scripts/check-rust-panic-surface.mjs --all-server --max-per-file=999999
```

The crate-level `#![allow(warnings)]` in `tandem-server` remains a broader
follow-up because removing it currently affects unrelated modules outside the
three TAN-200 hotspot files.
