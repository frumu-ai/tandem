---
title: CLI Vision
---

Tandem CLI is a binary-first, local-first workflow for users who want terminal speed and scriptability.

## Current State

- `tandem-engine` provides HTTP/SSE runtime plus `run`, `parallel`, `tool`, `providers`, and token utilities.
- `tandem-tui` provides interactive multi-agent terminal UX on top of the same engine runtime.
- Shared defaults are tuned for local use (`127.0.0.1:39731`) with optional API token hardening.

## Near-Term Direction

1. Keep engine commands stable and script-friendly.
2. Keep TUI keyboard-first and parity-aligned with desktop runtime contracts.
3. Expand practical examples for CI jobs, local automation, and ops-safe deployment patterns.

## Boundaries

- CLI docs should not assume users compile from source.
- Source-build workflows remain available, but binary install stays first-class.
- Advanced protocol and implementation detail belong in deep references under `docs/`.

## Related Deep Docs

- `docs/ENGINE_CLI.md`
- `docs/ENGINE_COMMUNICATION.md`
- `docs/ENGINE_PROTOCOL_MATRIX.md`
