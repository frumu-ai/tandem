# Automation Planner Kanban

## Slice Goal
Ship the smallest correct engine-first automation creation path:

`prompt -> workflow-plans preview/apply -> AutomationV2Spec -> runtime execution`

This board is a burn-down board for the current slice only.
It is not a wishlist for future planner features.

## Ship Blockers
- [x] Run a targeted backend verification pass after clearing unrelated test compile failures
- [x] Fix regressions found by that verification pass
- [x] Explicitly flag the current deterministic planner-chat revision surface as limited in the UI

## In Progress
- [x] Keep the kanban scoped to ship-blocking work only
- [x] Current slice has no remaining ship blockers

## Done In This Slice
- [x] Add canonical backend planner endpoints:
  - `POST /workflow-plans/preview`
  - `POST /workflow-plans/apply`
- [x] Compile planner output into `AutomationV2Spec`
- [x] Persist planner metadata on created V2 automations
- [x] Add additive V2 dataflow fields:
  - `input_refs`
  - `output_contract`
- [x] Persist canonical node outputs for downstream consumption
- [x] Inject deterministic upstream outputs into downstream node execution
- [x] Keep legacy `/automations` compatible during migration
- [x] Move the Automations page off legacy `/automations` creation and onto planner preview/apply
- [x] Seed the planner path from chat/setup interception
- [x] Make `workspace_root` a first-class planner/apply/runtime field
- [x] Validate `workspace_root` on planner preview/apply and direct `/automations/v2` create/patch
- [x] Fail V2 runs clearly when an explicit `workspace_root` does not exist
- [x] Fail V2 runs clearly when an explicit `workspace_root` is not a directory
- [x] Unblock targeted backend verification by fixing unrelated `coder.rs` test compile regressions
- [x] Carry allowed MCP server constraints through planner/apply into agent MCP policy
- [x] Add in-memory workflow plan drafts and planning conversations
- [x] Add planning draft routes:
  - `POST /workflow-plans/chat/start`
  - `POST /workflow-plans/chat/message`
  - `POST /workflow-plans/chat/reset`
  - `GET /workflow-plans/:plan_id`
- [x] Add planning chat UI to the Automations review step
- [x] Explicitly label the planning chat surface as limited in this slice
- [x] Surface planner change summaries and clarifier feedback in the review UI
- [x] Return explicit supported-edit guidance when planner-chat receives an unsupported revision note
- [x] Preserve advanced create settings as backend-owned `operator_preferences`
- [x] Compile execution-mode and model preferences into V2 execution/model policy
- [x] Keep the review summary aligned with the latest revised workflow plan
- [x] Remove pack-builder-first wording from the default automation creation flow
- [x] Clarify the optional scaffold section as a reusable-skill export path, not the default flow
- [x] Allow apply to optionally export a reusable Pack Builder draft after planning
- [x] Show explicit cleared states in review:
  - `Workspace default` for cleared model override
  - `None` for cleared MCP constraints
- [x] Show the latest planned step list in review
- [x] Add explicit planner-model controls in the Automations wizard so broader planner fallback can be enabled without editing raw role-model JSON
- [x] Require planner-model fallback to be fully specified:
  - block half-filled planner-model config in the wizard
  - strip incomplete `role_models.planner` values in backend normalization
- [x] Add planner-chat deterministic revisions for:
  - schedule updates
  - switching back to manual execution
  - title updates
  - workspace root updates
  - safe workflow-shape switching
    Current shapes: single-step, compare/report, research/report, notification
  - analysis-step add/remove
  - MCP add/remove/clear
  - MCP `only` narrowing semantics
  - notification-step add/remove
  - execution mode / max parallel updates
  - model override set/clear
  - planner model override set/clear
- [x] Bound planner LLM fallback with a backend timeout and return a specific clarifier when broader planner revision cannot produce a valid workflow update
- [x] Fast-fail planner LLM fallback when the requested planner-model provider is not configured on the engine, with a specific clarifier instead of entering the runtime path
- [x] Show broader planner-revision availability explicitly in review instead of leaving it implicit in planner-model fields and clarifiers

## Deferred After This Slice
- [~] Replace deterministic planner-chat rules with a true LLM-backed revision loop
- Current state: planner chat now has an engine-owned LLM fallback path for unsupported general revisions when explicit planner model preferences are present, and the control panel exposes a dedicated planner-model override. Deterministic revisions still remain the primary path and provider-backed coverage is not yet in place.
- [~] Add optional export/persistence to Pack Builder after planning
- Current state: `POST /workflow-plans/apply` accepts optional `pack_builder_export` and can persist a Pack Builder preview for pending/apply follow-ups.
- [ ] Expand planner-chat semantics beyond the current safe deterministic field set
- [ ] Optionally relocate or further polish the reusable-skill export UI if Step 1 remains confusing

## Explicitly Out Of Scope For This Slice
- [ ] Dynamic replanning during runtime execution
- [ ] Large typed artifact/data contract framework
- [ ] Planner memory system beyond existing automation/runtime persistence
- [ ] Replacing legacy `/automations` all at once

## Risks
- The current planner-chat layer is still deterministic and limited, even though the contract is now stable.
- We ran targeted backend verification for this slice, but not an exhaustive full-suite sweep.

## Notes
- New work should only enter `Ship Blockers` if it blocks the current engine-first migration slice from shipping.
- Everything else belongs in `Deferred After This Slice` or a separate future-planning document.
- Current status: the engine-first automation creation slice is ready for handoff; remaining items are deferred follow-up work.
