# Default Mission Flow

## Summary

Default flow is configurable but starts with:
`Pal + Nerd -> Board -> Specialists -> Reviewer -> Tester`.

## Planning Stage

- Pal clarifies intent, done criteria, risks, and milestones.
- Nerd maps files/constraints and decomposes into structured `WorkItem`s.
- Planning output must be strict JSON compatible with work item schema.

## Execution Stage

- Assign work items by skill tags/capabilities.
- Allow bounded parallel execution.
- Each work item binds to a `run_id` and emits artifacts.

## Gates

- Reviewer gate before apply/promotion on sensitive outputs.
- Tester gate before final completion.
- On gate fail, emit revision event and return item to pending/rework.

## Completion

- Mission completes only when all required items pass reviewer/tester gates.
- Summary artifact + audit events are persisted.
