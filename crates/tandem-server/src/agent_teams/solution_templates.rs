// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Native AgentTemplate staging. Callers must authorize the installation and
//! revalidate its journal, signed artifacts and current host facts first. A
//! returned fingerprint is an observed disabled resource, never activation.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::ensure;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use tandem_orchestrator::{AgentTemplate, SolutionTemplateOwner, SpawnPolicy};
use tandem_solutions::{canonical_json, sha256, solution_resource_id, MAX_ARTIFACT_BYTES};
use tandem_types::VerifiedTenantContext;

use super::AgentTeamRuntime;

impl AgentTeamRuntime {
    pub(super) async fn require_unmanaged_template_destination(path: &Path) -> anyhow::Result<()> {
        match tokio::fs::symlink_metadata(path).await {
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        // Follow supported manual-template symlinks, but never interpret a
        // dangling link, special file or unreadable document as an absent file.
        ensure!(
            tokio::fs::metadata(path).await?.is_file(),
            "template destination is not a regular file"
        );
        let raw = tokio::fs::read_to_string(path).await?;
        let existing: AgentTemplate = serde_yaml::from_str(&raw)?;
        ensure!(
            existing.solution_owner.is_none()
                && !Self::template_filename(&existing.template_id)
                    .to_ascii_lowercase()
                    .starts_with("solution-"),
            "solution templates require the installation lifecycle"
        );
        Ok(())
    }

    /// Observe an already staged receipt without recreating a missing file or
    /// trusting a stale cache. Ownership is checked before exposing a digest.
    pub async fn observe_solution_template(
        &self,
        workspace_root: &str,
        resource_id: &str,
        owner: &SolutionTemplateOwner,
    ) -> anyhow::Result<String> {
        ensure!(
            resource_id.starts_with("solution-")
                && resource_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)),
            "invalid solution resource ID"
        );
        let _operation = self.template_persistence.lock().await;
        let workspace = PathBuf::from(workspace_root);
        let owner = owner.clone();
        let resource_id = resource_id.to_owned();
        tokio::task::spawn_blocking(move || {
            // Observe through the same retained, nonredirectable directory
            // capability as staging, without creating any missing parent.
            let mut directory = Dir::open_ambient_dir(&workspace, cap_std::ambient_authority())?;
            for component in [".tandem", "agent-team", "templates"] {
                directory = directory.open_dir_nofollow(component)?;
            }
            let path = existing_template_path(&directory, &workspace, &resource_id)?
                .ok_or_else(|| anyhow::anyhow!("staged solution template is missing"))?;
            let mut raw = Vec::new();
            open_regular(&directory, &path)?
                .take(MAX_ARTIFACT_BYTES as u64 + 1)
                .read_to_end(&mut raw)?;
            ensure!(
                raw.len() <= MAX_ARTIFACT_BYTES,
                "solution template is too large"
            );
            let observed: AgentTemplate = serde_yaml::from_slice(&raw)?;
            let document: serde_json::Value = serde_yaml::from_slice(&raw)?;
            ensure!(
                !observed.enabled
                    && observed.template_id == resource_id
                    && observed.solution_owner.as_ref() == Some(&owner),
                "solution template ownership or activation conflict"
            );
            let canonical = canonical_json(&observed)?;
            ensure!(
                canonical_json(&document)? == canonical,
                "solution template contains unrecognized content"
            );
            ensure_directory_binding(&directory, &workspace)?;
            Ok(sha256(&canonical))
        })
        .await?
    }

    /// Create or reconcile an exact disabled template without replacing a
    /// pre-existing resource. Generic template mutation cannot activate it.
    pub async fn stage_solution_template(
        &self,
        workspace_root: &str,
        template: AgentTemplate,
        owner: SolutionTemplateOwner,
    ) -> anyhow::Result<String> {
        self.stage_solution_template_inner(workspace_root, template, owner, None)
            .await
    }

    /// The bounded native commit retains policy publication authority even if
    /// its HTTP caller is cancelled while persistence is running.
    pub(crate) async fn stage_solution_template_authorized(
        &self,
        workspace_root: &str,
        template: AgentTemplate,
        owner: SolutionTemplateOwner,
        state: crate::AppState,
        verified: VerifiedTenantContext,
        publication: tokio::sync::OwnedMutexGuard<()>,
    ) -> anyhow::Result<String> {
        let runtime = self.clone();
        let workspace = workspace_root.to_owned();
        tokio::spawn(async move {
            let _publication = publication;
            runtime
                .stage_solution_template_inner(&workspace, template, owner, Some((state, verified)))
                .await
        })
        .await?
    }

    #[cfg(test)]
    pub(crate) async fn lock_solution_template_writer_for_test(
        &self,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.template_persistence.clone().lock_owned().await
    }

    async fn stage_solution_template_inner(
        &self,
        workspace_root: &str,
        mut template: AgentTemplate,
        owner: SolutionTemplateOwner,
        authorization: Option<(crate::AppState, VerifiedTenantContext)>,
    ) -> anyhow::Result<String> {
        validate_owner(&template, &owner)?;
        template.enabled = false;
        template.solution_owner = Some(owner);
        let payload = canonical_json(&template)?;
        ensure!(
            payload.len() <= MAX_ARTIFACT_BYTES,
            "solution template is too large"
        );
        let fingerprint = sha256(&payload);
        let _operation = self.template_persistence.lock().await;
        let workspace = PathBuf::from(workspace_root.trim());
        // Reject redirected managed parents before native loading can populate
        // the cache, and retain this directory for every publication operation.
        let opening_workspace = workspace.clone();
        let (agent_team, directory) =
            tokio::task::spawn_blocking(move || managed_directories(&opening_workspace)).await??;
        self.load_staging_workspace(workspace_root, &agent_team, &directory)
            .await?;
        // Reconcile against the held directory, not a cache that may predate
        // an operator's on-disk repair. Persistence verifies the full document.
        let filename = format!("{}.yaml", template.template_id);
        tokio::task::spawn_blocking(move || {
            let persist = || persist_in_directory(&directory, &workspace, &filename, &payload);
            if let Some((state, verified)) = authorization {
                state
                    .enterprise
                    .hosted_policy
                    .with_current_permission(
                        Some(&verified),
                        tandem_enterprise_contract::AccessPermission::HostedAdmin,
                        persist,
                    )
                    .map_err(anyhow::Error::msg)?
            } else {
                persist()
            }
        })
        .await??;
        self.templates
            .write()
            .await
            .insert(template.template_id.clone(), template);
        Ok(fingerprint)
    }

    async fn load_staging_workspace(
        &self,
        workspace_root: &str,
        agent_team: &Dir,
        directory: &Dir,
    ) -> anyhow::Result<()> {
        let normalized = workspace_root.trim();
        if self.loaded_workspace.read().await.as_deref() == Some(normalized) {
            return Ok(());
        }
        let workspace = PathBuf::from(normalized);
        let agent_team = agent_team.try_clone()?;
        let directory = directory.try_clone()?;
        let (policy, templates) = tokio::task::spawn_blocking(move || {
            let policy = read_native_document(
                &agent_team,
                Path::new("spawn-policy.yaml"),
                &workspace.join(".tandem/agent-team"),
            )?
            .map(|raw| serde_yaml::from_str::<SpawnPolicy>(&raw))
            .transpose()?;
            let mut templates = HashMap::new();
            for entry in directory.entries()? {
                let name = PathBuf::from(entry?.file_name());
                let extension = name
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !matches!(extension.as_str(), "yaml" | "yml" | "json") {
                    continue;
                }
                if let Some(raw) = read_native_document(
                    &directory,
                    &name,
                    &workspace.join(".tandem/agent-team/templates"),
                )? {
                    let template: AgentTemplate = serde_yaml::from_str(&raw)?;
                    templates.insert(template.template_id.clone(), template);
                }
            }
            ensure_directory_binding(&directory, &workspace)?;
            Ok::<_, anyhow::Error>((policy, templates))
        })
        .await??;
        *self.policy.write().await = policy;
        *self.templates.write().await = templates;
        *self.loaded_workspace.write().await = Some(normalized.to_owned());
        Ok(())
    }
}

fn validate_owner(template: &AgentTemplate, owner: &SolutionTemplateOwner) -> anyhow::Result<()> {
    ensure!(
        template.template_id.starts_with("solution-")
            && template.template_id.len() <= 240
            && template
                .template_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
        "solution template requires a portable stable resource ID"
    );
    for reference in [
        &owner.org_id,
        &owner.workspace_id,
        &owner.deployment_id,
        &owner.instance_id,
        &owner.component_id,
    ] {
        ensure!(
            !reference.is_empty()
                && reference.len() <= 256
                && reference.trim() == reference
                && !reference.chars().any(char::is_control),
            "solution template requires complete ownership"
        );
    }
    ensure!(
        owner.composition_sha256.len() == 64
            && owner
                .composition_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "solution template requires a reviewed composition digest"
    );
    ensure!(
        template.solution_owner.is_none(),
        "artifact must not supply installation ownership"
    );
    ensure!(
        template.template_id
            == solution_resource_id(
                &owner.org_id,
                &owner.workspace_id,
                &owner.deployment_id,
                &owner.instance_id,
                &owner.component_id,
            )?,
        "solution template resource ID does not match its owner"
    );
    Ok(())
}

#[cfg(all(test, unix))]
fn managed_directory(workspace: &Path) -> anyhow::Result<Dir> {
    Ok(managed_directories(workspace)?.1)
}

fn managed_directories(workspace: &Path) -> anyhow::Result<(Dir, Dir)> {
    // The selected workspace root may itself be a symlink. Only descendants
    // are managed paths; walk each once without following symlinks and retain
    // the resulting directory capability for every subsequent operation.
    let mut directory = Dir::open_ambient_dir(workspace, cap_std::ambient_authority())?;
    let mut agent_team = None;
    for component in [".tandem", "agent-team", "templates"] {
        directory = match directory.open_dir_nofollow(component) {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match directory.create_dir(component) {
                    Ok(()) => (),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                    Err(error) => return Err(error.into()),
                }
                directory.open_dir_nofollow(component)?
            }
            Err(error) => return Err(error.into()),
        };
        if component == "agent-team" {
            agent_team = Some(directory.try_clone()?);
        }
    }
    Ok((
        agent_team.ok_or_else(|| anyhow::anyhow!("missing agent-team directory"))?,
        directory,
    ))
}

fn read_native_document(
    directory: &Dir,
    name: &Path,
    ambient_parent: &Path,
) -> anyhow::Result<Option<String>> {
    let metadata = match directory.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return read_manual_link(directory, name, ambient_parent);
    }
    if !metadata.is_file() {
        return Ok(None);
    }
    let mut raw = String::new();
    open_regular(directory, name)?.read_to_string(&mut raw)?;
    Ok(Some(raw))
}

fn read_manual_link(
    directory: &Dir,
    name: &Path,
    ambient_parent: &Path,
) -> anyhow::Result<Option<String>> {
    #[cfg(unix)]
    {
        use rustix::fs::{openat, Mode, OFlags};
        let _ = ambient_parent;
        // Manual leaf links intentionally permit external read-only targets.
        // Resolve the leaf from the retained parent descriptor so replacement
        // of its ambient pathname cannot substitute a different document.
        let descriptor = match openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
                return Ok(None)
            }
            Err(error) => return Err(error.into()),
        };
        let mut file = std::fs::File::from(descriptor);
        if !file.metadata()?.is_file() {
            return Ok(None);
        }
        let mut raw = String::new();
        file.read_to_string(&mut raw)?;
        Ok(Some(raw))
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        // On Windows, cap-std retains directory handles without SHARE_DELETE,
        // preventing replacement of these parents while this read is active.
        let path = ambient_parent.join(name);
        if !path.is_file() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_to_string(path)?))
    }
}

fn persist_in_directory(
    directory: &Dir,
    workspace: &Path,
    filename: &str,
    payload: &[u8],
) -> anyhow::Result<()> {
    ensure_directory_binding(directory, workspace)?;
    let template: AgentTemplate = serde_json::from_slice(payload)?;
    if let Some(existing) = existing_template_path(directory, workspace, &template.template_id)? {
        // Verify the actual durable alias, not only a possibly stale cache.
        verify_disabled(directory, &existing, payload)?;
        return ensure_directory_binding(directory, workspace);
    }
    let temporary = format!(".solution-{}.tmp", uuid::Uuid::new_v4());
    let result = (|| {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = directory.open_with(&temporary, &options)?;
        file.write_all(payload)?;
        file.sync_all()?;
        drop(file);
        // Hard-link publication is atomic and never replaces the destination.
        // An interrupted writer leaves only an ignored .tmp or the whole file.
        match directory.hard_link(&temporary, directory, filename) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        // New bytes were synced through the writable temporary handle above.
        // Sync publication here; reconciliation of an existing durable alias
        // must not require a writable file or directory.
        #[cfg(unix)]
        // Directory capabilities may use O_PATH handles, which cannot fsync.
        // Open the same directory read-only through the capability, not its
        // ambient pathname, to obtain a syncable descriptor.
        directory.open(".")?.sync_all()?;
        verify_disabled(directory, Path::new(filename), payload)?;
        ensure_directory_binding(directory, workspace)
    })();
    let _ = directory.remove_file(temporary);
    result
}

fn ensure_directory_binding(directory: &Dir, workspace: &Path) -> anyhow::Result<()> {
    use cap_fs_ext::MetadataExt;

    // Publication itself never re-resolves an ambient parent path. Also refuse
    // a receipt if the selected pathname now names a different directory.
    let mut current = Dir::open_ambient_dir(workspace, cap_std::ambient_authority())?;
    for component in [".tandem", "agent-team", "templates"] {
        current = current.open_dir_nofollow(component)?;
    }
    let expected = directory.dir_metadata()?;
    let observed = current.dir_metadata()?;
    ensure!(
        expected.dev() == observed.dev() && expected.ino() == observed.ino(),
        "solution template directory changed during staging"
    );
    Ok(())
}

fn existing_template_path(
    directory: &Dir,
    workspace: &Path,
    template_id: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let mut found = None;
    for entry in directory.entries()? {
        let name = PathBuf::from(entry?.file_name());
        let extension = name
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "yaml" | "yml" | "json") {
            continue;
        }
        let metadata = directory.symlink_metadata(&name)?;
        let is_symlink = metadata.file_type().is_symlink();
        if !metadata.is_file() && !is_symlink {
            continue;
        }
        let Some(raw) = read_native_document(
            directory,
            &name,
            &workspace.join(".tandem/agent-team/templates"),
        )?
        else {
            continue;
        };
        let observed: AgentTemplate = serde_yaml::from_str(&raw)?;
        if observed.template_id == template_id {
            ensure!(!is_symlink, "solution template alias must not be a symlink");
            ensure!(found.is_none(), "duplicate solution template aliases");
            found = Some(name);
        }
    }
    Ok(found)
}

fn open_regular(directory: &Dir, name: &Path) -> anyhow::Result<cap_std::fs::File> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let file = directory.open_with(name, &options)?;
    ensure!(
        file.metadata()?.is_file(),
        "solution template destination is not a regular file"
    );
    Ok(file)
}

fn verify_disabled(directory: &Dir, name: &Path, payload: &[u8]) -> anyhow::Result<()> {
    let file = open_regular(directory, name)?;
    let mut existing = Vec::new();
    (&file)
        .take(MAX_ARTIFACT_BYTES as u64 + 1)
        .read_to_end(&mut existing)?;
    ensure!(
        existing.len() <= MAX_ARTIFACT_BYTES,
        "existing template is too large"
    );
    let observed: AgentTemplate = serde_yaml::from_slice(&existing)?;
    // Typed deserialization ignores unknown fields. Bind the receipt to the
    // complete document as well, including nested ownership/constraint data.
    let document: serde_json::Value = serde_yaml::from_slice(&existing)?;
    ensure!(
        canonical_json(&observed)? == payload && canonical_json(&document)? == payload,
        "solution template ownership or content conflict"
    );
    Ok(())
}

#[cfg(test)]
#[path = "solution_templates_tests.rs"]
mod tests;
