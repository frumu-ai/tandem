# Automation Planner Kanban

## Slice Goal
Refactor Tandem's engine planner so workflow creation and revision are LLM-first and no longer driven by hard-coded keyword heuristics, preset workflow shapes, or deterministic text parsers.

## Done
- [x] Audit the current planner functions and identify heuristic planning entrypoints
- [x] Make initial workflow creation LLM-first
- [x] Make workflow revision LLM-first
- [x] Add a dedicated workflow-creation planner prompt
- [x] Tighten the workflow-revision planner prompt around allowed step ids and plan invariants
- [x] Add a shared LLM invocation seam for planner build and revision
- [x] Add a shared normalize-and-validate path for LLM-returned plans
- [x] Keep compile-to-`AutomationV2Spec` deterministic
- [x] Shrink deterministic fallback to:
  - generic single-step initial fallback plan
  - keep-current-plan revision fallback with clarifier
- [x] Remove heuristic planning from the active create path
- [x] Remove heuristic planning from the active revision path
- [x] Stop using hard-coded schedule inference from prompt text
- [x] Stop using hard-coded revision text extractors for:
  - title
  - workspace root
  - schedule
  - model provider / model id
  - output contract
  - MCP changes
  - workflow-shape switching
  - step insertion/removal
- [x] Preserve deterministic validation and normalization for:
  - workspace root
  - provider existence
  - operator preferences
  - allowed step ids
  - dependency/input-ref integrity
  - JSON extraction
- [x] Add backend tests for:
  - valid LLM-created plan accepted
  - invalid step id rejected
  - invalid dependency rejected
  - provider unavailable fallback behavior
  - no planner model fallback behavior
  - LLM revision clarify
  - LLM revision keep
  - invalid LLM revision fallback
  - apply still persists compiled V2 automation metadata
- [x] Move planner create/revise invocation off the generic engine tool path
- [x] Make planner provider calls tool-free (`ToolMode::None`, no tool schemas)
- [x] Add compact backend-generated MCP capability summaries to planner create/revise prompts
- [x] Preserve planner fallback diagnostics (`fallback_reason`, `detail`) in draft state and API responses
- [x] Surface planner fallback reason in the control-panel review step
- [x] Sanitize Gmail delivery tool args so empty/null attachment `s3key` values are omitted
- [x] Tighten email delivery prompt guidance so `notify_user` defaults to inline-body delivery unless a real attachment artifact exists
- [x] Surface workflow run node-output blockers in the run debugger for delivery failures and tool errors
- [x] Expand run debugger plumbing for workflow runs:
  - use `automationsV2.getRun(...)`
  - include workflow node-output session IDs
  - show started/finished timestamps and objective previews in run lists
- [x] Add tandem-core tests for Gmail attachment sanitization

## In Progress
- [ ] None

## Remaining Follow-Up
- [ ] Add broader provider-backed planner integration coverage beyond the current env-override test seam
- [ ] Decide whether initial preview should require planner configuration in more environments instead of silently returning the generic fallback
- [ ] Expand the allowed fixed step catalog if the engine needs richer normalized action vocabulary
- [ ] Surface initial planner clarifiers more explicitly in the control panel review/create flow
- [ ] Add richer delivery-status presentation for workflow terminal steps beyond the current blocker extraction

## Explicitly Out Of Scope
- [ ] Dynamic replanning during runtime execution
- [ ] Arbitrary custom step ids without engine validation support
- [ ] Moving compile/runtime logic into the LLM
- [ ] Replacing structural validation with heuristic trust in model output

## Risks
- The planner is now correctly LLM-first, but the current initial fallback still returns a generic single-step draft when the planner model is unavailable. That is safe, but can hide missing planner configuration if the UI does not surface it clearly enough.
- The planner still relies on a fixed allowed step-id catalog. That is intentional for structural safety, but it limits vocabulary breadth until the engine expands that catalog.
- Delivery can still fail for runtime/auth reasons even with valid tool exposure. The current UI now surfaces blocked node output, but status presentation is still not as explicit as it should be.

## Verification
- [x] `cargo test -p tandem-server workflow_plan_ -- --test-threads=1`
- [x] `cargo test -p tandem-core normalize_tool_args_gmail_send_email_ -- --test-threads=1`

## Notes
- Planner intelligence now lives in the model.
- Rust owns validation, normalization, persistence, provider checks, and deterministic compilation.
- The old heuristic functions are no longer used as the primary planner.
- Planner provider calls are now planner-specific and tool-free; execution-time MCP/tool schemas are no longer sent during planning.
