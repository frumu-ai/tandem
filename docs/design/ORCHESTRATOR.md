# Orchestrator Architecture

## Summary

Orchestrator is modeled as deterministic `Event -> State -> Command`.
It does not execute tools directly; it emits commands executed by engine runtime adapters.

## Core Types

- `MissionSpec`: objective, success criteria, budgets, capabilities, entrypoint.
- `MissionState`: durable state for mission lifecycle and board progress.
- `WorkItem`: generic node with deps, assignee hints, and artifacts.
- `MissionEvent`: mission/run/tool/approval/timer/resource events.
- `MissionCommand`: start run, request approval, persist artifact, schedule timer, emit notice.

## Reducer Interface

```rust
trait MissionReducer {
    fn init(spec: MissionSpec) -> MissionState;
    fn on_event(state: &MissionState, event: MissionEvent) -> Vec<MissionCommand>;
}
```

## Execution Mapping

- `MissionCommand::StartRun` -> existing run/session APIs.
- `MissionCommand::RequestApproval` -> permission/question system.
- `MissionCommand::EmitNotice` -> `EngineEvent`.
- `MissionCommand::PersistArtifact` -> orchestrator artifact store.

## Compatibility Rules

- Keep `sessionID`/`runID` semantics in emitted events.
- Keep current orchestrator run/task persistence readable in `.tandem/orchestrator`.
