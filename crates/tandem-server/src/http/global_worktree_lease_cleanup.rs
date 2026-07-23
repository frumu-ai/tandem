// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[derive(Default)]
pub(in crate::http::global) struct LeaseWorktreeCleanupResult {
    pub(in crate::http::global) cleaned_paths: Vec<String>,
    pub(in crate::http::global) failures: Vec<Value>,
}

pub(in crate::http::global) async fn cleanup_managed_worktrees_for_lease(
    state: &AppState,
    lease_id: &str,
    caller_authority: Option<(&AuthorizedHostEffect, &HostEffectRequest)>,
) -> LeaseWorktreeCleanupResult {
    let records = state
        .managed_worktrees
        .read()
        .await
        .values()
        .filter(|row| {
            row.lease_id.as_deref() == Some(lease_id)
                && caller_authority.is_none_or(|(_, caller_effect)| {
                    row.tenant_context == caller_effect.resource.tenant_context
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut result = LeaseWorktreeCleanupResult::default();
    for record in records {
        if let Some((caller_grant, caller_effect)) = caller_authority {
            if let Err(error) = caller_grant.revalidate(state, caller_effect) {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": error.code(),
                    "authority": "caller",
                }));
                continue;
            }
        }
        let effect = HostEffectRequest::new(
            HostAction::WorktreeCleanup,
            CanonicalHostResource::new(
                "managed_worktree",
                record.key.clone(),
                record.tenant_context.clone(),
            ),
            json!({
                "repository_id": &record.repository_id,
                "repo_root": &record.repo_root,
                "path": &record.path,
                "branch": &record.branch,
                "lease_id": lease_id,
                "cleanup_branch": record.cleanup_branch,
                "reason": "lease_released_or_expired",
            }),
        );
        let grant = match crate::action_authorization::authorize_internal_host_effect(
            state,
            "http.global.cleanup_managed_worktrees_for_lease",
            &effect,
        )
        .await
        {
            Ok(grant) => grant,
            Err(error) => {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": error.code(),
                }));
                continue;
            }
        };
        if let Err(error) = grant.revalidate(state, &effect) {
            result.failures.push(json!({
                "worktree_id": record.key,
                "code": error.code(),
            }));
            continue;
        }
        if let Some((caller_grant, caller_effect)) = caller_authority {
            if let Err(error) = caller_grant.revalidate(state, caller_effect) {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": error.code(),
                    "authority": "caller",
                }));
                continue;
            }
        }
        if crate::runtime::worktrees::validate_managed_worktree_path(
            &record.repo_root,
            StdPath::new(&record.path),
            false,
        )
        .is_err()
        {
            result.failures.push(json!({
                "worktree_id": record.key,
                "code": "WORKTREE_PATH_CONTAINMENT_FAILED",
            }));
            continue;
        }
        let registered_branch = match parse_registered_worktree_entries(&record.repo_root).await {
            Ok(entries) => entries
                .into_iter()
                .find(|entry| entry.path == record.path)
                .and_then(|entry| entry.branch),
            Err(_) => {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": "WORKTREE_REGISTRATION_CHECK_FAILED",
                }));
                continue;
            }
        };
        let worktree_already_removed = registered_branch.is_none()
            && !StdPath::new(&record.path).exists()
            && record.cleanup_branch;
        if !worktree_already_removed && registered_branch.as_deref() != Some(record.branch.as_str())
        {
            result.failures.push(json!({
                "worktree_id": record.key,
                "expected_branch": record.branch,
                "registered_branch": registered_branch,
                "code": "WORKTREE_BRANCH_MISMATCH",
            }));
            continue;
        }
        if !worktree_already_removed {
            if let Some((caller_grant, caller_effect)) = caller_authority {
                if let Err(error) = caller_grant.revalidate(state, caller_effect) {
                    result.failures.push(json!({
                        "worktree_id": record.key,
                        "code": error.code(),
                        "authority": "caller",
                    }));
                    continue;
                }
            }
            if let Err(error) = grant.revalidate(state, &effect) {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": error.code(),
                }));
                continue;
            }
            let output = match crate::runtime::worktrees::run_managed_git(
                &record.repo_root,
                &["worktree", "remove", "--", &record.path],
            )
            .await
            {
                Ok(output) => output,
                Err(_) => {
                    result.failures.push(json!({
                        "path": record.path,
                        "branch": record.branch,
                        "repo_root": record.repo_root,
                        "code": "WORKTREE_REMOVE_FAILED",
                    }));
                    continue;
                }
            };
            if !output.success {
                result.failures.push(json!({
                    "path": record.path,
                    "branch": record.branch,
                    "repo_root": record.repo_root,
                    "code": "WORKTREE_REMOVE_FAILED",
                    "stderr": output.stderr.clone(),
                }));
                continue;
            }
        }
        let mut branch_cleanup_complete = true;
        if record.cleanup_branch {
            if let Some((caller_grant, caller_effect)) = caller_authority {
                if let Err(error) = caller_grant.revalidate(state, caller_effect) {
                    result.failures.push(json!({
                        "worktree_id": record.key,
                        "code": error.code(),
                        "authority": "caller",
                    }));
                    continue;
                }
            }
            if let Err(error) = grant.revalidate(state, &effect) {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": error.code(),
                }));
                continue;
            }
            match crate::runtime::worktrees::run_managed_git(
                &record.repo_root,
                &["branch", "-D", "--", &record.branch],
            )
            .await
            {
                Ok(branch_output) if branch_output.success => {}
                Ok(branch_output) => {
                    branch_cleanup_complete = false;
                    result.failures.push(json!({
                        "path": record.path,
                        "branch": record.branch,
                        "repo_root": record.repo_root,
                        "code": "WORKTREE_BRANCH_DELETE_FAILED",
                        "stderr": branch_output.stderr.clone(),
                    }));
                }
                Err(_) => {
                    branch_cleanup_complete = false;
                    result.failures.push(json!({
                        "path": record.path,
                        "branch": record.branch,
                        "repo_root": record.repo_root,
                        "code": "WORKTREE_BRANCH_DELETE_FAILED",
                    }));
                }
            }
        }
        if !branch_cleanup_complete {
            continue;
        }
        let mut managed_worktrees = state.managed_worktrees.write().await;
        if let Some((caller_grant, caller_effect)) = caller_authority {
            if let Err(error) = caller_grant.revalidate(state, caller_effect) {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": error.code(),
                    "authority": "caller",
                }));
                continue;
            }
        }
        if let Err(error) = grant.revalidate(state, &effect) {
            result.failures.push(json!({
                "worktree_id": record.key,
                "code": error.code(),
            }));
            continue;
        }
        if !remove_authorized_cleanup_record(&mut managed_worktrees, &record) {
            result.failures.push(json!({
                "worktree_id": record.key,
                "code": "WORKTREE_RECORD_CHANGED",
            }));
            continue;
        }
        result.cleaned_paths.push(record.path);
    }
    result
}

fn remove_authorized_cleanup_record(
    records: &mut std::collections::HashMap<String, crate::ManagedWorktreeRecord>,
    authorized: &crate::ManagedWorktreeRecord,
) -> bool {
    records.remove(&authorized.key).is_some()
}

#[cfg(test)]
mod cleanup_record_tests {
    use super::*;

    fn record(key: &str, lease_id: &str) -> crate::ManagedWorktreeRecord {
        crate::ManagedWorktreeRecord {
            key: key.to_string(),
            repo_root: "/repo".to_string(),
            repository_id: Some("repo-1".to_string()),
            tenant_context: TenantContext::local_implicit(),
            path: "/repo/.tandem/worktrees/shared".to_string(),
            branch: format!("tandem/{key}"),
            base: "HEAD".to_string(),
            managed: true,
            task_id: Some("task".to_string()),
            owner_run_id: Some("run".to_string()),
            lease_id: Some(lease_id.to_string()),
            cleanup_branch: true,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    #[test]
    fn cleanup_removes_only_the_exact_authorized_record() {
        let authorized = record("authorized", "lease-old");
        let replacement = record("replacement", "lease-new");
        let mut records = std::collections::HashMap::from([
            (authorized.key.clone(), authorized.clone()),
            (replacement.key.clone(), replacement.clone()),
        ]);

        assert!(remove_authorized_cleanup_record(&mut records, &authorized));
        assert!(!records.contains_key(&authorized.key));
        assert_eq!(records.get(&replacement.key), Some(&replacement));
    }
}
