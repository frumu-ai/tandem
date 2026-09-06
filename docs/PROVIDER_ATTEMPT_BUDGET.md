# Durable provider attempt accounting

`ProviderAttemptPolicy` scopes the existing provider registry. Each of the eight
concrete adapter send sites obtains a new admission after constructing its final
request. Transport retries, authentication recovery and Responses completion's
streaming fallback therefore consume separate reservations. Repeatable
`ProviderDispatchAuthority` checks remain separate from accounting.

The optional scope clamps the adapter's output token field and limits serialized
request bytes. Admission receives the actual provider/model/protocol and hashes
of the endpoint, credential headers and payload, without raw keys or prompts.
Unsupported adapters fail before dispatch while a policy is present. Unscoped
legacy dispatch keeps its existing behavior. Redirects are already disabled by
the provider-authority dependency.

`OrchestrationStateStore::solution_provider_attempt_policy` connects these sends
to the protected solution budget ledger. Its trusted callback must resolve the
current authorized model, account, price and root-run facts. The reservation
transaction verifies the stored installation generation, protected customer
configuration version and locked model binding. One atomic reservation checks
solution/day/root cost, tokens, outstanding requests and finite attempt limits.
The callback runs again after reservation; dispatch authority also revalidates
before the adapter sends. A denial before send can settle at confirmed zero.

Every physical attempt receives a server-generated ID. Missing or malformed
usage, transport errors, interrupted streams, cancellation and process failure
leave the reservation held. There is no timeout refund or retry takeover. The
existing encrypted records and backend transfer preserve those holds. A private,
nonserializable settlement capability permits reconciliation of an incurred
request after its initiating assertion expires; it cannot authorize another send.

Confirmed token counts are valued using current host-approved upper rates in
integer micro-USD. Settlement records `approved_upper_bound`, distinct from
`confirmed` costs supplied to the original ledger API. Missing/expired prices
block admission. Input rates must cover all permitted categories, including
cache writes and reads. Prices and input-token ceilings must come from reviewed
host/model facts; prompt bytes and adapter context-window defaults are not proof
of a billing bound. Overflow fails closed. An observed overrun retains the
existing global admission halt.

Usage parsers require complete unsigned counters. Anthropic input includes cache
creation/read tokens; streaming deltas merge cumulative counters. Cohere uses
billed token units for rate calculations and actual token usage for token limits.
Unpriced non-token units remain unresolved. Incomplete or decreasing cumulative
stream receipts cannot release held funds. These contracts follow the
[Claude streaming](https://platform.claude.com/docs/en/build-with-claude/streaming),
[Claude caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
and [Cohere chat](https://docs.cohere.com/v2/reference/chat) interfaces. They do not
establish a provider invoice or guarantee future vendor billing behavior.

## Verification

The full `tandem-providers` library suite includes local HTTP tests for actual
output caps, confirmed usage, receipts after scope exit, dropped streams,
separate fallback admission and revocation between reservation and send.
Codec tests cover missing, overflowing and inconsistent usage.

The existing `solution_budget_` suite runs on SQLite and PostgreSQL. Its provider
cases use actual local HTTP requests and the protected ledger to exercise
concurrent admission, unknown usage across reopen, settlement after assertion
expiry, and changed authorization before send. Synthetic approval callbacks
isolate accounting behavior; they do not prove a production account resolver.

## Remaining integration

This scope is not yet installed by a production Company Brain activation/run
entry point. Current account-generation resolution, model/tool/modality support,
data-class destination policy, independently authorized user sources/private
memory, root lineage, approved input bounds and prices still need that trusted
runtime factory. Embeddings, transcription and tool charges need their own
physical-attempt adapters. Operator reconciliation, retention and external
rollback anchors remain separate work. Activation, product UI/CLI, live-provider,
two-user privacy and clean-host acceptance remain open. Do not infer completion
of TAN-831, TAN-834 or the full system from this accounting foundation.
