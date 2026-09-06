# Solution provider execution lineage

The solution provider-attempt budget adapter now requires the current native
Automation V2 run and execution claim. An arbitrary `root_run_id` in a trusted
callback is no longer sufficient for runtime admission.

Inside the budget writer transaction, the adapter reads the existing protected
run, goal and goal-run links. It requires a running run with an unexpired claim
matching the executor, claim ID and lease epoch; an active goal naming this run
and orchestration node; a current deadline; and a complete parent chain to the
claimed hop-zero root in the same tenant and goal version. Missing, cyclic,
cross-goal or mismatched lineage blocks admission. Native goal links remain the
source of lineage; no new run store, goal service or schema is introduced.

Encrypted native goal startup exposed a scope reconstruction defect: the
existing column reader passed deployment IDs into `TenantContext::explicit`'s
actor argument. It now preserves the deployment partition explicitly and leaves
actor identity unset. Trusted scope columns do not identify a user. The direct
encrypted round-trip regression also rejects another or missing deployment.

After reservation, it repeats the current callback and persisted execution
check. The final check samples time after acquiring its writer transaction, then
checks identity and price freshness again after the wait. A proven denial before
dispatch settles the reservation at zero. Settlement for an already dispatched
request remains possible after its goal pauses or identity expires; unknown
usage remains reserved under the existing ledger rules.

All goal children use the same protected hop-zero accounting root. Claim renewal
with the same claim ID and epoch remains valid; takeover, claim expiration,
paused/cancelled/finished goals and old active runs cannot continue spending.
The existing solution ledger enforces its root, daily and concurrent request
ceilings. Existing goal policy still controls orchestration transitions.

## Scope and remaining integration

This adapter currently supports native Automation V2 goal lineage. Standalone
sessions, AgentTemplate workers and classic routines still need their native
execution adapters; they must not fabricate a goal link to bypass those checks.
The generic accounting store remains a trusted ledger primitive, not a dispatch
API. This change does not activate installed resources or attach the optional
provider policy automatically to every worker task.

The trusted production factory must still establish current user and account
sharing authority, bind a goal to its approved installation, resolve current
model/data/price policy and supply narrowed budgets. Durable profile history,
explicit activation, source-backed text execution, non-model charges and full
customer acceptance remain open. A lineage check does not replace those grants.
Cancellation stops later admission; it cannot retract a request already sent.

## Regression evidence

The provider budget fixture now creates actual protected native goals, runs and
execution claims through the existing store. New SQLite/PostgreSQL cases cover
missing or wrong run/root/claim/executor/epoch/tenant, claim expiration, paused
or cancelled goals, stale active runs and deadlines before any HTTP send; pause
after reservation with a zero-cost refund; a child using the parent's request
ceiling after reopening storage; and missing, cyclic or foreign parent links.

The child test writes the native protected transition records directly to
isolate accounting lineage; it does not claim to validate the entire governed
handoff lifecycle or real user authorization. Existing provider wire, retry,
account revision and unknown-usage tests also exercise the required execution
check. Runtime test results must be read from the current-head CI jobs.
