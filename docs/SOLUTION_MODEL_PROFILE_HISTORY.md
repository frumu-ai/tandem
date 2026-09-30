# Durable model profile history

`OrchestrationStateStore::select_runtime_model_profile` runs the existing
versioned profile resolver under the same writer transaction that verifies the
current native run claim, goal/root association, protected customer configuration
and fully staged installation. It replaces caller-supplied prior route history,
start time, catalog digest, customer model bindings and global constraints with
the protected record and current installation. The clock is sampled after the
writer lock. A selected binding must still be in the current installation plan.

The record is keyed by installation scope, actual root and logical operation ID
and uses the existing encrypted, versioned solution record store. Separate
workers under one root have separate histories; a second request for the same
selection ID returns `Replayed` without a decision. A successful selection
persists the full path, cumulative evaluations, narrowed limits and catalog
digest for the next transition. Any failed resolution terminates that logical
operation because the pure resolver cannot report how many candidates it
evaluated before failing. Retrying requires a new logical operation with its
own current native execution and budget authority. A restart does not reset
the history or make a repeated selection sendable.

This method is a protected selection record, not dispatch permission. The
caller still has to supply trustworthy current availability, capability,
processing region, retention, data-class and price facts. It must recheck the
current actor's model-account grants and the actual provider route for every
physical attempt, then use the existing shared budget reservation and final
pre-send check. The current code does not yet wire this controller into active
worker/routine/session launches or install activation. Its tests use synthetic
route facts and staging receipts with actual SQLite/PostgreSQL protected state;
they do not establish a live provider, billing or customer privacy result.
