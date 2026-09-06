# Shared solution budget accounting

Solution budgets now reserve and reconcile charges through the existing
OrchestrationStateStore. SQLite's immediate transaction and PostgreSQL's existing
schema-scoped writer lock update the whole accounting operation atomically.
Schema version 8 adds protected current records and immutable version history;
the existing backend transfer copies both. This is accounting infrastructure:
it does not activate a solution or grant permission to send a provider request.

A trusted runtime caller supplies a verified tenant context, installation scope,
current composition, an authoritative root run, a unique physical-attempt ID,
reviewed route revision, bounded token/cost reservation and finite root-run
limits. None of these approval inputs come from a browser, pack or model output.
The store loads the current installation and intersects limits with its reviewed
token, concurrency and daily cost ceilings. A root's limits can narrow but cannot
grow during retries, escalation or child execution.

Each admission updates three accounts in one transaction: the solution's global
outstanding count, its UTC-day cost account, and its root-run account. The global
outstanding limit conservatively applies the configured concurrent-run ceiling
to physical requests, including requests within one root. Children must inherit
the runtime's root identifier. Model, retry, escalation, embedding, transcription
and tool charges use the same accounting operation; their actual adapters still
need to call it before dispatch.

Money uses integer micro-USD. Unknown pricing blocks admission, rather than being
treated as free. Estimates must bound the intended request; rates are deployment
inputs, not hardcoded vendor prices. A duplicate attempt returns
`newly_reserved: false` and must be observed/reconciled, never sent again. A changed
intent cannot reuse that ID. Failed admission cannot partially consume counters.

There is no timeout refund. After an unknown transport result or crash, cost and
concurrency stay reserved even across midnight. A confirmed adapter receipt may
settle once; an exact repeat is idempotent and conflicting settlements fail.
Confirmed non-dispatch can settle at zero. Actual usage above the reservation is
recorded honestly and halts further admissions, including on the next day, until
an operator reconciliation protocol handles the overrun. That protocol is not
implemented in this slice. An old reservation settles against its original UTC
day. The global clock high-water mark rejects backwards-time admission.

Protected record identity includes the tenant, installation, record key and
generation. History detects deleting or rolling back only a current row. A full
database snapshot rollback still needs the system's external recovery anchors;
local version history alone does not prove off-host anti-rollback protection.
Retention must preserve unsettled attempts and consumed root-run accounting;
automatic pruning is intentionally absent.

Regression tests use real SQLite and PostgreSQL transactions for simultaneous
workers, root/child token and request limits, duplicate receipts, changed routes,
unknown price, cross-tenant scopes, overruns, midnight/clock rollback, current-row
deletion/rollback and atomic v7 migration. The existing encrypted backend round
trip now carries a settled charge and an interrupted reservation, and verifies
that the restored budget cannot spend the reserved remainder again.

TAN-831 remains open. Required next work includes binding current credentials and
model profiles to actual provider dispatch, proving output/retry bounds for each
supported adapter, routing/modality/region/retention checks, finite escalation,
usage reconciliation and telemetry, explicit activation, budget-blocked runtime
state and UI/doctor evidence. These storage tests alone cannot prove that every
provider/tool charge is routed through this accounting boundary.
