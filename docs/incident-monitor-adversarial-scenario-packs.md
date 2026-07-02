# Incident Monitor — Adversarial Scenario Packs

Adversarial scenario packs are versioned, data-driven descriptions of
production-mirroring governance edge cases. They exercise the Incident Monitor
destination router's **route preview, approval gate, and readiness gates** so
you can surface authority-boundary, escalation, and safety gaps *before*
production — as a complement to the Phase-7 controlled probes.

## Safety model

- **Dry-run / sandbox only.** Running a scenario pack never mutates external
  systems. The runner only reads the live configuration and computes a route
  preview; `mutates_external_systems` is always `false` in the response.
- **Authorized and bounded.** Scenario packs are an admin-only surface. They
  must only be run against systems you operate, with the full admin API token —
  a scoped intake key is rejected. Do not point custom packs at destinations or
  tenants you are not authorized to assess.

## What a scenario asserts

Each scenario declares a synthetic (untrusted) incident input and the control
behavior a well-governed configuration should produce, e.g.:

- a high-risk regulatory / prompt-injection / denied-action case must
  **require approval** (`approval_required: true`);
- an unsafe, unready, or unknown destination must be **blocked** (fail closed);
- a forged cross-tenant report must not route across the tenant boundary.

A scenario **passes** when the operator's configuration produces the expected
control behavior. A **failing** scenario surfaces a governance gap and carries a
`finding_id` for evidence linkage. When no destination is routable to evaluate
the approval gate, the scenario is reported as `blocked` (not evaluated).

## Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/incident-monitor/security/scenario-packs` | Describe the built-in pack without executing it. |
| `POST` | `/incident-monitor/security/scenario-packs` | Run the built-in (or a supplied) pack in dry-run. |

`POST` accepts an optional body:

```json
{
  "pack": { "pack_id": "...", "version": "...", "scenarios": [ ... ] },
  "scenario_ids": ["prompt_injection_requires_approval"]
}
```

- `pack` — an optional custom pack to run instead of the built-in default pack.
- `scenario_ids` — an optional subset of scenario ids to run.

Scenario results are also folded into the Phase-8 security assessment report
(`/incident-monitor/security/assessment-report`) under
`sections.adversarial_scenario_packs`, and are persisted in the report's
evidence pack.

## Built-in default pack

The built-in pack (`tandem_default_adversarial`, versioned) covers: regulatory
escalation, cross-system disputes, prompt injection / instruction conflict,
excessive agency / tool overreach, low-confidence / missing-evidence
escalation, cross-tenant / missing-tenant-context, unsafe / unready
destinations, and recurring denied-action escalation.
