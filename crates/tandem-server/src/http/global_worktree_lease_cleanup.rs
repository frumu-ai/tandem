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
        managed_worktrees
            .retain(|_, row| !(row.repo_root == record.repo_root && row.path == record.path));
        result.cleaned_paths.push(record.path);
    }
    result
}
