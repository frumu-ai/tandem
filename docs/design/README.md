# Design Control Plane

## Purpose

This folder is the single source of truth for planning and execution of the multi-agent/orchestrator build.

## Where to Track What

- Scope and architecture: `ENGINE_VS_UI.md`, `ORCHESTRATOR.md`, `DEFAULT_MISSION_FLOW.md`
- Subsystem specs: `SHARED_RESOURCES.md`, `MEMORY_TIERS.md`, `ROUTINES_CRON.md`
- Delivery sequencing: `IMPLEMENTATION_PLAN.md`
- Packaging strategy: `PACKAGING.md`
- Third-party contract matrix: `SDK_CONTRACTS.md`
- Future/non-committed ideas backlog: `ORCHESTRATOR_ROADMAP.md`
- Active work queue and status: `WORKBOARD.md`
- Ongoing execution progress: `PROGRESS_LOG.md`
- Decision history and rationale: `DECISIONS.md`
- Build/push coordination workflow: `EXECUTION_PLAYBOOK.md`

## Golden Rule

If a task is not in `WORKBOARD.md`, it does not exist for execution.

## IDs and Linking

- Every executable task has an ID (`W-###`) in `WORKBOARD.md`.
- Every progress update references one or more `W-###` IDs in `PROGRESS_LOG.md`.
- Every architectural choice references one or more `W-###` IDs in `DECISIONS.md`.
- Commits should include the active `W-###` in the commit message.
