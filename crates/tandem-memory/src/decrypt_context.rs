//! Task-scoped decrypt principal for memory reads (TAN-668).
//!
//! The memory database is a single, `Arc`-shared, multi-tenant handle, so the
//! per-request decrypt principal (which tenant / data classes / source grants the
//! caller is authorized for) cannot live on the handle and cannot be a global.
//! Instead the retrieval gateway in tandem-server scopes it as a task-local for
//! the duration of a request's reads via [`with_decrypt_principal`], and the low
//! level read path ([`crate::db`] `row_to_chunk`) reads it with
//! [`current_decrypt_principal`] — so no read signature has to thread it.
//!
//! Local/plaintext and local-key modes never consult it (those rows carry no
//! envelope). In hosted-KMS mode a hosted-sealed row read without a scoped
//! principal fails closed rather than leaking ciphertext.
//!
//! Caveat: `tokio` task-locals do not propagate across `tokio::spawn`. A caller
//! that fans reads out onto separate spawned tasks must re-establish the scope in
//! each; the retrieval gateway awaits its reads inline, within the scope.

use crate::decrypt_broker::MemoryDecryptPrincipal;

tokio::task_local! {
    static MEMORY_DECRYPT_PRINCIPAL: MemoryDecryptPrincipal;
}

/// The decrypt principal scoped for the current async task, if any.
pub fn current_decrypt_principal() -> Option<MemoryDecryptPrincipal> {
    MEMORY_DECRYPT_PRINCIPAL
        .try_with(|principal| principal.clone())
        .ok()
}

/// Run `future` with `principal` scoped as the decrypt principal for any memory
/// reads it performs. tandem-server's retrieval gateway wraps its reads in this
/// so hosted-sealed rows are authorized + decrypted under the caller's grants.
pub async fn with_decrypt_principal<F>(principal: MemoryDecryptPrincipal, future: F) -> F::Output
where
    F: std::future::Future,
{
    MEMORY_DECRYPT_PRINCIPAL.scope(principal, future).await
}
