---
title: Design System
---

Tandem design principles prioritize clarity, trust, and actionable status visibility across Desktop and TUI experiences.

## Product UI Principles

1. Safety context should be visible at decision time (not hidden in logs).
2. Runtime status should be legible at a glance (connected, running, waiting, error).
3. Interaction patterns should stay consistent across Desktop and TUI where possible.

## Runtime UX Contracts

- Permission/approval surfaces should clearly state tool intent and scope.
- Long-running tasks should expose progress and recovery paths.
- Session/workspace boundaries should be explicit in navigation and commands.

## Implementation Layering

- Guide pages describe user-facing patterns and behaviors.
- Deep visual/style token details and implementation rules belong in `docs/` references.

## Related Deep Docs

- `docs/DESIGN_SYSTEM.md`
- `docs/PLANNING_MODES.md`
- `docs/IMPROVE_SESSION_FLOW.md`
