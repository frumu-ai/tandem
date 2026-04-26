//! Identity resolution for principals that act through external surfaces.
//!
//! Anyone who clicks a button on a Slack/Discord/Telegram approval card or
//! types a slash command needs to be resolved to a Tandem-owned
//! [`RequestPrincipal`] before their action is recorded in audit. Without
//! this resolver, the audit trail would only carry the surface user ID
//! ("slack:U123") and could not be cross-referenced with workflow ownership,
//! tenant scope, or downstream approval delegation.
//!
//! v1 keeps the model simple:
//!
//! - The resolver inspects the channel's `allowed_users` allowlist.
//! - If the surface user is allowed, it returns a `RequestPrincipal` whose
//!   `actor_id` carries a stable `channel:{kind}:{user_id}` identifier and
//!   whose `source` names the channel.
//! - If the surface user is not allowed, the resolver returns `None` and
//!   callers MUST reject the action (do not silently approve as anonymous).
//!
//! Future: when the enterprise sidecar wires SSO + RBAC, this resolver
//! becomes a thin wrapper that delegates to the sidecar's `resolve_identity`
//! capability instead. The shape returned to callers (a `RequestPrincipal`
//! plus optional `AuthorityChain` metadata) does not change.

pub mod channel_identity;
