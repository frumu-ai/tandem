//! Portable storage contract for the memory subsystem (TAN-677).
//!
//! [`MemoryStore`] exposes scoped read, query, write, mutation, batch, health,
//! and migration-capability operations without leaking a database driver into
//! business logic. [`crate::db::MemoryDatabase`] remains the SQLite compatibility
//! adapter and delegates these operations to its tenant-aware implementation.

use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

use crate::types::{MemoryResult, MemoryTenantScope};

#[path = "store_adapter.rs"]
mod adapter;
#[path = "store_contract.rs"]
mod contract;

pub use contract::*;

/// Open Tandem's bundled SQLite implementation behind the portable contract.
/// Runtime callers use this assembly point rather than depending on the
/// concrete database adapter.
pub async fn open_sqlite_memory_store(db_path: &Path) -> MemoryResult<Arc<dyn MemoryStore>> {
    let database = crate::db::MemoryDatabase::new(db_path).await?;
    Ok(Arc::new(database))
}

/// Open the configured production backend. SQLite remains the default; setting
/// `TANDEM_MEMORY_BACKEND=postgres` requires the `postgres` crate feature and a
/// `TANDEM_MEMORY_POSTGRES_URL` connection string.
pub async fn open_memory_store(db_path: &Path) -> MemoryStoreResult<Arc<dyn MemoryStore>> {
    match std::env::var("TANDEM_MEMORY_BACKEND")
        .unwrap_or_else(|_| "sqlite".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "sqlite" => open_sqlite_memory_store(db_path)
            .await
            .map_err(MemoryStoreError::from),
        "postgres" | "postgresql" => {
            #[cfg(feature = "postgres")]
            {
                let config = crate::postgres_store::PostgresMemoryStoreConfig::from_env()?;
                let store = crate::postgres_store::PostgresMemoryStore::connect(config).await?;
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "postgres"))]
            {
                Err(MemoryStoreError::unsupported(
                    "TANDEM_MEMORY_BACKEND=postgres requires the tandem-memory/postgres feature",
                ))
            }
        }
        backend => Err(MemoryStoreError::invalid(format!(
            "unsupported TANDEM_MEMORY_BACKEND value: {backend}"
        ))),
    }
}

/// Visibility authority carried by a read or mutation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryReadAccess {
    /// Return shared rows plus private rows owned by `subject`.
    Scoped,
    /// Trusted local/system maintenance may address every row in the tenant.
    /// HTTP/client input must never select this mode.
    TrustedUnrestricted,
}

/// The full scope for a memory **read**: tenant plus the department and per-user
/// dimensions enforced by storage queries. Bundling these here means backends
/// receive one scope value rather than loose string parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryReadScope {
    /// Tenant partition (org / workspace / deployment).
    pub tenant: MemoryTenantScope,
    /// Department (`owner_org_unit_id`) filter. Enforced in-query on the
    /// global-record and chunk vector surfaces via SQL predicates (TAN-645).
    /// `None` = no department narrowing.
    pub org_unit: Option<String>,
    /// Caller subject for private visibility. `Some(subject)` includes shared
    /// rows plus that subject's private rows; `None` returns shared rows only.
    pub subject: Option<String>,
    /// Normal owner visibility or an explicit trusted local/system bypass.
    pub access: MemoryReadAccess,
}

impl MemoryReadScope {
    /// A tenant-only scope (no department / subject narrowing).
    pub fn tenant(tenant: MemoryTenantScope) -> Self {
        Self {
            tenant,
            org_unit: None,
            subject: None,
            access: MemoryReadAccess::Scoped,
        }
    }

    pub fn trusted_unrestricted(tenant: MemoryTenantScope) -> Self {
        Self {
            tenant,
            org_unit: None,
            subject: None,
            access: MemoryReadAccess::TrustedUnrestricted,
        }
    }
}

/// The full scope for a memory **write**. Mirrors [`MemoryReadScope`]; kept
/// separate so write-time defaults (e.g. stamping the collector's department)
/// can diverge from read-time filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryWriteScope {
    /// Tenant partition (org / workspace / deployment).
    pub tenant: MemoryTenantScope,
    /// Department (`owner_org_unit_id`) to stamp.
    pub org_unit: Option<String>,
    /// Per-user subject to stamp when the item is private.
    pub subject: Option<String>,
}

impl MemoryWriteScope {
    /// A tenant-only write scope (no department / subject stamping).
    pub fn tenant(tenant: MemoryTenantScope) -> Self {
        Self {
            tenant,
            org_unit: None,
            subject: None,
        }
    }
}

/// Operation-level storage contract for the memory subsystem (TAN-677).
///
/// Backends implement this so business logic depends on scoped operations, not
/// on a concrete SQL driver. The scope predicate must be enforced **in the
/// query** by each backend — never a global top-k that another scope's rows
/// could suppress (see `docs/STORAGE_PORTABILITY_DESIGN.md`, Decision 2).
/// A backend that cannot enforce a requested scope dimension or operation mode
/// MUST return a contract error rather than silently weaken the request.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Execute a scoped point/list read using backend-neutral request and result
    /// values.
    async fn read(
        &self,
        request: MemoryStoreReadRequest,
    ) -> MemoryStoreResult<MemoryStoreReadResult>;

    /// Execute a scoped search or filtered-list query.
    async fn query(
        &self,
        request: MemoryStoreQueryRequest,
    ) -> MemoryStoreResult<MemoryStoreQueryResult>;

    /// Execute a scoped insert or upsert.
    async fn write(
        &self,
        request: MemoryStoreWriteRequest,
    ) -> MemoryStoreResult<MemoryStoreWriteResult>;

    /// Execute a scoped update, delete, or maintenance transition.
    async fn mutate(
        &self,
        request: MemoryStoreMutationRequest,
    ) -> MemoryStoreResult<MemoryStoreMutationResult>;

    /// Execute multiple writes/mutations under explicit commit semantics.
    async fn batch(
        &self,
        request: MemoryStoreBatchRequest,
    ) -> MemoryStoreResult<MemoryStoreBatchResult>;

    /// Probe backend-owned storage structures without exposing driver details.
    async fn backend_health(
        &self,
        request: MemoryBackendHealthRequest,
    ) -> MemoryStoreResult<MemoryBackendHealthResult>;

    /// Perform an explicit backend-wide recovery action. Destructive reset
    /// requests must carry their data-loss confirmation in the request value.
    async fn recover_backend(
        &self,
        _request: MemoryBackendRecoveryRequest,
    ) -> MemoryStoreResult<MemoryBackendRecoveryResult> {
        Err(MemoryStoreError::unsupported(
            "this memory backend does not expose recovery operations",
        ))
    }

    /// Describe whether this backend can satisfy migration-coordinator needs.
    async fn migration_capabilities(
        &self,
        request: MemoryMigrationCapabilityRequest,
    ) -> MemoryStoreResult<MemoryMigrationCapabilityResult>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemoryDatabase;

    // Compile-time assertion that the concrete SQLite-backed database satisfies
    // the storage contract — i.e. the seam exists and is object-safe to depend on.
    const _: fn() = || {
        fn assert_impl<T: MemoryStore>() {}
        assert_impl::<MemoryDatabase>();
    };

    #[test]
    fn read_scope_tenant_only_has_no_narrowing() {
        let scope = MemoryReadScope::tenant(MemoryTenantScope::local());
        assert!(scope.org_unit.is_none());
        assert!(scope.subject.is_none());
        assert_eq!(scope.tenant, MemoryTenantScope::local());
    }

    #[test]
    fn write_scope_tenant_only_has_no_stamping() {
        let scope = MemoryWriteScope::tenant(MemoryTenantScope::local());
        assert!(scope.org_unit.is_none());
        assert!(scope.subject.is_none());
    }

    #[test]
    fn read_scope_represents_shared_and_private_visibility() {
        let shared = MemoryReadScope::tenant(MemoryTenantScope::local());
        assert!(shared.subject.is_none());

        let mut private = shared.clone();
        private.subject = Some("user-a".to_string());
        private.org_unit = Some("finance".to_string());
        assert_eq!(private.subject.as_deref(), Some("user-a"));
        assert_eq!(private.org_unit.as_deref(), Some("finance"));
    }
}

#[cfg(test)]
#[path = "store_contract_tests.rs"]
mod contract_tests;
