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
use crate::types::{
    GlobalMemoryRecord, GlobalMemorySearchHit, GlobalMemoryWriteResult, MemoryChunk, MemoryError,
    MemoryResult, MemoryTenantScope, MemoryTier,
};

/// Fail closed when a read scope requests per-user (`subject`) narrowing that the
/// global-record query cannot yet enforce.
///
/// The [`MemoryStore`] contract says the backend enforces the scope predicate in
/// the query. Department (`org_unit`) narrowing IS now enforced on the
/// global-record surface via the `owner_org_unit_id` column + SQL predicate
/// (TAN-645), so it is passed through rather than rejected. Per-user `subject`
/// narrowing still lacks a column/predicate here (`owner_subject` / `private`,
/// TAN-648), so silently delegating it to the tenant query would **widen** the
/// read and leak same-tenant records — reject instead.
fn reject_unsupported_narrowing(scope: &MemoryReadScope) -> MemoryResult<()> {
    if scope.subject.is_some() {
        return Err(MemoryError::InvalidConfig(
            "MemoryStore SQLite backend does not yet enforce subject scope narrowing \
             (TAN-648); refusing to widen the read to tenant scope"
                .to_string(),
        ));
    }
    Ok(())
}

/// Guard for chunk vector search. The underlying query enforces `subject` via
/// `visible_subject` and department via `owner_org_unit_id`, but a shared-only
/// subject read cannot yet be honored safely and is rejected fail-closed rather
/// than silently widening the read:
///
/// - **`subject == None`** would disable the subject filter entirely: the query
///   predicate `(?N IS NULL OR c.subject IS NULL OR c.subject = ?N)` returns
///   **all** subjects' chunks when `?N` is NULL, not just shared (`c.subject IS
///   NULL`) rows. Since the query can't express "shared-only" until `owner_subject`
///   / `private` land as real predicates (TAN-648), require an explicit subject
///   rather than return every subject's memory under a "subject-enforced" contract.
fn reject_unenforceable_chunk_read(scope: &MemoryReadScope) -> MemoryResult<()> {
    if scope.subject.is_none() {
        return Err(MemoryError::InvalidConfig(
            "MemoryStore chunk search requires an explicit scope.subject: a None subject \
             disables the query's subject filter and would return all subjects' chunks \
             (TAN-648); pass the caller's subject rather than widen the read"
                .to_string(),
        ));
    }
    Ok(())
}

/// The full scope for a memory **read**: tenant plus the department and per-user
/// dimensions the M1 work fills in. Bundling these here means backends receive
/// one scope value rather than a growing list of loose string parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryReadScope {
    /// Tenant partition (org / workspace / deployment).
    pub tenant: MemoryTenantScope,
    /// Department (`owner_org_unit_id`) filter. Enforced in-query on the
    /// global-record and chunk vector surfaces via SQL predicates (TAN-645).
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
/// tenant-isolation work and grows as call sites are migrated. A backend that
/// cannot yet enforce a requested scope dimension (e.g. `org_unit` / `subject`)
/// MUST **fail closed** rather than silently widen the read.
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

    /// Insert or update a global memory record (dedup-aware). The record carries
    /// its own tenant scope today; department / subject stamping moves onto a
    /// [`MemoryWriteScope`] with TAN-646 / TAN-648.
    async fn put_global_record(
        &self,
        record: &GlobalMemoryRecord,
    ) -> MemoryResult<GlobalMemoryWriteResult>;

    /// Store (insert/replace) a memory chunk and its embedding. The chunk carries
    /// its own tenant scope + subject today; department / subject stamping moves
    /// onto a [`MemoryWriteScope`] with TAN-646 / TAN-648.
    async fn put_chunk(&self, chunk: &MemoryChunk, embedding: &[f32]) -> MemoryResult<()>;

    /// Vector-similarity search over a memory tier within `scope`. `scope.subject`
    /// (required) is enforced in the query as owner-plus-shared visibility;
    /// `scope.org_unit` is enforced in the query via `owner_org_unit_id`; a
    /// `None` `scope.subject` (which would widen to all subjects) is rejected
    /// fail-closed (TAN-648).
    async fn search_chunks(
        &self,
        scope: &MemoryReadScope,
        query_embedding: &[f32],
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> MemoryResult<Vec<(MemoryChunk, f64)>>;
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
        // Fail closed on department/subject narrowing the tenant-only query can't
        // yet enforce, rather than silently widening the read (TAN-645/648).
        reject_unsupported_narrowing(scope)?;
        // Department narrowing is enforced in-query via the owner_org_unit_id
        // predicate (TAN-645); a None org_unit is tenant-wide (behavior-preserving).
        self.search_global_memory_for_tenant_scoped(
            &scope.tenant.org_id,
            &scope.tenant.workspace_id,
            scope.tenant.deployment_id.as_deref(),
            user_id,
            query,
            limit,
            project_tag,
            None,
            None,
            scope.org_unit.as_deref(),
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
        reject_unsupported_narrowing(scope)?;
        self.list_global_memory_for_tenant_scoped(
            &scope.tenant.org_id,
            &scope.tenant.workspace_id,
            scope.tenant.deployment_id.as_deref(),
            user_id,
            query,
            project_tag,
            None,
            limit,
            offset,
            scope.org_unit.as_deref(),
        )
        .await
    }

    async fn put_global_record(
        &self,
        record: &GlobalMemoryRecord,
    ) -> MemoryResult<GlobalMemoryWriteResult> {
        self.put_global_memory_record(record).await
    }

    async fn put_chunk(&self, chunk: &MemoryChunk, embedding: &[f32]) -> MemoryResult<()> {
        self.store_chunk(chunk, embedding).await
    }

    async fn search_chunks(
        &self,
        scope: &MemoryReadScope,
        query_embedding: &[f32],
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> MemoryResult<Vec<(MemoryChunk, f64)>> {
        // Reject a None subject (which would disable the subject filter and
        // return all subjects) rather than silently widen the read (TAN-648).
        reject_unenforceable_chunk_read(scope)?;
        self.search_similar_for_tenant(
            query_embedding,
            tier,
            project_id,
            session_id,
            &scope.tenant,
            limit,
            scope.subject.as_deref(),
            scope.org_unit.as_deref(),
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

    #[test]
    fn chunk_read_guard_requires_subject_and_allows_org_unit() {
        // An explicit subject with no department narrowing is allowed…
        let mut ok = MemoryReadScope::tenant(MemoryTenantScope::local());
        ok.subject = Some("user-a".to_string());
        assert!(reject_unenforceable_chunk_read(&ok).is_ok());

        // …and department narrowing is now an owner_org_unit_id query predicate.
        let mut by_dept = MemoryReadScope::tenant(MemoryTenantScope::local());
        by_dept.subject = Some("user-a".to_string());
        by_dept.org_unit = Some("finance".to_string());
        assert!(reject_unenforceable_chunk_read(&by_dept).is_ok());

        // …and a None subject would widen to all subjects → fail closed.
        let no_subject = MemoryReadScope::tenant(MemoryTenantScope::local());
        assert!(reject_unenforceable_chunk_read(&no_subject).is_err());
    }

    #[test]
    fn narrowed_read_scope_rejects_subject_but_allows_org_unit() {
        // Department narrowing is now enforced in-query on the global-record
        // surface (TAN-645 owner_org_unit_id predicate), so it is accepted.
        let mut by_dept = MemoryReadScope::tenant(MemoryTenantScope::local());
        by_dept.org_unit = Some("finance".to_string());
        assert!(reject_unsupported_narrowing(&by_dept).is_ok());

        // Per-user (private) narrowing still lacks a column/predicate → fail closed.
        let mut by_subject = MemoryReadScope::tenant(MemoryTenantScope::local());
        by_subject.subject = Some("user-a".to_string());
        assert!(reject_unsupported_narrowing(&by_subject).is_err());

        // A department read that also requests subject narrowing is still rejected.
        let mut by_dept_and_subject = MemoryReadScope::tenant(MemoryTenantScope::local());
        by_dept_and_subject.org_unit = Some("finance".to_string());
        by_dept_and_subject.subject = Some("user-a".to_string());
        assert!(reject_unsupported_narrowing(&by_dept_and_subject).is_err());

        // Tenant-only reads are accepted (behavior-preserving path).
        assert!(
            reject_unsupported_narrowing(&MemoryReadScope::tenant(MemoryTenantScope::local()))
                .is_ok()
        );
    }
}
