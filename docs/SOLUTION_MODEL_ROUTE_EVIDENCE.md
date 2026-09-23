# Current model route evidence

`AppState::observe_current_model_route` builds `ModelRouteFacts` for one class in
the exact current, signed staged model-profile catalog. It is a host-only
observation, not an activation path or a provider-send permit. No caller is
wired to dispatch a completion from these facts.

The operator may add a `review` object to an existing
`solution_installation.models.<binding_id>` entry in the managed or startup CLI
configuration. This follows the same operator-only path as `policy`, `models`,
and `account`; customer configuration and request fields cannot provide it.
Its version 1 fields are `revision`, `provider_id`, `model_id`, `credential_ref`,
`authorization_revision`, `endpoint_sha256`, `reviewed_until_ms`, `modalities`,
`supports_tool_use`, `processing_regions`, `retention_hours`, and `price`.
`price` uses the existing `ModelProfilePrice` fields, including
`valid_until_ms`. The endpoint digest is the current
`ProviderRuntimeBinding.endpoint_sha256`, obtained from the provider registry;
it binds the review to the actual transport, including its credential source.
The operator must independently verify capability, processing region, data
retention, and price before setting these claims. The host does not derive
these claims from a provider model list.

The factory requires the protected staged installation and signed catalog to
match the requested generation, composition, and class. It reproduces the
approved host-facts digest, authorizes the current human's use of the existing
credential resource, validates the reviewed route and its expiry, then asks the
adapter for a live model-list observation. The OpenAI-compatible adapter sends
`GET /models` to its configured API base, with the existing tenant credential,
bounded response, no redirects, and the provider endpoint restrictions. The
selected model ID must appear exactly once. Unsupported adapters, missing or
ambiguous models, stale reviews, and transport or credential drift fail closed.
The observation expires after five seconds; a model-list response gives coarse
availability evidence, not a guarantee that a completion will succeed.

The factory checks account authorization immediately before the probe request
and repeats account, catalog, installation, host facts, review, and observation
checks after the network wait. A caller resolving constraints must use the
returned expiry before it lapses. Every future physical provider attempt still
needs a fresh actor/account/current-route authorization check at send time.

The full host-facts digest currently requires the installer/admin context to
reproduce the approved plan. This factory deliberately rejects normal member
execution. Member execution requires a separate actor-specific approval and a
protected route-review receipt tied to the staged plan, followed by the same
pre-send revalidation. It also requires an adapter availability strategy for
providers without a supported model-list endpoint. Neither dispatch
integration nor those later acceptance steps are implemented here.
