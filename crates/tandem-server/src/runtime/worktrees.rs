use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedWorktreeRecord {
    pub key: String,
    pub repo_root: String,
    pub path: String,
    pub branch: String,
    pub base: String,
    pub managed: bool,
    pub task_id: Option<String>,
    pub owner_run_id: Option<String>,
    pub lease_id: Option<String>,
    pub cleanup_branch: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ManagedWorktreeEnsureInput {
    pub repo_root: String,
    pub task_id: Option<String>,
    pub owner_run_id: Option<String>,
    pub lease_id: Option<String>,
    pub branch_hint: Option<String>,
    pub base: String,
    pub cleanup_branch: bool,
}

#[derive(Debug, Clone)]
pub struct ManagedWorktreeEnsureResult {
    pub record: ManagedWorktreeRecord,
    pub reused: bool,
}

fn slug_part(raw: Option<&str>) -> Option<String> {
    let cleaned = raw
        .unwrap_or_default()
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let collapsed = cleaned
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

pub fn managed_worktree_slug(
    task_id: Option<&str>,
    owner_run_id: Option<&str>,
    lease_id: Option<&str>,
    branch_hint: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(task_id) = slug_part(task_id) {
        parts.push(task_id);
    }
    if let Some(owner_run_id) = slug_part(owner_run_id) {
        parts.push(owner_run_id);
    }
    if let Some(lease_id) = slug_part(lease_id) {
        parts.push(lease_id);
    }
    if parts.is_empty() {
        parts.push(
            slug_part(branch_hint)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "worktree".to_string()),
        );
    }
    parts.join("-")
}

pub fn managed_worktree_key(
    repo_root: &str,
    task_id: Option<&str>,
    owner_run_id: Option<&str>,
    lease_id: Option<&str>,
    path: &str,
    branch: &str,
) -> String {
    let task_id = task_id.unwrap_or("");
    let owner_run_id = owner_run_id.unwrap_or("");
    let lease_id = lease_id.unwrap_or("");
    format!("{repo_root}::{task_id}::{owner_run_id}::{lease_id}::{path}::{branch}")
}

pub fn managed_worktree_root(repo_root: &str) -> PathBuf {
    PathBuf::from(repo_root).join(".tandem").join("worktrees")
}

pub fn managed_worktree_path(repo_root: &str, slug: &str) -> PathBuf {
    managed_worktree_root(repo_root).join(slug)
}

pub fn is_within_managed_worktree_root(repo_root: &str, path: &Path) -> bool {
    path.starts_with(managed_worktree_root(repo_root))
}

pub fn resolve_git_repo_root(candidate: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-C", candidate, "rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
    crate::normalize_absolute_workspace_root(&resolved).ok()
}

pub async fn ensure_managed_worktree(
    state: &crate::AppState,
    input: ManagedWorktreeEnsureInput,
) -> anyhow::Result<ManagedWorktreeEnsureResult> {
    let slug = managed_worktree_slug(
        input.task_id.as_deref(),
        input.owner_run_id.as_deref(),
        input.lease_id.as_deref(),
        input.branch_hint.as_deref(),
    );
    let path = managed_worktree_path(&input.repo_root, &slug);
    let branch = format!("tandem/{slug}");
    let path_string = path.to_string_lossy().to_string();
    let key = managed_worktree_key(
        &input.repo_root,
        input.task_id.as_deref(),
        input.owner_run_id.as_deref(),
        input.lease_id.as_deref(),
        &path_string,
        &branch,
    );
    if let Some(existing) = state.managed_worktrees.read().await.get(&key).cloned() {
        if worktree_is_registered(&input.repo_root, &existing.path)? {
            return Ok(ManagedWorktreeEnsureResult {
                record: existing,
                reused: true,
            });
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() && !worktree_is_registered(&input.repo_root, &path_string)? {
        anyhow::bail!("managed worktree path conflict: {path_string}");
    }
    let now = crate::now_ms();
    if worktree_is_registered(&input.repo_root, &path_string)? {
        let record = ManagedWorktreeRecord {
            key: key.clone(),
            repo_root: input.repo_root.clone(),
            path: path_string,
            branch,
            base: input.base,
            managed: true,
            task_id: input.task_id,
            owner_run_id: input.owner_run_id,
            lease_id: input.lease_id,
            cleanup_branch: input.cleanup_branch,
            created_at_ms: now,
            updated_at_ms: now,
        };
        state
            .managed_worktrees
            .write()
            .await
            .insert(key, record.clone());
        return Ok(ManagedWorktreeEnsureResult {
            record,
            reused: true,
        });
    }
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &input.repo_root,
            "worktree",
            "add",
            "-b",
            &branch,
            &path.to_string_lossy(),
            &input.base,
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let record = ManagedWorktreeRecord {
        key: key.clone(),
        repo_root: input.repo_root,
        path: path.to_string_lossy().to_string(),
        branch,
        base: input.base,
        managed: true,
        task_id: input.task_id,
        owner_run_id: input.owner_run_id,
        lease_id: input.lease_id,
        cleanup_branch: input.cleanup_branch,
        created_at_ms: now,
        updated_at_ms: now,
    };
    state
        .managed_worktrees
        .write()
        .await
        .insert(key, record.clone());
    Ok(ManagedWorktreeEnsureResult {
        record,
        reused: false,
    })
}

pub async fn delete_managed_worktree(
    state: &crate::AppState,
    record: &ManagedWorktreeRecord,
) -> anyhow::Result<()> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &record.repo_root,
            "worktree",
            "remove",
            "--force",
            &record.path,
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git worktree remove failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if record.cleanup_branch {
        let _ = std::process::Command::new("git")
            .args(["-C", &record.repo_root, "branch", "-D", &record.branch])
            .output();
    }
    state
        .managed_worktrees
        .write()
        .await
        .retain(|_, row| !(row.repo_root == record.repo_root && row.path == record.path));
    Ok(())
}

fn worktree_is_registered(repo_root: &str, path: &str) -> anyhow::Result<bool> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_root, "worktree", "list", "--porcelain"])
        .output()?;
    if !output.status.success() {
        return Ok(false);
    }
    let needle = PathBuf::from(path);
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(value) = line.strip_prefix("worktree ") {
            if PathBuf::from(value) == needle {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
