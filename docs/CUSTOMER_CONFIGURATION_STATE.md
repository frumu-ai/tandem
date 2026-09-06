# Customer configuration state

`OrchestrationStateStore` persists the existing `tandem-solutions` customer
document separately from reusable packs and runtime component installation.
It uses the selected SQLite or PostgreSQL backend, its transaction boundary,
and the existing tenant/kind/record-bound encryption helpers.

The storage key contains organization, workspace, deployment and installation.
The caller supplies a verified user context and an explicitly authorized
installation selection. Customer configuration cannot create that authority.
The service must authorize reading/editing the installation and obtain current
host-owned references, connector generations, subjects, departments, projects
and policy before calling the store. The existing preparation function checks
those bindings again with the revision loaded inside the transaction.

Every change atomically updates the current document and appends a protected
version record. Both copies bind the configuration digest, blueprint digest,
actor and time. The expected version includes a monotonically increasing
generation as well as the content digest, so changing A to B and back to A
cannot make an earlier preview current. Concurrent writes use a database
compare-and-set; a failed history append rolls back the document update.
Saving unchanged content against the current version is a no-op.

Schema version 6 adds the two scoped tables on both backends without rewriting
existing runtime records. Existing backend transfer discovers them along with
the rest of the stateful schema. No data downgrade or delete is introduced.

The deployable document remains customer-sensitive, including opaque reference
names. Reusable export still uses the blueprint-only template from
`tandem-solutions`; it does not serialize stored customer configuration.
Encryption follows the existing configured provider: local plaintext mode is
still plaintext, and a local-key test is not production KMS recovery evidence.

The shared regression suite runs against SQLite and real PostgreSQL in CI. It
covers two organizations with the same installation name, reopen, scope and
expiry rejection, concurrent edits, A/B/A stale versions, append-failure
rollback, encrypted tenant substitution, sanitized template export and schema
upgrade with existing runtime state.

This storage adapter is not an HTTP installation API or an activation receipt.
The authorized service/UI/CLI integration, current-policy checks before runtime
effects, component materialization, durable apply/activation receipts and
clean-host recovery remain required work. No solution is installed merely
because its customer document has been saved.
