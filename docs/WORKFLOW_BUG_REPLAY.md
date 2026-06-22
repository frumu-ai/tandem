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

## Dogfooding Regression Fixtures

Long-lived dogfooding regressions live in
`eval_datasets/dogfooding_regressions.yaml`. Each case is an eval-runner test
case tagged `dogfooding_regression` and should include:

- `automation_spec.config.source_issue` or `source: bug_monitor`
- `automation_spec.config.historical_failure_signature`
- `automation_spec.config.expected_guardrail`
- validators that name the protected bug class, not just the surface symptom
- quality indicators for the evidence the replay must preserve

Run the seeded suite locally with:

```bash
cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/dogfooding_regressions.yaml \
  --engine-mode stub \
  --filter-tag dogfooding_regression \
  --verbose
```

The scheduled `Dogfooding Regression Fixtures` GitHub Actions workflow runs the
same dataset nightly through the deterministic stub engine path and can be
triggered manually.

## Bug Monitor Scaffold Command

When Bug Monitor produces an incident JSON export, scaffold a replay dataset
with:

```bash
cargo run -p tandem-eval --bin bug-monitor-fixture -- \
  --incident /tmp/incident.json \
  --output eval_datasets/regressions/dogfood_006.yaml \
  --id dogfood_006_short_name \
  --tag dogfooding_regression
```

The scaffold command writes an eval-runner-compatible YAML dataset and redacts
prompt-like fields, arguments, message bodies, credentials, and authorization
values before they land in the fixture.

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
6. Add or update the dogfooding regression fixture, or explicitly explain why
   the bug cannot be replayed deterministically yet.
7. Do not mark the fix complete until the replay exists in the repo.

## Release Rule

If a workflow-runtime fix shipped since the previous release does not have replay coverage, the release candidate is not ready.

Related:

- [Engine Testing](./ENGINE_TESTING.md)
- [Release Process](./RELEASE_PROCESS.md)
