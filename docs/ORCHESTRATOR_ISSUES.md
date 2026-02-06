# Orchestrator: Current Issues

- Missing Resume/Continue after reload
  - Impact: Users cannot resume pending tasks; only Pause/Cancel are visible after reload.
  - Notes: Resume does not set run status immediately; UI may remain “failed” or “paused”.
  - Code: [OrchestratorPanel.tsx](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx#L514-L534), [engine.rs:resume](file:///c:/Users/evang/work/tandem/src-tauri/src/orchestrator/engine.rs#L806-L809), [commands.rs:orchestrator_resume](file:///c:/Users/evang/work/tandem/src-tauri/src/commands.rs#L3814-L3835)

- Control buttons below task board
  - Impact: Users must scroll to access Pause/Resume/Cancel; poor ergonomics.
  - Code: [OrchestratorPanel.tsx TaskBoard](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx#L497-L512), [Control Buttons](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx#L514-L534)

- Status inconsistency on resume
  - Impact: UI chip shows “Execution Failed” while execution loop is running again.
  - Cause: Resume clears pause flag but does not set status to Executing; execute is spawned async.
  - Code: [engine.rs:resume](file:///c:/Users/evang/work/tandem/src-tauri/src/orchestrator/engine.rs#L806-L809), [commands.rs resume spawns execute](file:///c:/Users/evang/work/tandem/src-tauri/src/commands.rs#L3814-L3835), [Status chip](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx#L325-L334)

- “Start New Run” proximity and behavior
  - Impact: Accidental click wipes current orchestration context; no confirmation flow.
  - Code: [Start New Run button](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx#L556-L559), [handleNewRun](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx#L281-L289)

- Ambiguity between Restart vs Resume vs Continue
  - Impact: Users want to continue pending tasks without restarting from scratch.
  - Code: [Restart Execution](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx#L553-L555), [orchestrator_restart_run](file:///c:/Users/evang/work/tandem/src-tauri/src/commands.rs#L4016-L4043), [orchestrator_resume](file:///c:/Users/evang/work/tandem/src-tauri/src/commands.rs#L3814-L3835)
