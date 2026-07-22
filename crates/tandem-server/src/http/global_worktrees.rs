// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

pub(in crate::http) async fn create_worktree(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    if input.managed == Some(false) {
        return Err(StatusCode::FORBIDDEN);
    }
    let (resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        input.repository_id.as_deref(),
        input.repo_root.as_deref(),
    )
    .await?;
    if verified.is_some()
        && input
            .lease_id
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        return Err(StatusCode::CONFLICT);
    }
    let managed = true;
    let base = input.base.clone().unwrap_or_else(|| "HEAD".to_string());
    if base.trim_start().starts_with('-') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let slug = crate::runtime::worktrees::managed_worktree_slug(
        input.task_id.as_deref(),
        input.owner_run_id.as_deref(),
        input.lease_id.as_deref(),
        input.branch.as_deref(),
    );
    let default_path = PathBuf::from(&repo_candidate)
        .join(".tandem")
        .join("worktrees")
        .join(&slug);
    let path = resolve_worktree_path(&repo_candidate, input.path.as_deref(), &default_path)?;
    if !is_within_managed_worktree_root(&repo_candidate, &path) {
        return Err(StatusCode::CONFLICT);
    }
    let branch = input
        .branch
        .clone()
        .unwrap_or_else(|| format!("tandem/{slug}"));
    if branch.trim_start().starts_with('-') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let cleanup_branch = input.cleanup_branch.unwrap_or(true);
    let lease =
        validate_managed_worktree_lease(&state, true, input.lease_id.as_deref(), &tenant).await?;
    let path_string = path.to_string_lossy().to_string();
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeCreate,
        resource,
        json!({
            "repository_candidate": repo_candidate,
            "path": path_string,
            "branch": branch,
            "base": base,
            "lease_id": &input.lease_id,
            "cleanup_branch": cleanup_branch,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate).await?;
    let key = crate::runtime::worktrees::managed_worktree_key(
        &repo_root,
        input.task_id.as_deref(),
        input.owner_run_id.as_deref(),
        input.lease_id.as_deref(),
        &path_string,
        &branch,
    );
    let worktree_id = key.clone();
    let expose_host_paths = verified.is_none();
    if let Some(existing) = state.managed_worktrees.read().await.get(&key).cloned() {
        crate::http::sessions_actor_scope::ensure_same_session_actor(
            &tenant,
            &existing.tenant_context,
        )?;
        if existing.repository_id != input.repository_id {
            return Err(StatusCode::NOT_FOUND);
        }
        grant
            .revalidate(&state, &effect)
            .map_err(host_authorization_status)?;
        if worktree_is_registered(&repo_root, &existing.path).await? {
            return Ok(Json(json!({
                "ok": true,
                "worktree_id": existing.key,
                "repository_id": existing.repository_id,
                "repo_root": expose_host_paths.then_some(existing.repo_root),
                "path": expose_host_paths.then_some(existing.path),
                "branch": existing.branch,
                "base": existing.base,
                "managed": existing.managed,
                "task_id": existing.task_id,
                "owner_run_id": existing.owner_run_id,
                "lease_id": existing.lease_id,
                "lease_client_id": lease.as_ref().map(|row| row.client_id.clone()),
                "lease_client_type": lease.as_ref().map(|row| row.client_type.clone()),
                "cleanup_branch": existing.cleanup_branch,
                "reused": true,
            })));
        }
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    crate::runtime::worktrees::validate_managed_worktree_path(&repo_root, &path, true)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    if path.exists() && !worktree_is_registered(&repo_root, &path_string).await? {
        return Ok(Json(json!({
            "ok": false,
            "worktree_id": worktree_id,
            "repository_id": input.repository_id,
            "repo_root": expose_host_paths.then_some(repo_root),
            "path": expose_host_paths.then_some(path_string),
            "branch": branch,
            "base": base,
            "managed": managed,
            "error": "target path already exists but is not a registered worktree",
            "code": "WORKTREE_PATH_CONFLICT",
        })));
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    if worktree_is_registered(&repo_root, &path_string).await? {
        let now = crate::now_ms();
        state.managed_worktrees.write().await.insert(
            key.clone(),
            crate::ManagedWorktreeRecord {
                key: crate::runtime::worktrees::managed_worktree_key(
                    &repo_root,
                    input.task_id.as_deref(),
                    input.owner_run_id.as_deref(),
                    input.lease_id.as_deref(),
                    &path_string,
                    &branch,
                ),
                repo_root: repo_root.clone(),
                repository_id: input.repository_id.clone(),
                tenant_context: tenant.clone(),
                path: path_string.clone(),
                branch: branch.clone(),
                base: base.clone(),
                managed,
                task_id: input.task_id,
                owner_run_id: input.owner_run_id,
                lease_id: input.lease_id,
                cleanup_branch,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
        return Ok(Json(json!({
            "ok": true,
            "worktree_id": worktree_id,
            "repository_id": input.repository_id,
            "repo_root": expose_host_paths.then_some(repo_root),
            "path": expose_host_paths.then_some(path_string),
            "branch": branch,
            "base": base,
            "managed": managed,
            "cleanup_branch": cleanup_branch,
            "lease_client_id": lease.as_ref().map(|row| row.client_id.clone()),
            "lease_client_type": lease.as_ref().map(|row| row.client_type.clone()),
            "reused": true,
        })));
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    crate::runtime::worktrees::validate_managed_worktree_path(&repo_root, &path, true)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    let output = crate::runtime::worktrees::run_managed_git(
        &repo_root,
        &["worktree", "add", "-b", &branch, &path_string, "--", &base],
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let ok = output.success;
    if ok {
        let now = crate::now_ms();
        state.managed_worktrees.write().await.insert(
            key.clone(),
            crate::ManagedWorktreeRecord {
                key: crate::runtime::worktrees::managed_worktree_key(
                    &repo_root,
                    input.task_id.as_deref(),
                    input.owner_run_id.as_deref(),
                    input.lease_id.as_deref(),
                    &path_string,
                    &branch,
                ),
                repo_root: repo_root.clone(),
                repository_id: input.repository_id.clone(),
                tenant_context: tenant.clone(),
                path: path_string.clone(),
                branch: branch.clone(),
                base: base.clone(),
                managed,
                task_id: input.task_id,
                owner_run_id: input.owner_run_id,
                lease_id: input.lease_id,
                cleanup_branch,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
    }
    Ok(Json(json!({
        "ok": ok,
        "worktree_id": worktree_id,
        "repository_id": input.repository_id,
        "repo_root": expose_host_paths.then_some(repo_root),
        "path": expose_host_paths.then_some(path_string),
        "branch": branch,
        "base": base,
        "managed": managed,
        "cleanup_branch": cleanup_branch,
        "lease_client_id": lease.as_ref().map(|row| row.client_id.clone()),
        "lease_client_type": lease.as_ref().map(|row| row.client_type.clone()),
        "reused": false,
        "stderr": expose_host_paths.then(|| output.stderr.clone())
    })))
}

pub(in crate::http) async fn list_worktrees(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Query(query): Query<WorktreeListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let (resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        query.repository_id.as_deref(),
        query.repo_root.as_deref(),
    )
    .await?;
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeList,
        resource,
        json!({
            "repository_candidate": repo_candidate,
            "managed_only": true,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate).await?;
    let records = state
        .managed_worktrees
        .read()
        .await
        .values()
        .filter(|record| {
            record.managed
                && record.repo_root == repo_root
                && record.repository_id == query.repository_id
                && record.tenant_context.org_id == tenant.org_id
                && record.tenant_context.workspace_id == tenant.workspace_id
                && record.tenant_context.deployment_id == tenant.deployment_id
                && record.tenant_context.actor_id == tenant.actor_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let expose_host_paths = verified.is_none();
    let mut worktrees = Vec::with_capacity(records.len());
    for record in records {
        grant
            .revalidate(&state, &effect)
            .map_err(host_authorization_status)?;
        let registered = worktree_is_registered(&repo_root, &record.path).await?;
        worktrees.push(json!({
            "worktree_id": record.key,
            "repository_id": record.repository_id,
            "path": expose_host_paths.then_some(record.path),
            "repo_root": expose_host_paths.then_some(record.repo_root),
            "branch": record.branch,
            "base": record.base,
            "managed": true,
            "task_id": record.task_id,
            "owner_run_id": record.owner_run_id,
            "lease_id": record.lease_id,
            "cleanup_branch": record.cleanup_branch,
            "registered": registered,
        }));
    }
    Ok(Json(json!(worktrees)))
}

pub(in crate::http) async fn delete_worktree(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    if verified.is_some()
        && input
            .worktree_id
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (repository_resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        input.repository_id.as_deref(),
        input.repo_root.as_deref(),
    )
    .await?;
    let record = state
        .managed_worktrees
        .read()
        .await
        .values()
        .find(|record| {
            input
                .worktree_id
                .as_deref()
                .is_some_and(|worktree_id| record.key == worktree_id)
                || (verified.is_none()
                    && input
                        .path
                        .as_deref()
                        .is_some_and(|path| record.path == path))
        })
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    if !record.managed
        || record.repo_root != repo_candidate
        || record.repository_id != input.repository_id
        || record.tenant_context.org_id != tenant.org_id
        || record.tenant_context.workspace_id != tenant.workspace_id
        || record.tenant_context.deployment_id != tenant.deployment_id
        || record.tenant_context.actor_id != tenant.actor_id
    {
        return Err(StatusCode::NOT_FOUND);
    }
    validate_worktree_mutation_authority(&state, Some(&record), input.lease_id.as_deref()).await?;
    let resource = CanonicalHostResource::new(
        "managed_worktree",
        record.key.clone(),
        record.tenant_context.clone(),
    );
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeDelete,
        resource,
        json!({
            "repository_id": repository_resource.id,
            "repo_root": record.repo_root,
            "path": record.path,
            "branch": record.branch,
            "lease_id": &input.lease_id,
            "cleanup_branch": record.cleanup_branch,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate).await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    crate::runtime::worktrees::validate_managed_worktree_path(
        &repo_root,
        StdPath::new(&record.path),
        false,
    )
    .map_err(|_| StatusCode::FORBIDDEN)?;
    let dirty = crate::runtime::worktrees::run_managed_git(
        &record.path,
        &["status", "--porcelain", "--untracked-files=all"],
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !dirty.success || !dirty.stdout.trim().is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    crate::runtime::worktrees::validate_managed_worktree_path(
        &repo_root,
        StdPath::new(&record.path),
        false,
    )
    .map_err(|_| StatusCode::FORBIDDEN)?;
    let output = crate::runtime::worktrees::run_managed_git(
        &repo_root,
        &["worktree", "remove", "--", &record.path],
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut branch_deleted = false;
    if output.success && record.cleanup_branch {
        grant
            .revalidate(&state, &effect)
            .map_err(host_authorization_status)?;
        let branch_out = crate::runtime::worktrees::run_managed_git(
            &repo_root,
            &["branch", "-D", "--", &record.branch],
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        branch_deleted = branch_out.success;
    }
    if output.success {
        state.managed_worktrees.write().await.remove(&record.key);
    }
    let expose_host_paths = verified.is_none();
    Ok(Json(json!({
        "ok": output.success,
        "worktree_id": record.key,
        "repository_id": record.repository_id,
        "repo_root": expose_host_paths.then_some(repo_root),
        "path": expose_host_paths.then_some(record.path),
        "branch": record.branch,
        "cleanup_branch": record.cleanup_branch,
        "branch_deleted": branch_deleted,
        "stderr": expose_host_paths.then(|| output.stderr.clone())
    })))
}

pub(in crate::http) async fn reset_worktree(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    if verified.is_some()
        && input
            .worktree_id
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (repository_resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        input.repository_id.as_deref(),
        input.repo_root.as_deref(),
    )
    .await?;
    let record = state
        .managed_worktrees
        .read()
        .await
        .values()
        .find(|record| {
            input
                .worktree_id
                .as_deref()
                .is_some_and(|worktree_id| record.key == worktree_id)
                || (verified.is_none()
                    && input
                        .path
                        .as_deref()
                        .is_some_and(|path| record.path == path))
        })
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    if !record.managed
        || record.repo_root != repo_candidate
        || record.repository_id != input.repository_id
        || record.tenant_context.org_id != tenant.org_id
        || record.tenant_context.workspace_id != tenant.workspace_id
        || record.tenant_context.deployment_id != tenant.deployment_id
        || record.tenant_context.actor_id != tenant.actor_id
    {
        return Err(StatusCode::NOT_FOUND);
    }
    validate_worktree_mutation_authority(&state, Some(&record), input.lease_id.as_deref()).await?;
    let target = input.base.clone().unwrap_or_else(|| "HEAD".to_string());
    if target.trim_start().starts_with('-') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let backup_ref = format!("refs/tandem/backups/{}", Uuid::new_v4());
    let resource = CanonicalHostResource::new(
        "managed_worktree",
        record.key.clone(),
        record.tenant_context.clone(),
    );
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeReset,
        resource,
        json!({
            "repository_id": repository_resource.id,
            "repo_root": record.repo_root,
            "path": record.path,
            "target": target,
            "backup_ref": backup_ref,
            "lease_id": &input.lease_id,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate).await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    if !worktree_is_registered(&repo_root, &record.path).await? {
        return Err(StatusCode::NOT_FOUND);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    crate::runtime::worktrees::validate_managed_worktree_path(
        &repo_root,
        StdPath::new(&record.path),
        false,
    )
    .map_err(|_| StatusCode::FORBIDDEN)?;
    let dirty = crate::runtime::worktrees::run_managed_git(
        &record.path,
        &["status", "--porcelain", "--untracked-files=all"],
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !dirty.success || !dirty.stdout.trim().is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let backup = crate::runtime::worktrees::run_managed_git(
        &record.path,
        &["update-ref", &backup_ref, "HEAD"],
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !backup.success {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    crate::runtime::worktrees::validate_managed_worktree_path(
        &repo_root,
        StdPath::new(&record.path),
        false,
    )
    .map_err(|_| StatusCode::FORBIDDEN)?;
    let final_dirty = crate::runtime::worktrees::run_managed_git(
        &record.path,
        &["status", "--porcelain", "--untracked-files=all"],
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !final_dirty.success || !final_dirty.stdout.trim().is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let output =
        crate::runtime::worktrees::run_managed_git(&record.path, &["reset", "--keep", &target])
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let expose_host_paths = verified.is_none();
    Ok(Json(json!({
        "ok": output.success,
        "worktree_id": record.key,
        "repository_id": record.repository_id,
        "repo_root": expose_host_paths.then_some(repo_root),
        "path": expose_host_paths.then_some(record.path),
        "target": target,
        "backup_ref": backup_ref,
        "stderr": expose_host_paths.then(|| output.stderr.clone())
    })))
}

#[derive(Debug, Clone)]
struct RegisteredWorktreeEntry {
    path: String,
    branch: Option<String>,
}

async fn parse_registered_worktree_entries(
    repo_root: &str,
) -> Result<Vec<RegisteredWorktreeEntry>, StatusCode> {
    let output =
        crate::runtime::worktrees::run_managed_git(repo_root, &["worktree", "list", "--porcelain"])
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !output.success {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;
    for line in output.stdout.lines() {
        if line.is_empty() {
            if let Some(path) = current_path.take() {
                entries.push(RegisteredWorktreeEntry {
                    path,
                    branch: current_branch.take(),
                });
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.trim().to_string());
            continue;
        }
        if let Some(branch) = line.strip_prefix("branch ") {
            current_branch = branch
                .trim()
                .strip_prefix("refs/heads/")
                .map(ToString::to_string)
                .or_else(|| Some(branch.trim().to_string()));
        }
    }
    if let Some(path) = current_path.take() {
        entries.push(RegisteredWorktreeEntry {
            path,
            branch: current_branch.take(),
        });
    }
    Ok(entries)
}

pub(in crate::http) async fn cleanup_worktrees(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    payload: Option<Json<WorktreeCleanupInput>>,
) -> Result<Json<Value>, StatusCode> {
    let input = payload
        .map(|Json(value)| value)
        .unwrap_or_else(WorktreeCleanupInput::default);
    let dry_run = input.dry_run.unwrap_or(false);
    let remove_orphan_dirs = input.remove_orphan_dirs.unwrap_or(true);
    let (resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        input.repository_id.as_deref(),
        input.repo_root.as_deref(),
    )
    .await?;
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeCleanup,
        resource,
        json!({
            "repository_candidate": repo_candidate,
            "dry_run": dry_run,
            "remove_orphan_dirs": remove_orphan_dirs,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate).await?;
    let managed_root = crate::runtime::worktrees::managed_worktree_root(&repo_root);
    let managed_root_string = managed_root.to_string_lossy().to_string();
    let now = crate::now_ms();
    let active_lease_ids = state
        .engine_leases
        .read()
        .await
        .iter()
        .filter(|(_, lease)| lease.tenant_context == tenant && !lease.is_expired(now))
        .map(|(lease_id, _)| lease_id.clone())
        .collect::<std::collections::HashSet<_>>();
    let records = state
        .managed_worktrees
        .read()
        .await
        .values()
        .filter(|row| {
            row.repo_root == repo_root
                && row.repository_id == input.repository_id
                && row.tenant_context == tenant
        })
        .cloned()
        .collect::<Vec<_>>();
    let owned_by_path = records
        .iter()
        .map(|row| (row.path.clone(), row.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let stale_owned_paths = records
        .iter()
        .filter(|row| {
            row.lease_id
                .as_ref()
                .is_some_and(|lease_id| !active_lease_ids.contains(lease_id))
        })
        .map(|row| row.path.clone())
        .collect::<std::collections::HashSet<_>>();
    let active_paths = records
        .iter()
        .filter(|row| !stale_owned_paths.contains(&row.path))
        .map(|row| row.path.clone())
        .collect::<std::collections::HashSet<_>>();
    let tracked_paths = records
        .iter()
        .map(|row| row.path.clone())
        .collect::<Vec<_>>();
    let git_managed_worktrees = parse_registered_worktree_entries(&repo_root)
        .await?
        .into_iter()
        .filter(|entry| StdPath::new(&entry.path).starts_with(&managed_root))
        .collect::<Vec<_>>();

    let mut stale = Vec::new();
    let mut active = Vec::new();
    let mut unknown = Vec::new();
    for entry in &git_managed_worktrees {
        if active_paths.contains(&entry.path) {
            active.push(entry.path.clone());
        } else if stale_owned_paths.contains(&entry.path) {
            stale.push(entry.clone());
        } else {
            // No durable ownership record means no cleanup authority. This is
            // intentionally fail-closed after restart and for shared checkouts.
            unknown.push(entry.clone());
        }
    }

    let mut cleaned = Vec::new();
    let mut failures = Vec::new();
    if !dry_run {
        for entry in &stale {
            let Some(record) = owned_by_path.get(&entry.path) else {
                failures.push(json!({
                    "path": entry.path,
                    "code": "WORKTREE_OWNERSHIP_UNKNOWN",
                }));
                continue;
            };
            grant
                .revalidate(&state, &effect)
                .map_err(host_authorization_status)?;
            if crate::runtime::worktrees::validate_managed_worktree_path(
                &repo_root,
                StdPath::new(&entry.path),
                false,
            )
            .is_err()
            {
                failures.push(json!({
                    "path": entry.path,
                    "branch": entry.branch,
                    "code": "WORKTREE_PATH_CONTAINMENT_FAILED",
                }));
                continue;
            }
            let remove_output = crate::runtime::worktrees::run_managed_git(
                &repo_root,
                &["worktree", "remove", "--", &entry.path],
            )
            .await;
            match remove_output {
                Ok(result) if result.success => {
                    state
                        .managed_worktrees
                        .write()
                        .await
                        .retain(|_, row| row.key != record.key || row.tenant_context != tenant);
                    let mut branch_deleted = None;
                    let mut branch_delete_error = None;
                    if record.cleanup_branch {
                        if entry.branch.as_deref() != Some(record.branch.as_str()) {
                            branch_deleted = Some(false);
                            branch_delete_error = Some(
                                "registered branch does not match the owned cleanup record"
                                    .to_string(),
                            );
                        } else {
                            grant
                                .revalidate(&state, &effect)
                                .map_err(host_authorization_status)?;
                            match crate::runtime::worktrees::run_managed_git(
                                &repo_root,
                                &["branch", "-D", "--", &record.branch],
                            )
                            .await
                            {
                                Ok(branch_output) if branch_output.success => {
                                    branch_deleted = Some(true);
                                }
                                Ok(branch_output) => {
                                    branch_deleted = Some(false);
                                    branch_delete_error = Some(branch_output.stderr.clone());
                                }
                                Err(err) => {
                                    branch_deleted = Some(false);
                                    branch_delete_error = Some(err.to_string());
                                }
                            }
                        }
                    }
                    cleaned.push(json!({
                        "worktree_id": record.key,
                        "path": entry.path,
                        "branch": record.branch,
                        "branch_deleted": branch_deleted,
                        "branch_delete_error": branch_delete_error,
                        "via": "git_worktree_remove",
                    }));
                }
                Ok(result) => {
                    failures.push(json!({
                        "path": entry.path,
                        "branch": entry.branch,
                        "code": "WORKTREE_REMOVE_FAILED",
                        "stderr": result.stderr.clone(),
                    }));
                }
                Err(err) => {
                    failures.push(json!({
                        "path": entry.path,
                        "branch": entry.branch,
                        "code": "WORKTREE_REMOVE_FAILED",
                        "error": err.to_string(),
                    }));
                }
            }
        }
    }

    let mut orphan_dirs = Vec::new();
    if managed_root.exists() {
        let entries =
            std::fs::read_dir(&managed_root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let registered_paths = if dry_run {
            git_managed_worktrees
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<std::collections::HashSet<_>>()
        } else {
            parse_registered_worktree_entries(&repo_root)
                .await?
                .into_iter()
                .map(|entry| entry.path)
                .filter(|path| StdPath::new(path).starts_with(&managed_root))
                .collect::<std::collections::HashSet<_>>()
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let path_string = path.to_string_lossy().to_string();
            if registered_paths.contains(&path_string) {
                continue;
            }
            if !stale_owned_paths.contains(&path_string) {
                // Directories without an exact caller-owned stale record are
                // unknown and must survive cleanup, including after restart.
                continue;
            }
            orphan_dirs.push(path_string);
        }
    }

    let mut orphan_removed = Vec::new();
    if !dry_run && remove_orphan_dirs {
        for path in &orphan_dirs {
            grant
                .revalidate(&state, &effect)
                .map_err(host_authorization_status)?;
            if crate::runtime::worktrees::validate_managed_worktree_path(
                &repo_root,
                StdPath::new(path),
                false,
            )
            .is_err()
            {
                failures.push(json!({
                    "path": path,
                    "code": "WORKTREE_PATH_CONTAINMENT_FAILED",
                }));
                continue;
            }
            match crate::runtime::worktrees::remove_managed_worktree_dir(
                &repo_root,
                StdPath::new(path),
                verified.is_none(),
            ) {
                Ok(_) => {
                    if let Some(record) = owned_by_path.get(path) {
                        state
                            .managed_worktrees
                            .write()
                            .await
                            .retain(|_, row| row.key != record.key || row.tenant_context != tenant);
                    }
                    orphan_removed.push(json!({
                        "path": path,
                        "via": "descriptor_relative_remove_dir_all",
                    }));
                }
                Err(err) => {
                    failures.push(json!({
                        "path": path,
                        "code": "WORKTREE_ORPHAN_DIR_REMOVE_FAILED",
                        "error": err.to_string(),
                    }));
                }
            }
        }
    }

    let expose_host_details = verified.is_none();
    Ok(Json(json!({
        "ok": failures.is_empty(),
        "dry_run": dry_run,
        "repo_root": expose_host_details.then_some(repo_root),
        "managed_root": expose_host_details.then_some(managed_root_string),
        "tracked_path_count": tracked_paths.len(),
        "active_path_count": active.len(),
        "stale_path_count": stale.len(),
        "unknown_registered_path_count": unknown.len(),
        "cleaned_worktree_count": cleaned.len(),
        "orphan_dir_count": orphan_dirs.len(),
        "orphan_dir_removed_count": orphan_removed.len(),
        "failure_count": failures.len(),
        "tracked_paths": expose_host_details.then_some(tracked_paths),
        "active_paths": expose_host_details.then_some(active),
        "stale_paths": expose_host_details.then(|| stale.iter().map(|entry| json!({
            "path": entry.path,
            "branch": entry.branch,
        })).collect::<Vec<_>>()),
        "unknown_registered_paths": expose_host_details.then(|| unknown.iter().map(|entry| json!({
            "path": entry.path,
            "branch": entry.branch,
        })).collect::<Vec<_>>()),
        "cleaned_worktrees": expose_host_details.then_some(cleaned),
        "orphan_dirs": expose_host_details.then_some(orphan_dirs),
        "orphan_dirs_removed": expose_host_details.then_some(orphan_removed),
        "failures": expose_host_details.then_some(failures),
    })))
}

async fn resolve_worktree_resource_candidate(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    repository_id: Option<&str>,
    repo_root: Option<&str>,
) -> Result<(CanonicalHostResource, String), StatusCode> {
    if let Some(repository_id) = repository_id
        .map(str::trim)
        .filter(|repository_id| !repository_id.is_empty())
    {
        let session = state
            .storage
            .get_session(repository_id)
            .await
            .ok_or(StatusCode::NOT_FOUND)?;
        crate::http::sessions_actor_scope::ensure_same_session_actor(
            tenant,
            &session.tenant_context,
        )?;
        let workspace = session
            .workspace_root
            .as_deref()
            .unwrap_or(session.directory.as_str());
        let canonical = tokio::fs::canonicalize(workspace)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        let candidate = crate::normalize_absolute_workspace_root(&canonical.to_string_lossy())
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        return Ok((
            CanonicalHostResource::new("repository", repository_id, session.tenant_context),
            candidate,
        ));
    }
    if verified.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let requested = if let Some(repo_root) = repo_root {
        PathBuf::from(repo_root)
    } else {
        let root = state.workspace_index.snapshot().await.root;
        let root = PathBuf::from(root);
        if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .join(root)
        }
    };
    let canonical = tokio::fs::canonicalize(requested)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let candidate = crate::normalize_absolute_workspace_root(&canonical.to_string_lossy())
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok((
        CanonicalHostResource::new("local_repository", "local-repository", tenant.clone()),
        candidate,
    ))
}

async fn verify_git_repo_root(candidate: &str) -> Result<String, StatusCode> {
    let output =
        crate::runtime::worktrees::run_managed_git(candidate, &["rev-parse", "--show-toplevel"])
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !output.success {
        return Err(StatusCode::CONFLICT);
    }
    let resolved = crate::normalize_absolute_workspace_root(output.stdout.trim())
        .map_err(|_| StatusCode::CONFLICT)?;
    if resolved != candidate {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(resolved)
}

async fn validate_managed_worktree_lease(
    state: &AppState,
    managed: bool,
    lease_id: Option<&str>,
    tenant: &TenantContext,
) -> Result<Option<crate::EngineLease>, StatusCode> {
    if !managed {
        return Ok(None);
    }
    let Some(lease_id) = lease_id.filter(|value| !value.trim().is_empty()) else {
        return Err(StatusCode::CONFLICT);
    };
    let now = crate::now_ms();
    let mut leases = state.engine_leases.write().await;
    leases.retain(|_, lease| !lease.is_expired(now));
    let lease = leases.get(lease_id).cloned().ok_or(StatusCode::CONFLICT)?;
    if lease.tenant_context != *tenant {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Some(lease))
}

pub(in crate::http) async fn prune_expired_leases(state: &AppState) -> usize {
    let now = crate::now_ms();
    let expired = {
        let mut leases = state.engine_leases.write().await;
        let expired = leases
            .iter()
            .filter(|(_, lease)| lease.is_expired(now))
            .map(|(lease_id, _)| lease_id.clone())
            .collect::<Vec<_>>();
        leases.retain(|_, lease| !lease.is_expired(now));
        expired
    };
    for lease_id in expired {
        cleanup_managed_worktrees_for_lease(state, &lease_id, None).await;
    }
    state.engine_leases.read().await.len()
}

async fn validate_worktree_mutation_authority(
    state: &AppState,
    record: Option<&crate::ManagedWorktreeRecord>,
    lease_id: Option<&str>,
) -> Result<(), StatusCode> {
    let record = record.ok_or(StatusCode::NOT_FOUND)?;
    if !record.managed {
        return Err(StatusCode::FORBIDDEN);
    }
    let record_lease_id = record
        .lease_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::CONFLICT)?;
    let request_lease_id = lease_id
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::CONFLICT)?;
    if request_lease_id != record_lease_id {
        return Err(StatusCode::CONFLICT);
    }
    validate_managed_worktree_lease(state, true, Some(request_lease_id), &record.tenant_context)
        .await
        .map(|_| ())
}

#[derive(Default)]
pub(in crate::http) struct LeaseWorktreeCleanupResult {
    pub(super) cleaned_paths: Vec<String>,
    pub(super) failures: Vec<Value>,
}

pub(in crate::http) async fn cleanup_managed_worktrees_for_lease(
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
                    result.failures.push(json!({
                        "path": record.path,
                        "branch": record.branch,
                        "repo_root": record.repo_root,
                        "code": "WORKTREE_BRANCH_DELETE_FAILED",
                        "stderr": branch_output.stderr.clone(),
                    }));
                }
                Err(_) => {
                    result.failures.push(json!({
                        "path": record.path,
                        "branch": record.branch,
                        "repo_root": record.repo_root,
                        "code": "WORKTREE_BRANCH_DELETE_FAILED",
                    }));
                }
            }
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

fn resolve_worktree_path(
    repo_root: &str,
    raw: Option<&str>,
    default_path: &StdPath,
) -> Result<PathBuf, StatusCode> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(default_path.to_path_buf());
    };
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = PathBuf::from(repo_root).join(candidate);
    if !is_within_managed_worktree_root(repo_root, &path) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(path)
}

fn is_within_managed_worktree_root(repo_root: &str, path: &StdPath) -> bool {
    let managed_root = PathBuf::from(repo_root).join(".tandem").join("worktrees");
    path.starts_with(managed_root)
}

async fn worktree_is_registered(repo_root: &str, path: &str) -> Result<bool, StatusCode> {
    Ok(parse_registered_worktree_entries(repo_root)
        .await?
        .into_iter()
        .any(|entry| entry.path == path))
}

fn annotate_managed_worktree(
    record: &mut serde_json::Map<String, Value>,
    repo_root: &str,
    managed_records: &std::collections::HashMap<String, crate::ManagedWorktreeRecord>,
) {
    let path = record
        .get("worktree")
        .and_then(Value::as_str)
        .or_else(|| record.get("path").and_then(Value::as_str));
    let Some(path) = path else {
        return;
    };
    if let Some(managed) = managed_records
        .values()
        .find(|row| row.repo_root == repo_root && row.path == path)
    {
        record.insert("path".to_string(), Value::String(managed.path.clone()));
        record.insert("branch".to_string(), Value::String(managed.branch.clone()));
        record.insert("base".to_string(), Value::String(managed.base.clone()));
        record.insert("managed".to_string(), Value::Bool(managed.managed));
        record.insert(
            "repo_root".to_string(),
            Value::String(managed.repo_root.clone()),
        );
        record.insert(
            "cleanup_branch".to_string(),
            Value::Bool(managed.cleanup_branch),
        );
        record.insert(
            "task_id".to_string(),
            managed
                .task_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        record.insert(
            "owner_run_id".to_string(),
            managed
                .owner_run_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        record.insert(
            "lease_id".to_string(),
            managed
                .lease_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        record.insert("registered".to_string(), Value::Bool(true));
    }
}

async fn find_managed_worktree_by_path(
    state: &AppState,
    repo_root: &str,
    path: &str,
) -> Option<crate::ManagedWorktreeRecord> {
    state
        .managed_worktrees
        .read()
        .await
        .values()
        .find(|row| row.repo_root == repo_root && row.path == path)
        .cloned()
}
