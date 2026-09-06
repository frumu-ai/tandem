# Model profile policy

`tandem-solutions::resolve_model_profile` selects a logical model class against
current, authorized host facts. A version 1 catalog supplies the default class,
required modalities and tool support, limits, fallback edges and escalation
edges. Concrete providers, models and connection references remain in the
existing customer model bindings. The text-only example is in
`crates/tandem-solutions/fixtures/model-profiles/text-default.json`.

The resolver checks each candidate against the global provider/network policy
and every input data class's provider, processing-region and retention policy.
Unknown availability, price or retention fails closed. Availability evidence
and price must still be current. Merely listing a model in a provider catalog
does not establish availability or capability. Input token bounds must come
from a supported host calculation, not a prompt-length guess.

Fallbacks preserve the originating profile's required capabilities and narrow
token, request-cost, latency and evaluation limits along each path. Escalation
requires an explicit allowed edge and a recorded reason. Earlier path limits
and capabilities remain in force. Cycles, exhausted evaluation counts, expired
deadlines and catalog changes stop selection. A shared fallback can be checked
under distinct paths with different requirements, within the same finite
evaluation count.

The decision records the catalog digest, current binding revision, selected
path, evaluated classes, effective limits, deadline and maximum request cost.
The trusted run controller must persist the complete `selected_path`, limits,
start time and cumulative evaluation count with its root generation, then pass
them unchanged into the next resolution. Failed resolutions also consume work;
the controller must stop the attempt or persist its own failed-attempt count.
Calling this pure function again with an old input does not provide replay
protection. A catalog change requires a new approved run rather than continued
use of old history.

Selection and the existing provider-attempt budget adapter now share one checked
integer micro-USD price calculation. The ledger remains responsible for atomic
root/day/solution reservations, physical retries and settlement. The profile's
evaluation count does not count network sends or enforce concurrency. A current
zero price is explicit; missing prices never become free. These upper rates do
not establish the provider's final invoice.

## Integration boundary

This change is a planning contract and shared price implementation. It does not
activate installed agents or routines, probe providers, refresh connections,
grant account access, reserve spend, execute requests or change tool/memory
permissions. Route facts must be supplied by a trusted host adapter after it
checks the current caller, account-sharing authority, adapter snapshot and
credential revision. A decision alone is not dispatch authority.

Production integration still needs that adapter and the durable run controller,
versioned effective installation bindings, current-user reauthorization at each
physical attempt, and propagation into spawned workers and child runs. It also
needs availability/capability and data-handling evidence, embedding/transcription
and tool charges, activation wiring, user-facing blocked states and telemetry.
The standalone text example does not alter or activate the signed Company Brain
fixture. End-to-end customer acceptance remains open.

## Validation

`cargo test -p tandem-solutions --locked` covers default binding resolution,
unavailable and forbidden fallbacks, modality/tool preservation, data-class
policies, unknown/expired/overflowing prices, cost/token/deadline limits,
authorized escalation, cycle/evaluation limits, shared fallback paths, catalog
changes and versioned parsing. The fallback-history regression checks that a
later escalation cannot drop requirements from an earlier failed profile.

The existing Solution Contract workflow also runs PostgreSQL and SQLite budget
integration tests against the shared price calculation. Local formatting and
diff checks do not substitute for those runtime results.
