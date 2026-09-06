# Native goal installation binding

The trusted solution provider budget path now requires both native execution
lineage and an immutable association between that goal and its installation.
An active goal for installation A cannot spend against installation B, even in
the same tenant with an otherwise valid model approval.

`OrchestrationStateStore::start_solution_goal` validates the current protected
customer configuration, fully staged installation, generation, composition and
verified human. It commits the association with the existing native goal, root
run, hop-zero link and start event in one writer transaction. A reused root run
is rejected. Replaying the same start returns the existing records; replaying
another installation or actor under that goal ID fails. No additional root
registry, storage schema or secret store is introduced.

The association is an authenticated envelope inside the existing encrypted goal
record. Its record kind and goal ID are authenticated through the existing
protected-record subsystem. This additional provenance matters because older
goal APIs accepted arbitrary metadata: plaintext metadata, even inside a valid
encrypted outer goal, cannot assert host approval. Copying an envelope from a
different goal also fails. Ordinary goal start rejects the reserved metadata
key. Generic goal writes preserve the association, including its absence, under
a writer transaction; they cannot attach, strip or replace it later.

Every physical provider reservation checks the association against the current
installation/configuration and actual root. The final check after reservation
repeats those reads with time sampled after the writer lock. Existing child
lineage shares the same association and budget; stopping execution still permits
settlement of already admitted attempts. Existing ordinary goals remain usable
outside solution dispatch but cannot acquire a solution budget through metadata.

This storage entry point accepts trusted host inputs. It is not exposed as a new
HTTP endpoint and does not authorize activation, establish current source/model
permissions or verify a workflow belongs to an installed worker component. The
production service still needs those checks, the current execution actor factory,
durable profile history and propagation into workers/routines/sessions before
Company Brain activation and end-to-end acceptance. No staged resource is enabled
by this change. The existing installer identity must not become another runtime
user's private-memory authority.

Regression coverage uses actual SQLite/PostgreSQL state and existing physical
provider attempts: rejected unstaged creation leaves no goal/root, restart and
idempotent creation, immutable generic writes, concurrent competing installation
starts, cross-installation spending with zero sends/reservations, and historical
plaintext/copied-envelope forgery. Existing child/restart and shared-ceiling tests
now start associated goals. Storage fixtures record synthetic staging receipts;
these tests do not claim real activation or governed worker handoff acceptance.
