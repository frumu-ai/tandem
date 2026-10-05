# Final response commit authority

The engine preserves the verified identity captured when a prompt starts. Before
the final assistant response is saved, it retains the existing asynchronous
session check and waits for the session SQLite writer. After acquiring that
writer, the hosted server checks the original identity against current policy
and holds that policy through the successful commit and the final message/idle
events. A policy revision change or expired/denied identity at that boundary
prevents the assistant write and those success events; the prompt fails and its
provider cancellation token is cancelled.

The synchronous `ToolPolicyHook::with_session_commit_authority` callback must
invoke and return its continuation exactly once. It must not await, repeat
approval/audit-producing tool evaluation, acquire the policy publication mutex
after the SQLite writer, or recursively enter policy authorization while its
snapshot lock is held. Success events follow `commit()?` inside the same
continuation, with no later fallible work. Cancelling the awaiting task does not
drop the blocking worker's policy guard while it is committing/publishing.

Standalone engines and existing embedders retain the default callback behavior;
the hosted server supplies the current-policy guard through existing native
storage/policy APIs. This introduces no tandem-web or tandem-agents dependency,
new storage schema, approval bypass or deployment configuration.

Core writer-wait tests cover revoked and current authority, an engine without a
host hook, SQL insertion failure, commit/event scope and the original identity
after the stored session is renewed. Server tests exercise the actual engine
with `ServerToolPolicyHook`, a real SQLite writer, production policy reload and
the native guard lifetime through final event publication/caller cancellation.
Hosted-policy CI verifies that the named regression cases exist and execute.

This boundary governs the final assistant response. Previously streamed output,
planning todo/question fallbacks, queued/running tool effects, synchronization
outages, supported releases and clean-host recovery require their own acceptance
evidence. Effects already sent before revocation cannot be undone. Whole
TAN-836/TAN-840/TAN-842 remain In Progress until their full criteria are proved.
