# Multi-Agent Orchestrator Brief (Archive)

## Note

This document is retained as historical context from the original ideation phase.
The active source of truth for implementation is under `docs/design/`.

## Active Specs

- `docs/design/ENGINE_VS_UI.md`
- `docs/design/ORCHESTRATOR.md`
- `docs/design/DEFAULT_MISSION_FLOW.md`
- `docs/design/SHARED_RESOURCES.md`
- `docs/design/MEMORY_TIERS.md`
- `docs/design/ROUTINES_CRON.md`
- `docs/design/IMPLEMENTATION_PLAN.md`
- `docs/design/WORKBOARD.md`
- `docs/design/PROGRESS_LOG.md`
- `docs/design/DECISIONS.md`

## Naming

Use `orchestrator` as the platform term.
The phrase "super orchestrator" was informal shorthand and is not used in canonical docs.

## Implemented Baseline (See Workboard for Verification)

- Shared resources APIs + eventing.
- Mission reducer crate + runtime APIs.
- Reviewer/tester mission gates.
- Memory tier governance (`session/project/team/curated`) with promotion + audit.
- Routine scheduler + routine APIs + policy gates.
- Desktop/TUI mission and routine parity wiring.

## Future/Incomplete Ideas

Forward-looking concepts that are not yet committed to the active build queue live in:

- `docs/design/ORCHESTRATOR_ROADMAP.md`

Any roadmap item must receive a `W-###` in `docs/design/WORKBOARD.md` before execution.
