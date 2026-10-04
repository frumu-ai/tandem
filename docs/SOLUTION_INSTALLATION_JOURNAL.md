# Durable solution staging journal

The existing `OrchestrationStateStore` now records a solution's reviewed intent,
stable component IDs and staging progress in its SQLite/PostgreSQL database.
Schema 7 adds protected current records and append-only version receipts. Both
updates share a transaction; the existing immediate-writer lock serializes
configuration edits and journal changes on both backends.

The service must authorize the selected installation before calling this Rust
adapter. `SolutionInstallationInput` is host-owned: references, models, connector
generations, deployment readiness, policy and exact PackManager artifact bytes
must come from current trusted services. They must not be accepted from a client
or derived by copying the customer's requested values into approved sets.

Every begin/claim/staging receipt repeats customer preparation and the existing
solution resolver against the document read inside the transaction. It compares
the reviewed composition, blueprint hash and configuration generation. A hash
alone grants no authority. Configuration A → B → A, changed model metadata,
expired identity, missing readiness and revoked references invalidate retries.
The service must also reauthorize immediately before any actual runtime effect;
the transaction cannot lock an external identity or connector control plane.

## Staging protocol

1. `Begin` records an immutable intent and pending components. Repeating it for
   the same current intent returns the existing record without authorizing effects.
2. `Claim` requires an expected journal generation, a pending selected component
   and staged dependencies. It durably binds an attempt ID before the adapter acts.
3. The runtime adapter creates the component disabled, using the plan's stable
   resource ID, and verifies ownership and exact effective content.
4. `RecordStaged` matches the durable attempt and records the observed resource
   fingerprint. The current journal and immutable receipt commit together.

A crash after claim leaves an unknown outcome. No elapsed timeout or repeated
claim permits blind re-execution. A runtime adapter must inspect the authoritative
resource and reconcile ownership/content before recording the result. Automatic
retry of a proven absent effect, manual drift repair, compensation and ownership
transfer require further explicit adapter protocols. This journal does not yet
implement those operations.

All components staged does **not** mean activated or ready. This slice has no
activation endpoint, resource mutation implementation, hosted permission grant,
upgrade/uninstall route or UI/CLI. Those adapters remain required, alongside
explicit activation, source-backed product acceptance and encrypted off-site
clean-host recovery. Existing pack installation and ACA run coordination retain
their own lifecycle; neither is treated as a solution installation receipt.

## Verification

The solution-contract workflow runs real SQLite/PostgreSQL regressions for
repeat begin, dependency ordering, interrupted claims, expected-generation
concurrency, wrong attempt, current host facts, configuration ABA, atomic receipt
failure rollback, two-tenant encrypted substitution and v6-to-v7 preservation.
These are journal/store tests, not evidence that actual runtime components were
installed. Encryption uses the existing configured protected-record provider;
local plaintext mode retains its existing behavior.
