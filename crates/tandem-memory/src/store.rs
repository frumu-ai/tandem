//! Storage-backend abstraction seam (TAN-659).
//!
//! Introduces a [`MemoryStore`] trait and scope value types so callers can
//! depend on memory operations by contract rather than on the concrete
//! rusqlite-backed [`MemoryDatabase`]. This is the seam that a future
//! `PostgresMemoryStore` (TAN-660) and the M1 scope columns
//! (`owner_org_unit_id` — TAN-645/662; `private` / `owner_subject` — TAN-648)
//! hang on: the scope tuple lives here, once, instead of being threaded as loose
//! strings through every call site.
//!
//! This first slice is **behavior-preserving**: [`MemoryDatabase`] implements the
//! trait by delegating to its existing tenant-scoped methods. Migrating the
//! remaining operations and adding a Postgres backend are tracked follow-ups on
//! TAN-659. See `docs/STORAGE_PORTABILITY_DESIGN.md`.

use async_trait::async_trait;

use crate::db::MemoryDatabase;
use crate::types::{GlobalMemoryRecord, GlobalMemorySearchHit, MemoryResult, MemoryTenantScope};

/// The full scope for a memory **read**: tenant plus the department and per-user
/// dimensions the M1 work fills in. Bundling these here means backends receive
/// one scope value rather than a growing list of loose string parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryReadScope {
    /// Tenant partition (org / workspace / deployment).
    pub tenant: MemoryTenantScope,
    /// Department (`owner_org_unit_id`) filter — reserved for TAN-645 / TAN-662.
    /// `None` = no department narrowing.
    pub org_unit: Option<String>,
    /// Per-user subject filter for `private` items — reserved for TAN-648.
    /// `None` = department-shared (not private).
    pub subject: Option<String>,
}

impl MemoryReadScope {
    /// A tenant-only scope (no department / subject narrowing).
    pub fn tenant(tenant: MemoryTenantScope) -> Self {
        Self {
            tenant,
            org_unit: None,
            subject: None,
        }
    }
}

/// The full scope for a memory **write**. Mirrors [`MemoryReadScope`]; kept
/// separate so write-time defaults (e.g. stamping the collector's department)
/// can diverge from read-time filters as the M1 work lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryWriteScope {
    /// Tenant partition (org / workspace / deployment).
    pub tenant: MemoryTenantScope,
    /// Department (`owner_org_unit_id`) to stamp — reserved for TAN-645 / TAN-646.
    pub org_unit: Option<String>,
    /// Per-user subject to stamp when the item is `private` — reserved for TAN-648.
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

/// Operation-level storage contract for the memory subsystem (TAN-659).
///
/// Backends implement this so business logic depends on scoped operations, not
/// on a concrete SQL driver. The scope predicate must be enforced **in the
/// query** by each backend — never a global top-k that another scope's rows
/// could suppress (see `docs/STORAGE_PORTABILITY_DESIGN.md`, Decision 2).
///
/// The surface starts with the global-record read operations exercised by the
/// tenant-isolation work and grows as call sites are migrated.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Full-text search over global memory records within `scope`.
    async fn search_global_records(
        &self,
        scope: &MemoryReadScope,
        user_id: &str,
        query: &str,
        limit: i64,
        project_tag: Option<&str>,
    ) -> MemoryResult<Vec<GlobalMemorySearchHit>>;

    /// List global memory records within `scope`.
    async fn list_global_records(
        &self,
        scope: &MemoryReadScope,
        user_id: &str,
        query: Option<&str>,
        project_tag: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> MemoryResult<Vec<GlobalMemoryRecord>>;
}

#[async_trait]
impl MemoryStore for MemoryDatabase {
    async fn search_global_records(
        &self,
        scope: &MemoryReadScope,
        user_id: &str,
        query: &str,
        limit: i64,
        project_tag: Option<&str>,
    ) -> MemoryResult<Vec<GlobalMemorySearchHit>> {
        // Behavior-preserving delegation to the existing tenant-scoped query.
        // `org_unit` / `subject` narrowing is still applied by the caller's
        // access filter today; it moves into the query with TAN-645 / TAN-648.
        self.search_global_memory_for_tenant(
            &scope.tenant.org_id,
            &scope.tenant.workspace_id,
            scope.tenant.deployment_id.as_deref(),
            user_id,
            query,
            limit,
            project_tag,
            None,
            None,
        )
        .await
    }

    async fn list_global_records(
        &self,
        scope: &MemoryReadScope,
        user_id: &str,
        query: Option<&str>,
        project_tag: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> MemoryResult<Vec<GlobalMemoryRecord>> {
        self.list_global_memory_for_tenant(
            &scope.tenant.org_id,
            &scope.tenant.workspace_id,
            scope.tenant.deployment_id.as_deref(),
            user_id,
            query,
            project_tag,
            None,
            limit,
            offset,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
