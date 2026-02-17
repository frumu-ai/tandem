---
title: SDK Vision
---

Tandem SDK direction is to provide stable, client-consumable contracts on top of today’s HTTP + SSE runtime.

## Current Reality

- Engine integrations are contract-driven via session/message/run endpoints and event streaming.
- Desktop and TUI are reference clients that exercise these contracts daily.
- Strict contract handling exists for critical orchestrator paths where structured output is required.

## SDK Goals

1. Preserve session-linear execution semantics as the core runtime model.
2. Stabilize wire contracts before adding new adapter surfaces.
3. Keep reconnect/recovery and conflict handling explicit and testable.

## Scope Boundaries

- HTTP + SSE contracts are current and canonical.
- Any future alternate transport (for example stdio/JSON-RPC adapters) must map to the same run/session semantics.
- SDK docs must separate normative contract from vision text.

## Related Deep Docs

- `docs/TANDEM_SDK_VISION.md`
- `docs/ENGINE_PROTOCOL_MATRIX.md`
- `docs/ENGINE_COMMUNICATION.md`
