// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Durable runtime event log — an **observability-only, tenant-scoped** ledger of
//! verbatim engine/tool events (TAN-650, decision recorded).
//!
//! ## Scope decision
//!
//! This log persists verbatim tool output. It is deliberately **not** an
//! agent-reachable retrieval source: nothing feeds these rows into the LLM
//! memory/context/prompt path (`RuntimeEventLogRow` / `query_runtime_event_log`
//! have no non-test, non-operator callers; `state.runtime_events_path` is only
//! consumed to derive a separate reliability ledger, not the payloads). It is an
//! operator/debug artifact surfaced through the tenant-scoped run-debugger/SSE
//! view.
//!
//! Because it is not a hot cross-tenant/department leak into the agent, the
//! envelope is scoped by **tenant only** — it carries no `subject` / department
//! (`owner_org_unit_id`) dimension, and [`RuntimeEventLogRow::visible_to_tenant`]
//! is the single read gate. At-rest exposure of the raw payloads (a disk dump is
//! unscoped by subject/department) is covered by the at-rest strategy (TAN-663
//! FDE / TAN-666 envelope encryption), not by an event-schema change.
//!
//! ## Invariant for future readers
//!
//! If a reader is ever added that exposes these rows beyond the tenant-scoped
//! operator view — especially anything agent-reachable — it MUST first extend the
//! [`RuntimeEvent`] envelope with `subject` (+ `owner_org_unit_id`) and narrow
//! [`RuntimeEventLogRow::visible_to_tenant`] accordingly, so a payload written
//! under one subject/department is not returned to another. Until then, every
//! read goes through the tenant gate below.

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tandem_types::{EngineEvent, RuntimeEvent, TenantContext};

#[path = "runtime_event_store.rs"]
mod runtime_event_store;
use runtime_event_store::RuntimeEventStore;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEventLogRow {
    #[serde(flatten)]
    pub event: RuntimeEvent,
}

impl<'de> Deserialize<'de> for RuntimeEventLogRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RuntimeEvent::deserialize(deserializer).map(|event| Self { event })
    }
}

impl RuntimeEventLogRow {
    pub fn from_engine_event(event: &EngineEvent) -> Option<Self> {
        let event = RuntimeEvent::from_engine_event(event)?;
        if event.envelope.run_id.is_none() && event.envelope.session_id.is_none() {
            return None;
        }
        Some(Self { event })
    }

    pub fn event_id(&self) -> &str {
        &self.event.envelope.event_id
    }

    pub fn seq(&self) -> u64 {
        self.event.envelope.seq
    }

    pub fn run_id(&self) -> Option<&str> {
        self.event.envelope.run_id.as_deref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.event.envelope.session_id.as_deref()
    }

    pub fn occurred_at_ms(&self) -> u64 {
        self.event.envelope.occurred_at_ms
    }

    pub fn tenant_context(&self) -> Option<&TenantContext> {
        self.event.envelope.tenant_context.as_ref()
    }

    /// The single read gate for event-log rows (TAN-650). Tenant-scoped by
    /// design: in local single-user mode (`is_local_implicit`) everything is
    /// visible; otherwise the row's tenant must match exactly. There is
    /// deliberately **no** subject/department dimension here — see the module
    /// docs before adding a reader that would need one.
    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        if tenant.is_local_implicit() {
            return true;
        }
        let Some(event_tenant) = self.tenant_context() else {
            return false;
        };
        event_tenant.org_id == tenant.org_id
            && event_tenant.workspace_id == tenant.workspace_id
            && event_tenant.deployment_id == tenant.deployment_id
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeEventLogQuery<'a> {
    pub run_id: &'a str,
    pub after_seq: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeEventLogWindowQuery<'a> {
    pub run_id: &'a str,
    pub after_seq: Option<u64>,
    pub before_seq: Option<u64>,
    pub limit: Option<usize>,
    pub tail: Option<usize>,
}

pub async fn append_runtime_event_log_row(
    path: &Path,
    row: &RuntimeEventLogRow,
) -> anyhow::Result<()> {
    let store = RuntimeEventStore::from_events_path(path);
    let legacy_path = path.to_path_buf();
    let row = row.clone();
    tokio::task::spawn_blocking(move || store.append(&legacy_path, &row))
        .await
        .context("runtime event store append task failed")?
}

pub fn load_runtime_event_log_rows(path: &Path) -> Vec<RuntimeEventLogRow> {
    RuntimeEventStore::from_events_path(path)
        .load_all(path)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to load transactional runtime event log");
            Vec::new()
        })
}

pub fn query_runtime_event_log(
    path: &Path,
    tenant: &TenantContext,
    query: RuntimeEventLogQuery<'_>,
) -> Vec<RuntimeEventLogRow> {
    query_runtime_event_log_window(
        path,
        tenant,
        RuntimeEventLogWindowQuery {
            run_id: query.run_id,
            after_seq: query.after_seq,
            before_seq: None,
            limit: query.limit,
            tail: None,
        },
    )
}

pub fn query_runtime_event_log_window(
    path: &Path,
    tenant: &TenantContext,
    query: RuntimeEventLogWindowQuery<'_>,
) -> Vec<RuntimeEventLogRow> {
    RuntimeEventStore::from_events_path(path)
        .query(path, tenant, query)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to query transactional runtime event log");
            Vec::new()
        })
}

pub async fn prune_runtime_event_log(
    path: &Path,
    retention_ms: u64,
    now_ms: u64,
) -> anyhow::Result<usize> {
    if retention_ms == 0 {
        return Ok(0);
    }
    let cutoff_ms = now_ms.saturating_sub(retention_ms);
    let store = RuntimeEventStore::from_events_path(path);
    let legacy_path = path.to_path_buf();
    tokio::task::spawn_blocking(move || store.prune(&legacy_path, cutoff_ms))
        .await
        .context("runtime event store retention task failed")?
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use serde_json::json;
    use tandem_types::{EngineEvent, RuntimeEventEnvelope, TenantContext};
    use tokio::io::AsyncWriteExt;
    use uuid::Uuid;

    use super::*;

    fn event(
        seq: u64,
        run_id: &str,
        tenant_context: Option<TenantContext>,
        occurred_at_ms: u64,
    ) -> EngineEvent {
        EngineEvent::new(
            "session.run.started",
            json!({
                "runID": run_id,
                "sessionID": "session-a",
                "tenantContext": tenant_context,
            }),
        )
        .with_envelope(RuntimeEventEnvelope {
            event_id: format!("evt-{seq}"),
            seq,
            schema_version: 1,
            occurred_at_ms,
            session_id: Some("session-a".to_string()),
            run_id: Some(run_id.to_string()),
            node_id: None,
            tenant_context,
        })
    }

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    async fn remove_runtime_event_store(path: &Path) {
        let database_path = path.with_extension("sqlite3");
        let sidecars = [
            path.to_path_buf(),
            database_path.clone(),
            PathBuf::from(format!("{}-wal", database_path.display())),
            PathBuf::from(format!("{}-shm", database_path.display())),
        ];
        for path in sidecars {
            let _ = tokio::fs::remove_file(path).await;
        }
    }

    #[test]
    fn visible_to_tenant_is_tenant_scoped_contract() {
        // TAN-650: lock the event-log read gate's contract. Local single-user
        // mode sees everything; cross-tenant and missing-tenant rows are denied.
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let row_a =
            RuntimeEventLogRow::from_engine_event(&event(1, "run-a", Some(tenant_a.clone()), 100))
                .expect("canonical row");
        let row_untenanted =
            RuntimeEventLogRow::from_engine_event(&event(2, "run-a", None, 200)).expect("row");

        // Same tenant: visible.
        assert!(row_a.visible_to_tenant(&tenant_a));
        // Different tenant: denied (no cross-tenant leak).
        assert!(!row_a.visible_to_tenant(&tenant_b));
        // Row without a tenant context is denied to any explicit tenant.
        assert!(!row_untenanted.visible_to_tenant(&tenant_a));
        // Local single-user mode sees everything (by design, documented).
        assert!(row_a.visible_to_tenant(&TenantContext::local_implicit()));
        assert!(row_untenanted.visible_to_tenant(&TenantContext::local_implicit()));
    }

    #[tokio::test]
    async fn query_filters_by_run_sequence_and_tenant() {
        let path = std::env::temp_dir().join(format!("runtime-events-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        for event in [
            event(1, "run-a", Some(tenant_a.clone()), 100),
            event(2, "run-b", Some(tenant_a.clone()), 200),
            event(3, "run-a", Some(tenant_b.clone()), 300),
            event(4, "run-a", Some(tenant_a.clone()), 400),
        ] {
            let row = RuntimeEventLogRow::from_engine_event(&event).expect("canonical row");
            append_runtime_event_log_row(&path, &row)
                .await
                .expect("append");
        }

        let rows = query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: Some(1),
                limit: None,
            },
        );

        assert_eq!(
            rows.iter().map(RuntimeEventLogRow::seq).collect::<Vec<_>>(),
            vec![4]
        );
        remove_runtime_event_store(&path).await;
    }

    #[tokio::test]
    async fn query_supports_tail_and_before_sequence_pages() {
        let path =
            std::env::temp_dir().join(format!("runtime-events-tail-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        for seq in 1..=6 {
            let row = RuntimeEventLogRow::from_engine_event(&event(
                seq,
                "run-a",
                Some(tenant_a.clone()),
                100 + seq,
            ))
            .expect("canonical row");
            append_runtime_event_log_row(&path, &row)
                .await
                .expect("append");
        }

        let tail = query_runtime_event_log_window(
            &path,
            &tenant_a,
            RuntimeEventLogWindowQuery {
                run_id: "run-a",
                after_seq: None,
                before_seq: None,
                limit: Some(2),
                tail: Some(2),
            },
        );
        assert_eq!(
            tail.iter().map(RuntimeEventLogRow::seq).collect::<Vec<_>>(),
            vec![5, 6]
        );

        let previous = query_runtime_event_log_window(
            &path,
            &tenant_a,
            RuntimeEventLogWindowQuery {
                run_id: "run-a",
                after_seq: None,
                before_seq: Some(5),
                limit: None,
                tail: Some(2),
            },
        );
        assert_eq!(
            previous
                .iter()
                .map(RuntimeEventLogRow::seq)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );

        remove_runtime_event_store(&path).await;
    }

    #[tokio::test]
    async fn prune_removes_rows_older_than_retention_window() {
        let path = std::env::temp_dir().join(format!("runtime-events-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        for event in [
            event(1, "run-a", Some(tenant_a.clone()), 100),
            event(2, "run-a", Some(tenant_a), 900),
        ] {
            let row = RuntimeEventLogRow::from_engine_event(&event).expect("canonical row");
            append_runtime_event_log_row(&path, &row)
                .await
                .expect("append");
        }

        let pruned = prune_runtime_event_log(&path, 500, 1_000)
            .await
            .expect("prune");

        assert_eq!(pruned, 1);
        let rows = load_runtime_event_log_rows(&path);
        assert_eq!(
            rows.iter().map(RuntimeEventLogRow::seq).collect::<Vec<_>>(),
            vec![2]
        );
        remove_runtime_event_store(&path).await;
    }

    #[tokio::test]
    async fn legacy_import_is_once_only_and_preserves_the_source_file() {
        let path =
            std::env::temp_dir().join(format!("runtime-events-import-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        let source_rows = [
            RuntimeEventLogRow::from_engine_event(&event(1, "run-a", Some(tenant_a.clone()), 100))
                .expect("first source row"),
            RuntimeEventLogRow::from_engine_event(&event(2, "run-a", Some(tenant_a.clone()), 200))
                .expect("second source row"),
        ];
        let source = format!(
            "{}\nnot-valid-json\n{}\n",
            serde_json::to_string(&source_rows[0]).expect("serialize first source row"),
            serde_json::to_string(&source_rows[1]).expect("serialize second source row"),
        );
        tokio::fs::write(&path, &source)
            .await
            .expect("write legacy event log");

        let imported = query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: None,
                limit: None,
            },
        );
        assert_eq!(
            imported
                .iter()
                .map(RuntimeEventLogRow::seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            tokio::fs::read_to_string(&path)
                .await
                .expect("read legacy source"),
            source
        );

        let later_legacy_row =
            RuntimeEventLogRow::from_engine_event(&event(3, "run-a", Some(tenant_a.clone()), 300))
                .expect("later legacy row");
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .await
            .expect("open legacy source for external edit")
            .write_all(
                format!(
                    "{}\n",
                    serde_json::to_string(&later_legacy_row).expect("serialize later legacy row")
                )
                .as_bytes(),
            )
            .await
            .expect("append externally edited legacy row");

        let after_external_edit = query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: None,
                limit: None,
            },
        );
        assert_eq!(
            after_external_edit
                .iter()
                .map(RuntimeEventLogRow::seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );

        append_runtime_event_log_row(&path, &later_legacy_row)
            .await
            .expect("append authoritative event");
        let authoritative_rows = query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: Some(1),
                limit: None,
            },
        );
        assert_eq!(
            authoritative_rows
                .iter()
                .map(RuntimeEventLogRow::seq)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        remove_runtime_event_store(&path).await;
    }

    #[tokio::test]
    async fn unreadable_legacy_source_is_not_marked_imported() {
        let path = std::env::temp_dir().join(format!(
            "runtime-events-unreadable-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        tokio::fs::write(&path, [0xff])
            .await
            .expect("write invalid UTF-8 source");

        assert!(query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: None,
                limit: None
            },
        )
        .is_empty());

        let row =
            RuntimeEventLogRow::from_engine_event(&event(1, "run-a", Some(tenant_a.clone()), 100))
                .expect("canonical row");
        tokio::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string(&row).expect("serialize source row")
            ),
        )
        .await
        .expect("repair source");
        assert_eq!(
            query_runtime_event_log(
                &path,
                &tenant_a,
                RuntimeEventLogQuery {
                    run_id: "run-a",
                    after_seq: None,
                    limit: None
                },
            )
            .iter()
            .map(RuntimeEventLogRow::seq)
            .collect::<Vec<_>>(),
            vec![1]
        );
        remove_runtime_event_store(&path).await;
    }

    #[tokio::test]
    async fn concurrent_replay_is_idempotent_and_preserves_cursor_pages() {
        let path =
            std::env::temp_dir().join(format!("runtime-events-cursor-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        let rows = (1..=16)
            .map(|seq| {
                RuntimeEventLogRow::from_engine_event(&event(
                    seq,
                    "run-a",
                    Some(tenant_a.clone()),
                    100 + seq,
                ))
                .expect("canonical row")
            })
            .collect::<Vec<_>>();

        let mut appends = Vec::new();
        for row in rows.iter().cloned().chain(rows.iter().cloned()) {
            let path = path.clone();
            appends.push(tokio::spawn(async move {
                append_runtime_event_log_row(&path, &row).await
            }));
        }
        for append in appends {
            append
                .await
                .expect("append task joined")
                .expect("idempotent append succeeded");
        }

        let first_page = query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: None,
                limit: Some(8),
            },
        );
        assert_eq!(
            first_page
                .iter()
                .map(RuntimeEventLogRow::seq)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );
        let second_page = query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: first_page.last().map(RuntimeEventLogRow::seq),
                limit: Some(8),
            },
        );
        assert_eq!(
            second_page
                .iter()
                .map(RuntimeEventLogRow::seq)
                .collect::<Vec<_>>(),
            (9..=16).collect::<Vec<_>>()
        );
        remove_runtime_event_store(&path).await;
    }

    #[test]
    #[ignore = "run explicitly to benchmark the 1M-row transactional event ledger"]
    fn million_row_query_and_retention_benchmark() {
        const EVENT_COUNT: u64 = 1_000_000;
        let path =
            std::env::temp_dir().join(format!("runtime-events-benchmark-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        let store = RuntimeEventStore::from_events_path(&path);
        let seed_started = Instant::now();
        store
            .append_rows_for_benchmark(
                &path,
                (1..=EVENT_COUNT).map(|seq| {
                    RuntimeEventLogRow::from_engine_event(&event(
                        seq,
                        if seq % 1_000 == 0 {
                            "run-target"
                        } else {
                            "run-other"
                        },
                        Some(tenant_a.clone()),
                        seq,
                    ))
                    .expect("benchmark row")
                }),
            )
            .expect("seed benchmark event ledger");
        let query_started = Instant::now();
        let rows = query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-target",
                after_seq: None,
                limit: Some(100),
            },
        );
        let query_elapsed = query_started.elapsed();
        assert_eq!(rows.len(), 100);

        let retention_started = Instant::now();
        let pruned = store
            .prune(&path, EVENT_COUNT / 2)
            .expect("prune benchmark ledger");
        let retention_elapsed = retention_started.elapsed();
        assert_eq!(pruned, EVENT_COUNT as usize / 2 - 1);
        eprintln!(
            "runtime-event benchmark rows={EVENT_COUNT} seed_ms={} query_ms={} retention_ms={}",
            seed_started.elapsed().as_millis(),
            query_elapsed.as_millis(),
            retention_elapsed.as_millis(),
        );
        std::fs::remove_file(path.with_extension("sqlite3")).ok();
    }
}
