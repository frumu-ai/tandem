// TAN-638: coder memory candidate GC + tenant-scoping tests.

/// Delete coder memory candidate JSON files older than `retention_days` across
/// every coder run (TAN-638). These plain-JSON files have no reaper otherwise
/// and accumulate for the life of the deployment. `0` disables GC. Best-effort:
/// unreadable/unparseable files are skipped. Returns the number of files deleted.
pub(crate) async fn reap_coder_memory_candidates(state: &AppState, retention_days: u64) -> u64 {
    if retention_days == 0 {
        return 0;
    }
    let root = coder_runs_root(state);
    if !root.exists() {
        return 0;
    }
    let cutoff_ms =
        crate::now_ms().saturating_sub(retention_days.saturating_mul(24 * 60 * 60 * 1000));
    let mut deleted = 0u64;
    let Ok(mut dir) = tokio::fs::read_dir(&root).await else {
        return 0;
    };
    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry
            .file_type()
            .await
            .map(|row| row.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(raw) = tokio::fs::read_to_string(entry.path()).await else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<CoderRunRecord>(&raw) else {
            continue;
        };
        let candidates_dir = coder_memory_candidates_dir(state, &record.linked_context_run_id);
        if !candidates_dir.exists() {
            continue;
        }
        let Ok(mut candidate_dir) = tokio::fs::read_dir(&candidates_dir).await else {
            continue;
        };
        while let Ok(Some(candidate_entry)) = candidate_dir.next_entry().await {
            if !candidate_entry
                .file_type()
                .await
                .map(|row| row.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let path = candidate_entry.path();
            let created_at_ms = tokio::fs::read_to_string(&path)
                .await
                .ok()
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .and_then(|payload| payload.get("created_at_ms").and_then(Value::as_u64));
            let Some(created_at_ms) = created_at_ms else {
                continue;
            };
            if created_at_ms < cutoff_ms && tokio::fs::remove_file(&path).await.is_ok() {
                deleted += 1;
            }
        }
    }
    if deleted > 0 {
        tracing::info!(
            retention_days,
            deleted,
            "coder memory candidate GC: reaped old candidates"
        );
    }
    deleted
}

#[cfg(test)]
mod coder_memory_candidate_scoping_tests {
    use super::*;
    use crate::test_support::test_state;
    use tandem_types::TenantContext;

    fn repo_binding() -> CoderRepoBinding {
        CoderRepoBinding {
            project_id: "proj-shared".to_string(),
            workspace_id: "ws-shared".to_string(),
            workspace_root: "/tmp/tandem-shared".to_string(),
            repo_slug: "org/shared".to_string(),
            default_branch: Some("main".to_string()),
        }
    }

    fn coder_run_record(suffix: &str) -> CoderRunRecord {
        CoderRunRecord {
            coder_run_id: format!("coder-{suffix}"),
            workflow_mode: CoderWorkflowMode::IssueFix,
            linked_context_run_id: format!("ctx-{suffix}"),
            repo_binding: repo_binding(),
            github_ref: None,
            source_client: None,
            model_provider: None,
            model_id: None,
            parent_coder_run_id: None,
            origin: None,
            origin_artifact_type: None,
            origin_policy: None,
            github_project_ref: None,
            remote_sync_state: None,
            worker_session_id: None,
            worker_run_id: None,
            managed_worktree: None,
            branch_name: None,
            commit_sha: None,
            pr_url: None,
            changed_files: None,
            validation_status: None,
            handoff_status: None,
            completion_gate: None,
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
        }
    }

    fn context_run_state(suffix: &str, tenant: &TenantContext) -> ContextRunState {
        ContextRunState {
            run_id: format!("ctx-{suffix}"),
            run_type: "coder".to_string(),
            tenant_context: tenant.clone(),
            source_client: None,
            model_provider: None,
            model_id: None,
            mcp_servers: Vec::new(),
            status: ContextRunStatus::Completed,
            objective: "seed".to_string(),
            workspace: ContextWorkspaceLease {
                workspace_id: "ws-shared".to_string(),
                canonical_path: "/tmp/tandem-shared".to_string(),
                lease_epoch: 1,
            },
            steps: Vec::new(),
            tasks: Vec::new(),
            why_next_step: None,
            revision: 1,
            last_event_seq: 0,
            created_at_ms: 1,
            started_at_ms: Some(1),
            ended_at_ms: Some(2),
            last_error: None,
            updated_at_ms: 2,
        }
    }

    /// Seed a coder run (record + context run state) owned by `tenant` with a
    /// single candidate carrying `summary`, `created_at_ms` and (optionally) a
    /// stamped tenant. Returns the candidate file path.
    async fn seed_candidate(
        state: &AppState,
        suffix: &str,
        tenant: &TenantContext,
        summary: &str,
        created_at_ms: u64,
    ) -> std::path::PathBuf {
        let record = coder_run_record(suffix);
        save_coder_run_record(state, &record).await.expect("save record");
        save_context_run_state(state, &context_run_state(suffix, tenant))
            .await
            .expect("save context run");
        let candidate_id = format!("memcand-{suffix}");
        let path =
            coder_memory_candidate_path(state, &record.linked_context_run_id, &candidate_id);
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .expect("candidate dir");
        let payload = json!({
            "candidate_id": candidate_id,
            "coder_run_id": record.coder_run_id,
            "linked_context_run_id": record.linked_context_run_id,
            "workflow_mode": record.workflow_mode,
            "kind": "run_outcome",
            "summary": summary,
            "payload": {},
            "repo_binding": record.repo_binding,
            "tenant_context": tenant,
            "created_at_ms": created_at_ms,
        });
        tokio::fs::write(&path, serde_json::to_string(&payload).unwrap())
            .await
            .expect("write candidate");
        path
    }

    fn summaries(hits: &[Value]) -> Vec<String> {
        let mut out: Vec<String> = hits
            .iter()
            .filter_map(|hit| hit.get("summary").and_then(Value::as_str))
            .map(ToString::to_string)
            .collect();
        out.sort();
        out
    }

    #[tokio::test]
    async fn candidate_retrieval_cannot_cross_tenant_boundaries() {
        let state = test_state().await;
        let tenant_a = TenantContext::explicit("org-a", "ws-a", None);
        let tenant_b = TenantContext::explicit("org-b", "ws-b", None);
        let now = crate::now_ms();

        seed_candidate(&state, "a", &tenant_a, "tenant-a-secret", now).await;
        seed_candidate(&state, "b", &tenant_b, "tenant-b-secret", now).await;

        // Same repo_slug in both tenants, but each caller only sees its own.
        let a_hits =
            list_repo_memory_candidates(&state, "org/shared", None, 20, Some(&tenant_a))
                .await
                .expect("list a");
        assert_eq!(summaries(&a_hits), vec!["tenant-a-secret".to_string()]);

        let b_hits =
            list_repo_memory_candidates(&state, "org/shared", None, 20, Some(&tenant_b))
                .await
                .expect("list b");
        assert_eq!(summaries(&b_hits), vec!["tenant-b-secret".to_string()]);

        // Local/system scope (None) is not a tenant boundary and sees both.
        let all_hits = list_repo_memory_candidates(&state, "org/shared", None, 20, None)
            .await
            .expect("list all");
        assert_eq!(
            summaries(&all_hits),
            vec!["tenant-a-secret".to_string(), "tenant-b-secret".to_string()]
        );
    }

    #[tokio::test]
    async fn promoted_candidates_are_excluded_from_retrieval_but_retained_on_disk() {
        let state = test_state().await;
        let tenant = TenantContext::explicit("org-a", "ws-a", None);
        let now = crate::now_ms();

        let unpromoted = seed_candidate(&state, "open", &tenant, "still-open", now).await;
        let promoted_path = seed_candidate(&state, "done", &tenant, "already-promoted", now).await;

        // Mark the second candidate as promoted, mirroring the promotion path.
        let mut payload: Value =
            serde_json::from_str(&tokio::fs::read_to_string(&promoted_path).await.unwrap())
                .unwrap();
        payload
            .as_object_mut()
            .unwrap()
            .insert("promoted_at_ms".to_string(), json!(now));
        tokio::fs::write(&promoted_path, serde_json::to_string(&payload).unwrap())
            .await
            .unwrap();

        let hits = list_repo_memory_candidates(&state, "org/shared", None, 20, Some(&tenant))
            .await
            .expect("list");
        assert_eq!(summaries(&hits), vec!["still-open".to_string()]);
        // The promoted candidate file is retained as provenance for the record.
        assert!(promoted_path.exists());
        assert!(unpromoted.exists());
    }

    #[tokio::test]
    async fn gc_reaps_candidates_older_than_retention() {
        let state = test_state().await;
        let tenant = TenantContext::explicit("org-a", "ws-a", None);
        let now = crate::now_ms();
        let old_ms = now.saturating_sub(120 * 24 * 60 * 60 * 1000);

        let fresh = seed_candidate(&state, "fresh", &tenant, "fresh-summary", now).await;
        let stale = seed_candidate(&state, "stale", &tenant, "stale-summary", old_ms).await;

        // 0 disables GC entirely.
        assert_eq!(reap_coder_memory_candidates(&state, 0).await, 0);
        assert!(stale.exists());

        // 90-day retention reaps only the 120-day-old candidate.
        let deleted = reap_coder_memory_candidates(&state, 90).await;
        assert_eq!(deleted, 1);
        assert!(!stale.exists());
        assert!(fresh.exists());
    }
}
