// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use tandem_types::TenantContext;

const MANAGED_GIT_DEADLINE: Duration = Duration::from_secs(15);
const MANAGED_GIT_OUTPUT_LIMIT: u64 = 256 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedWorktreeRecord {
    pub key: String,
    pub repo_root: String,
    #[serde(default)]
    pub repository_id: Option<String>,
    #[serde(default = "TenantContext::local_implicit")]
    pub tenant_context: TenantContext,
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
    pub repository_id: Option<String>,
    pub tenant_context: TenantContext,
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
    let identity = format!(
        "{repo_root}::{}::{}::{}::{path}::{branch}",
        task_id.unwrap_or(""),
        owner_run_id.unwrap_or(""),
        lease_id.unwrap_or("")
    );
    format!("wt_{:x}", Sha256::digest(identity.as_bytes()))
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

pub(crate) fn validate_managed_worktree_path(
    repo_root: &str,
    path: &Path,
    create_parents: bool,
) -> anyhow::Result<()> {
    let canonical_repo = std::fs::canonicalize(repo_root)?;
    let managed_root = canonical_repo.join(".tandem").join("worktrees");
    let requested_parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("managed worktree path has no parent"))?;
    let relative_parent = requested_parent
        .strip_prefix(&managed_root)
        .map_err(|_| anyhow::anyhow!("managed worktree path escapes managed root"))?;
    let mut current = canonical_repo;
    let root_components = [
        std::ffi::OsString::from(".tandem"),
        std::ffi::OsString::from("worktrees"),
    ];
    for component in root_components.into_iter().chain(
        relative_parent
            .components()
            .map(|component| component.as_os_str().to_os_string()),
    ) {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    anyhow::bail!("managed worktree parent is not a real directory");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create_parents => {
                match std::fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                let metadata = std::fs::symlink_metadata(&current)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    anyhow::bail!("managed worktree parent changed during creation");
                }
            }
            Err(error) => return Err(error.into()),
        }
        if std::fs::canonicalize(&current)? != current {
            anyhow::bail!("managed worktree parent resolves through a symlink");
        }
    }
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            anyhow::bail!("managed worktree target is a symlink");
        }
        let canonical_target = std::fs::canonicalize(path)?;
        if !canonical_target.starts_with(&managed_root) {
            anyhow::bail!("managed worktree target escapes managed root");
        }
    }
    Ok(())
}

#[cfg(unix)]
fn open_directory_no_symlinks(path: &Path) -> anyhow::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{open, openat, Mode, OFlags};

    if !path.is_absolute() {
        anyhow::bail!("directory path must be absolute");
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut current = open("/", flags, Mode::empty())?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                current = openat(&current, name, flags, Mode::empty())?;
            }
            _ => anyhow::bail!("directory path contains a non-normal component"),
        }
    }
    Ok(current)
}

#[cfg(unix)]
fn remove_directory_contents_at(directory: &rustix::fd::OwnedFd) -> anyhow::Result<()> {
    use rustix::fs::{openat, unlinkat, AtFlags, Dir, Mode, OFlags};

    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut entries = Dir::read_from(directory)?;
    for entry in &mut entries {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        match openat(directory, name, flags, Mode::empty()) {
            Ok(child) => {
                remove_directory_contents_at(&child)?;
                unlinkat(directory, name, AtFlags::REMOVEDIR)?;
            }
            Err(rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
                unlinkat(directory, name, AtFlags::empty())?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn remove_managed_worktree_dir(
    repo_root: &str,
    path: &Path,
    _allow_path_fallback: bool,
) -> anyhow::Result<()> {
    use rustix::fs::{openat, unlinkat, AtFlags, Mode, OFlags};

    let canonical_repo = std::fs::canonicalize(repo_root)?;
    let managed_root = canonical_repo.join(".tandem").join("worktrees");
    let relative = path
        .strip_prefix(&managed_root)
        .map_err(|_| anyhow::anyhow!("managed worktree path escapes managed root"))?;
    let mut components = relative.components();
    let Some(std::path::Component::Normal(target_name)) = components.next() else {
        anyhow::bail!("managed worktree target is invalid");
    };
    if components.next().is_some() {
        anyhow::bail!("orphan cleanup only accepts direct managed-root children");
    }

    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let repo = open_directory_no_symlinks(&canonical_repo)?;
    let tandem = openat(&repo, ".tandem", flags, Mode::empty())?;
    let worktrees = openat(&tandem, "worktrees", flags, Mode::empty())?;
    let target = openat(&worktrees, target_name, flags, Mode::empty())?;
    remove_directory_contents_at(&target)?;
    drop(target);
    unlinkat(&worktrees, target_name, AtFlags::REMOVEDIR)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn remove_managed_worktree_dir(
    repo_root: &str,
    path: &Path,
    allow_path_fallback: bool,
) -> anyhow::Result<()> {
    if !allow_path_fallback {
        anyhow::bail!("verified orphan cleanup requires descriptor-relative deletion");
    }
    validate_managed_worktree_path(repo_root, path, false)?;
    std::fs::remove_dir_all(path)?;
    Ok(())
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
    if input.base.trim_start().starts_with('-') {
        anyhow::bail!("git worktree base ref cannot start with '-'");
    }
    let effect = crate::action_authorization::HostEffectRequest::new(
        crate::action_authorization::HostAction::WorktreeCreate,
        crate::action_authorization::CanonicalHostResource::new(
            "repository",
            input
                .repository_id
                .clone()
                .unwrap_or_else(|| "local-repository".to_string()),
            input.tenant_context.clone(),
        ),
        json!({
            "repo_root": &input.repo_root,
            "path": &path_string,
            "branch": &branch,
            "base": &input.base,
            "lease_id": &input.lease_id,
            "cleanup_branch": input.cleanup_branch,
        }),
    );
    let grant = crate::action_authorization::authorize_internal_host_effect(
        state,
        "runtime.worktrees.ensure_managed_worktree",
        &effect,
    )
    .await
    .map_err(|error| anyhow::anyhow!("worktree authorization failed: {}", error.code()))?;
    if let Some(existing) = state.managed_worktrees.read().await.get(&key).cloned() {
        grant
            .revalidate(state, &effect)
            .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
        if worktree_registration_matches_async(
            input.repo_root.clone(),
            existing.path.clone(),
            existing.branch.clone(),
        )
        .await?
        {
            return Ok(ManagedWorktreeEnsureResult {
                record: existing,
                reused: true,
            });
        }
    }
    grant
        .revalidate(state, &effect)
        .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
    let repo_root_for_path = input.repo_root.clone();
    let path_for_validation = path.clone();
    tokio::task::spawn_blocking(move || {
        validate_managed_worktree_path(&repo_root_for_path, &path_for_validation, true)
    })
    .await
    .context("managed worktree path validation failed")??;
    grant
        .revalidate(state, &effect)
        .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
    if tokio::fs::try_exists(&path).await?
        && !worktree_registration_matches_async(
            input.repo_root.clone(),
            path_string.clone(),
            branch.clone(),
        )
        .await?
    {
        anyhow::bail!("managed worktree path conflict: {path_string}");
    }
    let now = crate::now_ms();
    grant
        .revalidate(state, &effect)
        .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
    if worktree_registration_matches_async(
        input.repo_root.clone(),
        path_string.clone(),
        branch.clone(),
    )
    .await?
    {
        let record = ManagedWorktreeRecord {
            key: key.clone(),
            repo_root: input.repo_root.clone(),
            repository_id: input.repository_id.clone(),
            tenant_context: input.tenant_context.clone(),
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
    grant
        .revalidate(state, &effect)
        .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
    add_git_worktree_async(
        input.repo_root.clone(),
        branch.clone(),
        path.clone(),
        input.base.clone(),
    )
    .await?;
    let record = ManagedWorktreeRecord {
        key: key.clone(),
        repo_root: input.repo_root,
        repository_id: input.repository_id,
        tenant_context: input.tenant_context,
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
    let effect = crate::action_authorization::HostEffectRequest::new(
        crate::action_authorization::HostAction::WorktreeDelete,
        crate::action_authorization::CanonicalHostResource::new(
            "managed_worktree",
            record.key.clone(),
            record.tenant_context.clone(),
        ),
        json!({
            "repository_id": &record.repository_id,
            "repo_root": &record.repo_root,
            "path": &record.path,
            "branch": &record.branch,
            "lease_id": &record.lease_id,
            "cleanup_branch": record.cleanup_branch,
        }),
    );
    let grant = crate::action_authorization::authorize_internal_host_effect(
        state,
        "runtime.worktrees.delete_managed_worktree",
        &effect,
    )
    .await
    .map_err(|error| anyhow::anyhow!("worktree authorization failed: {}", error.code()))?;
    grant
        .revalidate(state, &effect)
        .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
    let repo_root_for_validation = record.repo_root.clone();
    let path_for_validation = PathBuf::from(&record.path);
    tokio::task::spawn_blocking(move || {
        validate_managed_worktree_path(&repo_root_for_validation, &path_for_validation, false)
    })
    .await
    .context("managed worktree path validation failed")??;
    remove_git_worktree_async(
        state,
        &grant,
        &effect,
        record.repo_root.clone(),
        record.path.clone(),
        record.branch.clone(),
    )
    .await?;
    if record.cleanup_branch {
        grant
            .revalidate(state, &effect)
            .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
        delete_git_branch_async(record.repo_root.clone(), record.branch.clone()).await?;
    }
    grant
        .revalidate(state, &effect)
        .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
    state
        .managed_worktrees
        .write()
        .await
        .retain(|_, row| !(row.repo_root == record.repo_root && row.path == record.path));
    Ok(())
}

pub(crate) struct ManagedGitOutput {
    pub(crate) success: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
}

#[cfg(windows)]
fn null_device() -> &'static str {
    "NUL"
}

#[cfg(not(windows))]
fn null_device() -> &'static str {
    "/dev/null"
}

async fn read_managed_git_output<R>(reader: R) -> std::io::Result<(String, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut reader = reader;
    let mut bytes = Vec::new();
    (&mut reader)
        .take(MANAGED_GIT_OUTPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .await?;
    let truncated = bytes.len() as u64 > MANAGED_GIT_OUTPUT_LIMIT;
    bytes.truncate(MANAGED_GIT_OUTPUT_LIMIT as usize);
    tokio::io::copy(&mut reader, &mut tokio::io::sink()).await?;
    Ok((String::from_utf8_lossy(&bytes).to_string(), truncated))
}

async fn run_managed_git_with_filter_overrides(
    repo_root: &str,
    args: &[&str],
    filter_drivers: &[String],
) -> anyhow::Result<ManagedGitOutput> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .args(["-c", "core.fsmonitor=false"])
        .arg("-c")
        .arg(format!("core.hooksPath={}", null_device()))
        .args(["-c", "diff.external="])
        .args(["-c", "core.pager=cat"])
        .args(["-c", "credential.helper="])
        .args(["-c", "protocol.file.allow=never"])
        .args(["-c", "submodule.recurse=false"])
        .args(["-C", repo_root])
        .args(args)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_ATTR_GLOBAL", null_device())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut config_index = 0usize;
    for driver in filter_drivers {
        for (operation, value) in [
            ("clean", ""),
            ("smudge", ""),
            ("process", ""),
            ("required", "false"),
        ] {
            command
                .env(
                    format!("GIT_CONFIG_KEY_{config_index}"),
                    format!("filter.{driver}.{operation}"),
                )
                .env(format!("GIT_CONFIG_VALUE_{config_index}"), value);
            config_index += 1;
        }
    }
    command.env("GIT_CONFIG_COUNT", config_index.to_string());
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    let mut child = command.spawn().context("managed git spawn failed")?;
    let stdout = child.stdout.take().context("managed git stdout missing")?;
    let stderr = child.stderr.take().context("managed git stderr missing")?;
    let execution = tokio::time::timeout(MANAGED_GIT_DEADLINE, async {
        tokio::try_join!(
            read_managed_git_output(stdout),
            read_managed_git_output(stderr),
            child.wait(),
        )
    })
    .await;
    let ((stdout, stdout_truncated), (stderr, stderr_truncated), status) = match execution {
        Ok(result) => result.context("managed git execution failed")?,
        Err(_) => {
            let _ = child.kill().await;
            anyhow::bail!("managed git command timed out");
        }
    };
    Ok(ManagedGitOutput {
        success: status.success(),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

async fn managed_git_filter_drivers(repo_root: &str) -> anyhow::Result<Vec<String>> {
    let output = run_managed_git_with_filter_overrides(
        repo_root,
        &[
            "config",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\.",
        ],
        &[],
    )
    .await?;
    if !output.success && (!output.stdout.is_empty() || !output.stderr.is_empty()) {
        anyhow::bail!("managed Git filter discovery failed");
    }
    let mut drivers = std::collections::BTreeSet::new();
    for key in output.stdout.split("\0").filter(|key| !key.is_empty()) {
        let Some((driver, operation)) = key
            .strip_prefix("filter.")
            .and_then(|key| key.rsplit_once('.'))
        else {
            continue;
        };
        if !["clean", "smudge", "process", "required"].contains(&operation) {
            continue;
        }
        if driver.is_empty()
            || driver.len() > 128
            || !driver
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
        {
            anyhow::bail!("managed Git filter driver is invalid");
        }
        drivers.insert(driver.to_string());
        if drivers.len() > 64 {
            anyhow::bail!("managed Git filter driver limit exceeded");
        }
    }
    Ok(drivers.into_iter().collect())
}

pub(crate) async fn run_managed_git(
    repo_root: &str,
    args: &[&str],
) -> anyhow::Result<ManagedGitOutput> {
    let filter_drivers = managed_git_filter_drivers(repo_root).await?;
    run_managed_git_with_filter_overrides(repo_root, args, &filter_drivers).await
}

async fn worktree_registration_matches_async(
    repo_root: String,
    path: String,
    expected_branch: String,
) -> anyhow::Result<bool> {
    let output = run_managed_git(&repo_root, &["worktree", "list", "--porcelain"]).await?;
    if !output.success {
        return Ok(false);
    }
    let needle = PathBuf::from(path);
    for block in output.stdout.split("\n\n") {
        let registered_path = block
            .lines()
            .find_map(|line| line.strip_prefix("worktree ").map(PathBuf::from));
        if registered_path.as_ref() != Some(&needle) {
            continue;
        }
        let registered_branch = block.lines().find_map(|line| {
            line.strip_prefix("branch ")
                .and_then(|value| value.strip_prefix("refs/heads/"))
        });
        return Ok(registered_branch == Some(expected_branch.as_str()));
    }
    Ok(false)
}

async fn add_git_worktree_async(
    repo_root: String,
    branch: String,
    path: PathBuf,
    base: String,
) -> anyhow::Result<()> {
    let path = path.to_string_lossy().to_string();
    let output = run_managed_git(
        &repo_root,
        &["worktree", "add", "-b", &branch, &path, "--", &base],
    )
    .await?;
    if !output.success {
        anyhow::bail!("git worktree add failed: {}", output.stderr.trim());
    }
    Ok(())
}

async fn remove_git_worktree_async(
    state: &crate::AppState,
    grant: &crate::action_authorization::AuthorizedHostEffect,
    effect: &crate::action_authorization::HostEffectRequest,
    repo_root: String,
    path: String,
    expected_branch: String,
) -> anyhow::Result<()> {
    if !worktree_registration_matches_async(repo_root.clone(), path.clone(), expected_branch)
        .await?
    {
        anyhow::bail!("managed worktree registration does not match the owned branch");
    }
    grant
        .revalidate(state, effect)
        .map_err(|error| anyhow::anyhow!("worktree grant invalid: {}", error.code()))?;
    let output = run_managed_git(&repo_root, &["worktree", "remove", "--", &path]).await?;
    if !output.success {
        anyhow::bail!("git worktree remove failed: {}", output.stderr.trim());
    }
    Ok(())
}

async fn delete_git_branch_async(repo_root: String, branch: String) -> anyhow::Result<()> {
    let output = run_managed_git(&repo_root, &["branch", "-D", &branch]).await?;
    if !output.success {
        anyhow::bail!("git branch delete failed: {}", output.stderr.trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        managed_worktree_key, read_managed_git_output, remove_managed_worktree_dir,
        run_managed_git, validate_managed_worktree_path, worktree_registration_matches_async,
    };
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn managed_git_output_is_capped_while_the_pipe_is_fully_drained() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        let payload = vec![b'x'; super::MANAGED_GIT_OUTPUT_LIMIT as usize * 2];
        let write = tokio::spawn(async move {
            writer.write_all(&payload).await?;
            writer.shutdown().await
        });

        let (output, truncated) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_managed_git_output(reader),
        )
        .await
        .expect("bounded reader must not deadlock")
        .expect("read managed output");
        write.await.expect("join writer").expect("drain writer");
        assert!(truncated);
        assert_eq!(output.len(), super::MANAGED_GIT_OUTPUT_LIMIT as usize);
    }

    #[cfg(unix)]
    #[test]
    fn managed_worktree_path_rejects_symlinked_root() {
        let repo = tempfile::tempdir().expect("repo tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        std::fs::create_dir(repo.path().join(".tandem")).expect("create tandem directory");
        std::os::unix::fs::symlink(
            outside.path(),
            repo.path().join(".tandem").join("worktrees"),
        )
        .expect("create managed-root symlink");
        let target = repo.path().join(".tandem").join("worktrees").join("task-a");

        assert!(validate_managed_worktree_path(
            repo.path().to_str().expect("repo utf8"),
            &target,
            true,
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn descriptor_relative_orphan_removal_does_not_follow_symlinks() {
        let repo = tempfile::tempdir().expect("repo tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let managed_root = repo.path().join(".tandem").join("worktrees");
        let orphan = managed_root.join("orphan");
        std::fs::create_dir_all(orphan.join("nested")).expect("create orphan tree");
        std::fs::write(orphan.join("nested").join("file.txt"), b"orphan")
            .expect("write orphan file");
        let outside_sentinel = outside.path().join("keep.txt");
        std::fs::write(&outside_sentinel, b"keep").expect("write outside sentinel");
        std::os::unix::fs::symlink(outside.path(), orphan.join("outside-link"))
            .expect("create outside symlink");

        remove_managed_worktree_dir(repo.path().to_str().expect("repo utf8"), &orphan, false)
            .expect("remove orphan through retained handles");

        assert!(!orphan.exists());
        assert!(
            outside_sentinel.exists(),
            "outside symlink target must remain"
        );
    }

    #[cfg(unix)]
    #[test]
    fn descriptor_relative_orphan_removal_rejects_symlinked_parent() {
        let repo = tempfile::tempdir().expect("repo tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        std::fs::create_dir(repo.path().join(".tandem")).expect("create tandem directory");
        let outside_orphan = outside.path().join("orphan");
        std::fs::create_dir(&outside_orphan).expect("create outside orphan");
        let sentinel = outside_orphan.join("keep.txt");
        std::fs::write(&sentinel, b"keep").expect("write outside sentinel");
        std::os::unix::fs::symlink(
            outside.path(),
            repo.path().join(".tandem").join("worktrees"),
        )
        .expect("create managed-root symlink");
        let target = repo.path().join(".tandem").join("worktrees").join("orphan");

        assert!(remove_managed_worktree_dir(
            repo.path().to_str().expect("repo utf8"),
            &target,
            false,
        )
        .is_err());
        assert!(sentinel.exists(), "symlinked parent target must remain");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_git_disables_helpers_and_refuses_dirty_mutations() {
        use std::os::unix::fs::PermissionsExt;

        let repo = tempfile::tempdir().expect("repo tempdir");
        let marker = repo.path().join("fsmonitor-ran");
        let monitor = repo.path().join("monitor.sh");
        std::fs::write(&monitor, format!("#!/bin/sh\ntouch {}\n", marker.display()))
            .expect("write fsmonitor");
        let mut permissions = std::fs::metadata(&monitor)
            .expect("fsmonitor metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&monitor, permissions).expect("set fsmonitor executable");
        let filter_marker = repo.path().join("filter-ran");
        let filter = repo.path().join("filter.sh");
        std::fs::write(
            &filter,
            format!("#!/bin/sh\ntouch {}\ncat\n", filter_marker.display()),
        )
        .expect("write content filter");
        let mut filter_permissions = std::fs::metadata(&filter)
            .expect("filter metadata")
            .permissions();
        filter_permissions.set_mode(0o755);
        std::fs::set_permissions(&filter, filter_permissions).expect("set filter executable");
        let repo_root = repo.path().to_str().expect("repo utf8");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(["-C", repo_root])
                .args(args)
                .output()
                .expect("run git fixture command");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "security-test.test"]);
        git(&["config", "user.name", "Security Test"]);
        std::fs::write(repo.path().join("tracked.txt"), b"initial").expect("write tracked file");
        std::fs::write(repo.path().join("filtered.txt"), b"filtered").expect("write filtered file");
        std::fs::write(
            repo.path().join(".gitattributes"),
            b"filtered.txt filter=owned\n",
        )
        .expect("write attributes");
        git(&["add", "."]);
        git(&["commit", "-m", "initial"]);
        std::fs::write(repo.path().join("tracked.txt"), b"target").expect("write target revision");
        git(&["add", "tracked.txt"]);
        git(&["commit", "-m", "target"]);
        git(&[
            "config",
            "core.fsmonitor",
            monitor.to_str().expect("monitor utf8"),
        ]);
        git(&[
            "config",
            "filter.owned.smudge",
            filter.to_str().expect("filter utf8"),
        ]);
        git(&["config", "filter.owned.required", "true"]);

        let status = run_managed_git(repo_root, &["status", "--porcelain"])
            .await
            .expect("sanitized status");
        assert!(status.success, "{}", status.stderr);
        assert!(!marker.exists(), "configured fsmonitor must not execute");

        let worktree = repo.path().join("worktree-a");
        let addition = run_managed_git(
            repo_root,
            &[
                "worktree",
                "add",
                "-b",
                "test/worktree-a",
                worktree.to_str().expect("worktree utf8"),
                "--",
                "HEAD~1",
            ],
        )
        .await
        .expect("bounded worktree addition");
        assert!(addition.success, "{}", addition.stderr);
        assert!(
            !filter_marker.exists(),
            "configured content filter must not execute"
        );
        assert!(
            worktree_registration_matches_async(
                repo_root.to_string(),
                worktree.to_string_lossy().to_string(),
                "test/worktree-a".to_string(),
            )
            .await
            .expect("matching registration query"),
            "owned branch must match its registered worktree"
        );
        assert!(
            !worktree_registration_matches_async(
                repo_root.to_string(),
                worktree.to_string_lossy().to_string(),
                "test/other-branch".to_string(),
            )
            .await
            .expect("mismatched registration query"),
            "path-only registration must not match another branch"
        );
        std::fs::write(worktree.join("tracked.txt"), b"dirty").expect("dirty worktree");
        let reset = run_managed_git(
            worktree.to_str().expect("worktree utf8"),
            &["reset", "--keep", "main"],
        )
        .await
        .expect("bounded reset attempt");
        assert!(!reset.success, "dirty reset must be refused");
        assert_eq!(
            std::fs::read(worktree.join("tracked.txt")).expect("read dirty worktree"),
            b"dirty",
            "dirty content must remain intact"
        );
        let removal = run_managed_git(
            repo_root,
            &[
                "worktree",
                "remove",
                "--",
                worktree.to_str().expect("worktree utf8"),
            ],
        )
        .await
        .expect("bounded removal attempt");
        assert!(!removal.success);
        assert!(worktree.exists(), "dirty worktree must remain intact");
    }

    #[test]
    fn managed_worktree_key_is_stable_and_opaque() {
        let repo_root = "/srv/private/customer-repository";
        let path = "/srv/private/customer-repository/.tandem/worktrees/task-a";
        let key = managed_worktree_key(
            repo_root,
            Some("task-a"),
            Some("run-a"),
            Some("lease-a"),
            path,
            "tandem/task-a",
        );
        let repeated = managed_worktree_key(
            repo_root,
            Some("task-a"),
            Some("run-a"),
            Some("lease-a"),
            path,
            "tandem/task-a",
        );

        assert_eq!(key, repeated);
        assert!(key.starts_with("wt_"));
        assert_eq!(key.len(), 67);
        assert!(!key.contains(repo_root));
        assert!(!key.contains(path));
        assert_ne!(
            key,
            managed_worktree_key(repo_root, None, None, None, "/different", "branch")
        );
    }
}
