# Orchestrator Roadmap (Post-Phase-6)

## Purpose

This file tracks forward-looking orchestrator/control-center ideas that are not yet committed as active implementation work.

Authoritative implementation scope remains in:

- `README.md`
- `IMPLEMENTATION_PLAN.md`
- `WORKBOARD.md`
- `PROGRESS_LOG.md`

## Status Labels

- `implemented`: shipped and verified
- `planned`: accepted direction but not fully delivered
- `exploratory`: idea only, not committed to build queue

## Roadmap Items

### Team Composition UX

Status: `planned`

- Persistent team configurations and member-role templates.
- Mission kickoff from team templates in Desktop and TUI.
- Clear lead/worker assignment visibility in command-center views.

### Control Center Views

Status: `planned`

- Unified mission timeline + board + event stream filters.
- Intervention controls (pause/resume/override) with policy-aware affordances.
- Approval center unifying reviewer/tester/routine approvals.

### Operator Ergonomics

Status: `planned`

- More quick-action commands for mission lifecycle steps.
- Readable mission/routine summaries optimized for incident response.
- Better "what changed" diff surfaces for approval decisions.

### SDK/Builder Experience

Status: `planned`

- Stronger public API examples for mission/resource/routine workflows.
- Contract snapshots for key event families used by external clients.
- Versioning policy for API/event compatibility guarantees.

### Advanced Team Automation

Status: `exploratory`

- Agent-authored team/routine drafts with strict user activation gates.
- Curated orchestration playbooks installable as templates.
- Connector-backed routine bundles with explicit side-effect policies.

## Promotion Rule

Any item here must be assigned a `W-###` in `WORKBOARD.md` before implementation starts.
