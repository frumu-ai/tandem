# Provider attempt authority

Hosted membership and policy can change after a provider call starts. The existing
`ProviderDispatchAuthority` now revalidates immediately before each of the eight
adapter HTTP send sites, as well as the existing registry and authentication
recovery checks. This covers connection/timeout retries, OpenRouter affordability
and tool-choice retries, and Responses completion-to-streaming fallback.

Production Responses and Anthropic clients disable automatic HTTP redirects,
matching the existing hardened compatible/Cohere clients. Configure a provider's
final endpoint URL: a redirect must not create an additional content-bearing
request within the HTTP client without a new authority check. This intentionally
changes behavior for endpoints that previously depended on redirects.

Unscoped standalone dispatch retains the existing no-authority behavior. Current
allowed requests, OAuth recovery, exact-payload data-boundary permits and bounded
adapter retries retain their existing paths. A policy denial propagates as its
original error and is not treated as an authentication refresh opportunity.

Five new tests use synthetic local HTTP connections: revocation before a Responses
streaming fallback, the corresponding allowed fallback, all four adapter transport
retry loops, denied first sends through compatible/Responses/Anthropic adapters,
and redirect rejection through the production registry. Existing provider tests
cover unscoped fallback, authentication recovery and concurrent authority isolation.

This is an authorization barrier, not budget admission. A guard may be called
multiple times for one request; it must not charge once per check. Durable budget
reservation/reconciliation must bind actual attempts separately. This change also
does not cancel a request already sent, revoke a provider-side job, or prove full
Company Brain product, multi-user privacy or clean-host recovery acceptance.
