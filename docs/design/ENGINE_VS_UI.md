# Engine vs UI Boundary

## Summary

Tandem's mission/orchestrator platform is engine-first. The engine owns durable state, execution, enforcement, and streams. Desktop/TUI own presentation and intervention UX.

## Engine MUST Own

- Durable state: missions, work items, approvals, shared resources.
- Runtime: orchestrator reducer loop, run dispatch, tool execution integration.
- Enforcement: capability tokens, policy checks, sandbox boundaries.
- Streams: typed `EngineEvent` over `/event` SSE.
- Extensibility: providers, skills, MCP routing.
- Auditability: immutable event trail for read/write/promote/approve actions.

## Desktop/TUI MUST Own

- Command-center UX, board views, timeline views, filters, and hotkeys.
- Approval controls and intervention controls.
- Local device integrations (notifications, clipboard, file pickers).

## Litmus Tests

- If two clients need identical behavior, it belongs in the engine.
- If behavior must survive restart, it belongs in engine state/event log.
- If behavior is visual-only and replaceable per client, it belongs in UI.
- If a rule impacts safety or data leakage, it belongs in engine enforcement.
