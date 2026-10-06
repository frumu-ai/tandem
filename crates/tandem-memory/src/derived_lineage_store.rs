use std::cell::RefCell;

use crate::derived_lineage::{
    CanonicalMemoryRestriction, DerivedMemoryLineage, ResolvedDerivedLineage,
};
use crate::store::*;
use crate::types::SourceObjectLifecycleState;

const MAX_CANONICAL_READS: usize = 128;

/// Called only on the current row held by a native guarded write transaction.
pub(crate) fn ensure_expected_target(
    record: Option<&crate::types::GlobalMemoryRecord>,
    tenant: &crate::types::MemoryTenantScope,
    expected: &tandem_types::MemorySourceReference,
) -> MemoryStoreResult<()> {
    let current = record.and_then(|record|
        CanonicalMemoryRestriction::from_global_record(record,tenant).ok());
    if current.is_none_or(|current| current.source_reference() != *expected) {
        return Err(MemoryStoreError::new(MemoryStoreErrorKind::ScopeViolation,
            "guarded memory target changed or is unavailable"));
    }
    Ok(())
}

#[derive(Default)]
struct ResolutionState {
    path: Vec<String>,
    reads: usize,
}

tokio::task_local! { static RESOLUTION: RefCell<ResolutionState>; }

struct SourceReadGuard;
impl Drop for SourceReadGuard {
    fn drop(&mut self) {
        let _ = RESOLUTION.try_with(|state| {
            state.borrow_mut().path.pop();
        });
    }
}

fn enter_source(id: &str) -> MemoryStoreResult<SourceReadGuard> {
    RESOLUTION.with(|state| {
        let mut state = state.borrow_mut();
        if state.path.len() >= crate::derived_lineage::MAX_DERIVED_LINEAGE_DEPTH
            || state.reads >= MAX_CANONICAL_READS
            || state.path.iter().any(|seen| seen == id)
        {
            return Err(MemoryStoreError::new(
                MemoryStoreErrorKind::ScopeViolation,
                "derived source resolution cycle or bound",
            ));
        }
        state.reads += 1;
        state.path.push(id.to_string());
        Ok(SourceReadGuard)
    })
}

/// Resolve existence and semantic revisions without trusting persisted flags.
/// Current principal/grant decisions are applied separately by MemoryAccessFilter.
pub async fn resolve_derived_lineage<S: MemoryStore + ?Sized>(
    store: &S,
    scope: &MemoryReadScope,
    lineage: &DerivedMemoryLineage,
) -> MemoryStoreResult<ResolvedDerivedLineage> {
    if RESOLUTION.try_with(|_| ()).is_err() {
        RESOLUTION
            .scope(
                RefCell::new(ResolutionState::default()),
                resolve_inner(store, scope, lineage),
            )
            .await
    } else {
        resolve_inner(store, scope, lineage).await
    }
}

async fn resolve_inner<S: MemoryStore + ?Sized>(
    store: &S,
    scope: &MemoryReadScope,
    lineage: &DerivedMemoryLineage,
) -> MemoryStoreResult<ResolvedDerivedLineage> {
    lineage.validate().map_err(MemoryStoreError::from)?;
    let mut proof = ResolvedDerivedLineage::default();
    for expected in &lineage.sources {
        if expected.tenant_scope != scope.tenant {
            return Err(MemoryStoreError::new(
                MemoryStoreErrorKind::ScopeViolation,
                "derived source tenant mismatch",
            ));
        }
        let _guard = enter_source(&expected.memory_id)?;
        let mut source_scope = scope.clone();
        // The inherited target enforces departments or source grants. Do not
        // replace a grant-governed source with the output's department default.
        source_scope.org_unit = None;
        let record = match store
            .read(MemoryStoreReadRequest::GlobalRecord {
                scope: source_scope,
                id: expected.memory_id.clone(),
            })
            .await?
        {
            MemoryStoreReadResult::GlobalRecord(Some(record)) => record,
            _ => {
                return Err(MemoryStoreError::new(
                    MemoryStoreErrorKind::ScopeViolation,
                    "derived source missing or inaccessible",
                ))
            }
        };
        let current = CanonicalMemoryRestriction::from_global_record(&record, &scope.tenant)
            .map_err(MemoryStoreError::from)?;
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        if current.source_reference() != expected.source_reference()
            || current.demoted
            || current.expires_at_ms.is_some_and(|expires| expires <= now)
            || current
                .knowledge_scope
                .as_ref()
                .and_then(|policy| policy.retention_expires_at_ms)
                .is_some_and(|expires| expires <= now)
        {
            return Err(MemoryStoreError::new(
                MemoryStoreErrorKind::ScopeViolation,
                "derived source revision changed or inactive",
            ));
        }
        if let (Some(binding), Some(object)) = (
            &current.target.source_binding_id,
            &current.target.source_object_id,
        ) {
            let rows = match store
                .query(MemoryStoreQueryRequest::SourceObjectLifecyclesForBinding {
                    scope: MemoryReadScope::tenant(scope.tenant.clone()),
                    source_binding_id: binding.clone(),
                })
                .await?
            {
                MemoryStoreQueryResult::SourceObjectLifecycles(rows) => rows,
                _ => {
                    return Err(MemoryStoreError::invalid(
                        "unexpected source lifecycle result",
                    ))
                }
            };
            let active = rows.iter().any(|row| {
                row.source_object_id == *object
                    && row.tenant_scope == scope.tenant
                    && row.state == SourceObjectLifecycleState::Active
                    && serde_json::from_value::<tandem_enterprise_contract::ResourceRef>(
                        row.resource_ref.clone(),
                    )
                    .is_ok_and(|resource| resource == current.target.resource_ref)
                    && serde_json::from_value::<tandem_enterprise_contract::DataClass>(
                        serde_json::Value::String(row.data_class.clone()),
                    )
                    .is_ok_and(|class| class == current.target.data_class)
            });
            if !active {
                return Err(MemoryStoreError::new(
                    MemoryStoreErrorKind::ScopeViolation,
                    "derived source lifecycle inactive",
                ));
            }
        }
        if let Some(nested) = &current.nested_lineage {
            let nested_proof = Box::pin(resolve_derived_lineage(store, scope, nested)).await?;
            proof.digests.extend(nested_proof.digests);
        }
    }
    proof
        .digests
        .insert(lineage.digest().map_err(MemoryStoreError::from)?);
    Ok(proof)
}

async fn metadata_is_current<S: MemoryStore + ?Sized>(
    store: &S,
    scope: &MemoryReadScope,
    metadata: Option<&serde_json::Value>,
) -> bool {
    match DerivedMemoryLineage::from_metadata(metadata) {
        Ok(Some(lineage)) => {
            if scope.access == MemoryReadAccess::Scoped
                && (lineage
                    .owner_subject
                    .as_deref()
                    .is_some_and(|owner| scope.subject.as_deref() != Some(owner))
                    || (scope.org_unit.is_some()
                        && lineage
                            .owner_org_unit_id
                            .as_ref()
                            .is_some_and(|unit| scope.org_unit.as_ref() != Some(unit))))
            {
                return false;
            }
            resolve_derived_lineage(store, scope, &lineage)
                .await
                .is_ok()
        }
        Ok(None) => true,
        Err(_) => false,
    }
}

pub(crate) async fn filter_read_result<S: MemoryStore + ?Sized>(
    store: &S,
    scope: &MemoryReadScope,
    result: MemoryStoreReadResult,
) -> MemoryStoreResult<MemoryStoreReadResult> {
    match result {
        MemoryStoreReadResult::GlobalRecord(Some(record)) => {
            let valid = metadata_is_current(store, scope, record.metadata.as_ref()).await;
            Ok(MemoryStoreReadResult::GlobalRecord(valid.then_some(record)))
        }
        MemoryStoreReadResult::Chunks(chunks) => {
            let mut visible = Vec::with_capacity(chunks.len());
            for chunk in chunks {
                if metadata_is_current(store, scope, chunk.metadata.as_ref()).await {
                    visible.push(chunk);
                }
            }
            Ok(MemoryStoreReadResult::Chunks(visible))
        }
        other => Ok(other),
    }
}

pub(crate) async fn filter_query_result<S: MemoryStore + ?Sized>(
    store: &S,
    scope: &MemoryReadScope,
    result: MemoryStoreQueryResult,
) -> MemoryStoreResult<MemoryStoreQueryResult> {
    match result {
        MemoryStoreQueryResult::GlobalRecords(records) => {
            let mut visible = Vec::with_capacity(records.len());
            for record in records {
                if metadata_is_current(store, scope, record.metadata.as_ref()).await {
                    visible.push(record);
                }
            }
            Ok(MemoryStoreQueryResult::GlobalRecords(visible))
        }
        MemoryStoreQueryResult::GlobalSearchHits(hits) => {
            let mut visible = Vec::with_capacity(hits.len());
            for hit in hits {
                if metadata_is_current(store, scope, hit.record.metadata.as_ref()).await {
                    visible.push(hit);
                }
            }
            Ok(MemoryStoreQueryResult::GlobalSearchHits(visible))
        }
        MemoryStoreQueryResult::SimilarChunks(hits) => {
            let mut visible = Vec::with_capacity(hits.len());
            for (chunk, score) in hits {
                if metadata_is_current(store, scope, chunk.metadata.as_ref()).await {
                    visible.push((chunk, score));
                }
            }
            Ok(MemoryStoreQueryResult::SimilarChunks(visible))
        }
        other => Ok(other),
    }
}
