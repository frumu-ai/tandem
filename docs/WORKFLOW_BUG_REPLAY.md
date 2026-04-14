# Workflow Bug Replay Guide

This guide defines the minimum replay artifact and bug-fix loop for workflow-runtime failures.

Use it when:

- a live workflow blocks unexpectedly
- a workflow-runtime fix changes prompting, validation, repair, delivery, or status projection
- a release candidate includes workflow contract changes

## Policy

A workflow-runtime bug fix is not complete until a deterministic replay regression exists.

That replay must preserve the contract class of the live failure, not just the surface wording of one workflow.

## Minimum Replay Fixture

Capture these fields from the live blocker:

- automation ID
- run ID
- node ID
- node objective
- output contract kind
- required output path
- required workspace files, if any
- offered tools
- executed tools
- unmet requirements
- blocker category
- validator summary or blocker reason
- whether web research was used
- whether workspace inspection was used
- any upstream artifact paths or handoffs that materially affected the node

If the failure involved retries or repair:

- previous attempt count
- whether repair budget remained
- previous validation outcome
- required next tool actions

## Replay Template

Use this template when turning a live blocker into a regression:

```text
Live blocker:
- node_id:
- contract_kind:
- required_output_path:
- required_workspace_files:
- blocker_category:
- unmet_requirements:
- offered_tools:
- executed_tools:
- workspace_inspection_used:
- web_research_used:
- upstream_artifact_paths:
- validator_summary:
- repair_state:

Expected replay assertions:
- validation_outcome:
- final node status:
- failure kind / blocker category:
- required next actions:
- required unmet requirements that must remain visible:
```

## What The Replay Must Prove

At minimum, assert the bug class we care about:

1. The runtime preserves the right unmet requirements.
2. The node status and blocker category match the repairability of the failure.
3. Required next actions point the model at the correct repair path.
4. The failure cannot silently degrade into a generic fallback classification.

When relevant, also assert:

1. required workspace files are named explicitly
2. upstream evidence remains visible to synthesis validators
3. delivery failures distinguish unavailable tools from unexecuted delivery
4. code workflows distinguish missing verification from failed verification

## Mapping Live Bugs To Test Types

Use the narrowest test that still protects the bug class:

- `validation` tests:
  - unmet requirements
  - validation outcome
  - warning counts
  - quality-mode behavior
- `workflow_policy` tests:
  - final node status
  - blocker category
  - failure kind
  - repair routing
- `prompting` tests:
  - missing or contradictory instructions
  - wrong required tools
  - wrong output-path guidance
- `integration` tests:
  - multi-step handoff drift
  - delivery side effects
  - repair loops
  - end-to-end archetype behavior

## Bug-Fix Loop

When a workflow bug is reported:

1. Capture the live blocker details with the minimum replay fixture.
2. Write the failing replay regression before or alongside the fix.
3. Land the runtime fix.
4. Run the exact replay test locally.
5. If the bug class is release-relevant, add the replay to the deep gate or targeted release subset.
6. Do not mark the fix complete until the replay exists in the repo.

## Release Rule

If a workflow-runtime fix shipped since the previous release does not have replay coverage, the release candidate is not ready.

Related:

- [Engine Testing](./ENGINE_TESTING.md)
- [Release Process](./RELEASE_PROCESS.md)
