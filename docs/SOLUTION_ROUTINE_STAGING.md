# Native solution routine staging

`stage_solution_routine` materializes a tenant-scoped `RoutineSpec` using the
existing routine store. It sets paused status, disabled installation ownership,
no fire time and no previous execution. The canonical receipt includes native
content, tenant, installation, component and reviewed composition. Repeating
staging after reopening the store returns the same receipt only for identical
ownership and content. Customer routines with the same ID in different tenants
remain independent.

The caller must authorize the installation and revalidate its current journal,
signed artifact and host bindings before claiming and performing this effect.
`solution_routine_from_artifact` maps the current text fixture's reusable input
into native types, including its string `skip` policy. Tenant, creator and
ownership fields cannot come from the artifact. The initial artifact adapter
accepts the text fixture's paused/skip schema; it is not a replacement for the
native routine API's other scheduling policies.

Paused alone is insufficient: existing routines support manual execution while
paused. Installation ownership therefore provides a separate disabled state.
Manual execution policy rejects it, scheduling skips it, run creation records a
blocked run, and the final queue claimant rechecks the current native resource.
Old approval/recovery records cannot run a staged or missing managed routine.
Generic creation/update/delete cannot mutate managed resources or reserved
`solution-` IDs. Existing ordinary paused routines retain manual-run behavior.
Legacy manual IDs in the reserved namespace need an operator-reviewed migration.

The adapter reuses the existing host's single AppState writer and persistence
mutex. It detects disk/cache drift before staging and preserves conflicting
manual state. The native persistence path now syncs the temporary file before
rename and syncs the parent directory on Unix. Failed publication rolls back the
cache; a failure after rename still requires reload/reconciliation. The store
does not gain support for simultaneous independent engine processes writing the
same routines file. Windows directory-entry power-loss durability is not proven
by the process-restart tests.

This is native staging, not activation or full installation acceptance. The
authorized HTTP/service, live bindings, explicit activation and schedule start,
upgrade/rollback, UI/CLI and encrypted clean-host recovery remain. No new public
HTTP mutation route is introduced. Runtime/customer data must stay outside
reusable pack exports.
