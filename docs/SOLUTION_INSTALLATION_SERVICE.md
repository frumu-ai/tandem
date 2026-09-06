# Solution configuration and disabled installation

The enterprise onboarding service now connects the existing signed PackManager,
protected customer state and installation journal to native disabled agent
templates and paused routines. It requires a current synchronized hosted policy,
an unexpired verified identity with `hosted.admin`, and matching organization,
workspace and deployment. Admin operation permission does not imply source read
permission. No account, organization, invitation, source or secret is created.

The existing ConfigStore's **managed layer or startup CLI overrides** must contain
`solution_installation`, with `schema_version: 1`, a `policy` using the existing
solution Constraints schema, and a `models` map. Project, global and runtime
overlays cannot supply or widen this section. The managed configuration file and
startup arguments remain operator-controlled host files.

Each model entry contains `provider_id`, `model_id`, `credential_ref` (an existing
reference, never a key value), and optional `allow_test_provider: false`. The
concrete ProviderRegistry adapter supplies network locality and an opaque route
digest. A custom HTTP provider named `local` is still a network provider. Unknown
adapters are unavailable to this installer. Local Echo requires explicit opt-in
and only demonstrates mechanics; it is not a production language model. Concrete
credential-generation enforcement and dispatch binding remain activation work.

`profile-ref:<org-id>` refers to the current hosted organization. Enabled source
bindings expose `data-ref:<binding-id>` only with an active same-tenant connector,
prompt-context permission and the caller's current native read grant. Projects
come from those authorized source records. A private memory binding may name only
the verified caller; this does not grant access to other users' notes. Capability
connector and secret references with no implemented trusted adapter fail closed.

Three POST endpoints extend `/enterprise/onboarding-plans/solutions`:

- `/preview`: `{pack_selector, configuration}` resolves a draft without creating
  the state database or native resources. It returns the plan and composition
  digest, with `activation_required: true` and `solution_ready: false`.
- `/configuration`: `{request: {pack_selector, configuration}, expected_version}`
  saves through the existing protected generation/digest compare-and-swap store.
- `/stage`: `{pack_selector, scope, config_version, reviewed_composition,
  expected_generation}` begins/resumes the existing durable staging journal and
  materializes only disabled native agent templates and paused routines.

The composition binds current host settings, concrete provider routes, authorized
source records and connector metadata. Changing a source destination/revision
while retaining its ID invalidates the preview. Every journal transition reloads
the signed pack, authorization and protected configuration. Native claims have no
timeout takeover: their stable IDs reconcile exact existing content. Repeated
apply observes completed receipts from actual files; missing or edited resources
produce a conflict instead of being recreated. This reconciliation is safe only
for these idempotent native adapters, not arbitrary external effects. Routine
storage retains its existing one-AppState-writer-per-host constraint.

Agent staging narrows the token ceiling and sets the reviewed default model.
Routines remain paused with the customer's timezone; staging does not enable
schedules or execute models. Unsupported component kinds, tools and external
effects are rejected. Native resources cannot be activated through generic APIs.

Remaining work includes actionable aggregation of all missing prerequisites,
configuration/progress read routes, UI/CLI flows, preference/preset composition,
routine-to-agent invocation, current credential and model dispatch enforcement,
shared atomic budgets, explicit activation, upgrades/uninstall reference guards,
source-backed product behavior and clean-host recovery. The existing memory
facility readiness flag does not establish live KMS health or full solution
readiness. This slice must not close TAN-834, TAN-831 or the overall system goal.

Regression coverage includes route identity rejection, concrete provider locality,
host-rebinding rejection in SQLite/PostgreSQL journal transitions, and signed-pack
service tests for nonmutating preview, source grants, revocation, partial failure,
concurrent retry, native restart and manual deletion. CI is the execution evidence;
writing these tests alone is not a pass.
