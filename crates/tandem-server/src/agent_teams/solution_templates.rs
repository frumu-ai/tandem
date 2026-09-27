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

    /// Create or reconcile an exact disabled template without replacing a
    /// pre-existing resource. Generic template mutation cannot activate it.
    pub async fn stage_solution_template(
        &self,
        workspace_root: &str,
        mut template: AgentTemplate,
        owner: SolutionTemplateOwner,
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
            persist_in_directory(&directory, &workspace, &filename, &payload)
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
mod tests {
    use super::*;

    fn fixture() -> (AgentTemplate, SolutionTemplateOwner) {
        let mut template: AgentTemplate = serde_json::from_str(include_str!(
            "../../../tandem-solutions/fixtures/company-brain-text/agents/central-brain.json"
        ))
        .unwrap();
        let owner = SolutionTemplateOwner {
            org_id: "org-a".into(),
            workspace_id: "workspace-a".into(),
            deployment_id: "deployment-a".into(),
            instance_id: "brain-a".into(),
            component_id: "central-brain".into(),
            composition_sha256: "a".repeat(64),
        };
        template.template_id = solution_resource_id(
            &owner.org_id,
            &owner.workspace_id,
            &owner.deployment_id,
            &owner.instance_id,
            &owner.component_id,
        )
        .unwrap();
        (template, owner)
    }

    #[tokio::test]
    async fn generic_mutations_protect_managed_filename_aliases() {
        for delete in [false, true] {
            for warm_cache in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let workspace = dir.path().to_str().unwrap();
                let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
                let (template, owner) = fixture();
                let mut ordinary = template.clone();
                ordinary.template_id = "worker".into();
                if warm_cache {
                    runtime
                        .upsert_template(workspace, ordinary.clone())
                        .await
                        .unwrap();
                }
                let parent = dir.path().join(".tandem/agent-team/templates");
                std::fs::create_dir_all(&parent).unwrap();
                let alias = parent.join("worker.yaml");
                let mut managed = template.clone();
                managed.enabled = false;
                managed.solution_owner = Some(owner.clone());
                let original = canonical_json(&managed).unwrap();
                std::fs::write(&alias, &original).unwrap();
                runtime
                    .stage_solution_template(workspace, template.clone(), owner)
                    .await
                    .unwrap();
                let cached = canonical_json(&runtime.list_templates().await).unwrap();
                let denied = if delete {
                    runtime.delete_template(workspace, "worker").await.is_err()
                } else {
                    runtime.upsert_template(workspace, ordinary).await.is_err()
                };
                assert!(denied, "delete={delete}, warm_cache={warm_cache}");
                assert_eq!(std::fs::read(&alias).unwrap(), original);
                assert_eq!(
                    canonical_json(&runtime.list_templates().await).unwrap(),
                    cached
                );
                let restarted = AgentTeamRuntime::new(dir.path().join("restart-audit"));
                let observed = restarted
                    .get_template_for_workspace(workspace, &template.template_id)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(canonical_json(&observed).unwrap(), original);
            }
        }
    }

    #[tokio::test]
    async fn generic_mutations_validate_actual_destination_before_changing_cache() {
        for delete in [false, true] {
            for representation in ["owner", "reserved", "malformed"] {
                let dir = tempfile::tempdir().unwrap();
                let workspace = dir.path().to_str().unwrap();
                let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
                let (mut ordinary, owner) = fixture();
                ordinary.template_id = "work/er".into();
                runtime
                    .upsert_template(workspace, ordinary.clone())
                    .await
                    .unwrap();
                let cached = canonical_json(&runtime.list_templates().await).unwrap();
                let path = dir.path().join(".tandem/agent-team/templates/work_er.yaml");
                let mut observed = ordinary.clone();
                let replacement = match representation {
                    "owner" => {
                        observed.solution_owner = Some(owner);
                        canonical_json(&observed).unwrap()
                    }
                    "reserved" => {
                        observed.template_id = " SOLUTION-legacy ".into();
                        serde_yaml::to_string(&observed).unwrap().into_bytes()
                    }
                    _ => b"not: [valid YAML".to_vec(),
                };
                std::fs::write(&path, &replacement).unwrap();
                let denied = if delete {
                    runtime.delete_template(workspace, "work/er").await.is_err()
                } else {
                    runtime.upsert_template(workspace, ordinary).await.is_err()
                };
                assert!(denied, "delete={delete}, representation={representation}");
                assert_eq!(std::fs::read(&path).unwrap(), replacement);
                assert_eq!(
                    canonical_json(&runtime.list_templates().await).unwrap(),
                    cached
                );
            }
        }
    }

    #[tokio::test]
    async fn generic_mutations_preserve_large_manual_template_crud() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let mut template = fixture().0;
        template.template_id = "manual".into();
        template.system_prompt = Some("x".repeat(MAX_ARTIFACT_BYTES + 1));
        runtime
            .upsert_template(workspace, template.clone())
            .await
            .unwrap();
        template.display_name = Some("Updated manual template".into());
        runtime
            .upsert_template(workspace, template.clone())
            .await
            .unwrap();
        assert_eq!(
            runtime
                .get_template_for_workspace(workspace, "manual")
                .await
                .unwrap()
                .unwrap()
                .display_name,
            template.display_name
        );
        assert!(runtime.delete_template(workspace, "manual").await.unwrap());
        assert!(!runtime.delete_template(workspace, "manual").await.unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn generic_mutations_follow_manual_symlinks_but_protect_managed_targets() {
        for managed in [false, true] {
            for delete in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let workspace = dir.path().to_str().unwrap();
                let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
                let (mut template, owner) = fixture();
                template.template_id = "worker".into();
                runtime
                    .upsert_template(workspace, template.clone())
                    .await
                    .unwrap();
                let alias = dir.path().join(".tandem/agent-team/templates/worker.yaml");
                let target = dir.path().join("source.json");
                let mut observed = template.clone();
                if managed {
                    observed.solution_owner = Some(owner);
                }
                let original = canonical_json(&observed).unwrap();
                std::fs::write(&target, &original).unwrap();
                std::fs::remove_file(&alias).unwrap();
                std::os::unix::fs::symlink(&target, &alias).unwrap();
                template.display_name = Some("changed".into());
                let result = if delete {
                    runtime
                        .delete_template(workspace, "worker")
                        .await
                        .map(|_| ())
                } else {
                    runtime
                        .upsert_template(workspace, template.clone())
                        .await
                        .map(|_| ())
                };
                assert_eq!(
                    result.is_err(),
                    managed,
                    "managed={managed}, delete={delete}"
                );
                if managed || delete {
                    assert_eq!(std::fs::read(&target).unwrap(), original);
                } else {
                    let actual: AgentTemplate =
                        serde_yaml::from_slice(&std::fs::read(&target).unwrap()).unwrap();
                    assert_eq!(actual.display_name, template.display_name);
                }
                assert_eq!(
                    std::fs::symlink_metadata(&alias).is_ok(),
                    managed || !delete
                );
            }
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn native_stage_reconciles_read_only_alias_without_mutation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let parent = dir.path().join(".tandem/agent-team/templates");
        std::fs::create_dir_all(&parent).unwrap();
        let path = parent.join("checked-in.yaml");
        let (template, owner) = fixture();
        let mut installed = template.clone();
        installed.enabled = false;
        installed.solution_owner = Some(owner.clone());
        let original = canonical_json(&installed).unwrap();
        std::fs::write(&path, &original).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let result = runtime
            .stage_solution_template(workspace, template, owner)
            .await;
        let unchanged = std::fs::read(&path).unwrap() == original
            && std::fs::metadata(&path).unwrap().permissions().mode() & 0o777 == 0o444
            && std::fs::read_dir(&parent).unwrap().count() == 1;
        // Restore only the fixture directory so TempDir can clean it up even
        // when asserting the pre-fix failure below.
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(result.unwrap(), sha256(&original));
        assert!(unchanged);
    }

    #[test]
    fn native_stage_cannot_insert_into_another_workspaces_cache() {
        // Reserve the sole blocking worker so persistence pauses after A has
        // loaded its cache, without relying on filesystem timing or sleeps.
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        executor.block_on(async {
            let workspace_a = tempfile::tempdir().unwrap();
            let workspace_b = tempfile::tempdir().unwrap();
            let a = workspace_a.path().to_str().unwrap();
            let b = workspace_b.path().to_str().unwrap();
            let runtime = AgentTeamRuntime::new(workspace_a.path().join("audit"));
            runtime.ensure_loaded_for_workspace(a).await.unwrap();
            let (release, waiting) = std::sync::mpsc::channel();
            let (started, ready) = tokio::sync::oneshot::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                let _ = started.send(());
                let _ = waiting.recv();
            });
            ready.await.unwrap();
            let (template, owner) = fixture();
            let staging = runtime.stage_solution_template(a, template.clone(), owner);
            tokio::pin!(staging);
            assert!(futures::poll!(&mut staging).is_pending());
            let switching = runtime.ensure_loaded_for_workspace(b);
            tokio::pin!(switching);
            let switched = futures::poll!(&mut switching);
            release.send(()).unwrap();
            blocker.await.unwrap();
            staging.await.unwrap();
            match switched {
                std::task::Poll::Ready(result) => result.unwrap(),
                std::task::Poll::Pending => switching.await.unwrap(),
            }
            assert!(runtime
                .get_template_for_workspace(b, &template.template_id)
                .await
                .unwrap()
                .is_none());
            assert!(runtime
                .get_template_for_workspace(a, &template.template_id)
                .await
                .unwrap()
                .is_some());
        });
    }

    #[cfg(any(unix, windows))]
    fn redirect_directory(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            // Junctions do not require Windows Developer Mode or the symlink
            // privilege and exercise the relevant reparse-point boundary.
            let output = std::process::Command::new("cmd")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "junction creation failed: {output:?}"
            );
        }
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn native_stage_rejects_redirected_managed_directories() {
        let mut accepted = Vec::new();
        for component in [
            ".tandem",
            ".tandem/agent-team",
            ".tandem/agent-team/templates",
        ] {
            for adopt in [false, true] {
                let workspace = tempfile::tempdir().unwrap();
                let outside = tempfile::tempdir().unwrap();
                let redirected = workspace.path().join(component);
                std::fs::create_dir_all(redirected.parent().unwrap()).unwrap();
                redirect_directory(outside.path(), &redirected);
                let suffix = Path::new(".tandem/agent-team/templates")
                    .strip_prefix(component)
                    .unwrap();
                let outside_templates = outside.path().join(suffix);
                let (template, owner) = fixture();
                let mut original = None;
                if adopt {
                    std::fs::create_dir_all(&outside_templates).unwrap();
                    let mut observed = template.clone();
                    observed.enabled = false;
                    observed.solution_owner = Some(owner.clone());
                    let bytes = canonical_json(&observed).unwrap();
                    std::fs::write(outside_templates.join("alias.yaml"), &bytes).unwrap();
                    original = Some(bytes);
                }
                let runtime = AgentTeamRuntime::new(workspace.path().join("audit"));
                let result = runtime
                    .stage_solution_template(workspace.path().to_str().unwrap(), template, owner)
                    .await;
                if result.is_ok() {
                    accepted.push(format!("{component}, adopt={adopt}"));
                    continue;
                }
                assert!(runtime.list_templates().await.is_empty());
                if let Some(bytes) = original {
                    assert_eq!(std::fs::read_dir(&outside_templates).unwrap().count(), 1);
                    assert_eq!(
                        std::fs::read(outside_templates.join("alias.yaml")).unwrap(),
                        bytes
                    );
                } else {
                    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
                }
            }
        }
        assert!(
            accepted.is_empty(),
            "accepted redirected directories: {accepted:?}"
        );
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn native_stage_accepts_a_symlinked_workspace_root() {
        let workspace = tempfile::tempdir().unwrap();
        let aliases = tempfile::tempdir().unwrap();
        let alias = aliases.path().join("workspace");
        redirect_directory(workspace.path(), &alias);
        let runtime = AgentTeamRuntime::new(workspace.path().join("audit"));
        let (template, owner) = fixture();
        let receipt = runtime
            .stage_solution_template(alias.to_str().unwrap(), template.clone(), owner.clone())
            .await
            .unwrap();
        assert_eq!(
            runtime
                .stage_solution_template(alias.to_str().unwrap(), template.clone(), owner)
                .await
                .unwrap(),
            receipt
        );
        assert!(workspace
            .path()
            .join(".tandem/agent-team/templates")
            .join(format!("{}.yaml", template.template_id))
            .is_file());
    }

    #[cfg(unix)]
    #[test]
    fn managed_publication_rejects_parent_replacement_after_open() {
        for component in [
            ".tandem",
            ".tandem/agent-team",
            ".tandem/agent-team/templates",
        ] {
            for symlink in [false, true] {
                let workspace = tempfile::tempdir().unwrap();
                let outside = tempfile::tempdir().unwrap();
                let directory = managed_directory(workspace.path()).unwrap();
                let replaced = workspace.path().join(component);
                let parked = workspace.path().join("parked-directory");
                std::fs::rename(&replaced, &parked).unwrap();
                if symlink {
                    std::os::unix::fs::symlink(outside.path(), &replaced).unwrap();
                } else {
                    std::fs::create_dir_all(workspace.path().join(".tandem/agent-team/templates"))
                        .unwrap();
                }
                let (mut template, owner) = fixture();
                template.enabled = false;
                template.solution_owner = Some(owner);
                let filename = format!("{}.yaml", template.template_id);
                assert!(
                    persist_in_directory(
                        &directory,
                        workspace.path(),
                        &filename,
                        &canonical_json(&template).unwrap(),
                    )
                    .is_err(),
                    "{component}, symlink={symlink}"
                );
                assert_eq!(directory.entries().unwrap().count(), 0);
                assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn staging_cache_reads_held_directory_and_preserves_cache_on_rejection() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (agent_team, directory) = managed_directories(workspace.path()).unwrap();
        let templates = workspace.path().join(".tandem/agent-team/templates");
        let parked = workspace.path().join("parked-templates");
        let mut local = fixture().0;
        local.template_id = "local-worker".into();
        let local_bytes = serde_yaml::to_string(&local).unwrap();
        std::fs::write(templates.join("worker.yaml"), &local_bytes).unwrap();
        std::os::unix::fs::symlink("worker.yaml", templates.join("linked.yaml")).unwrap();
        let mut foreign = local.clone();
        foreign.template_id = "foreign-worker".into();
        std::fs::write(
            outside.path().join("worker.yaml"),
            serde_yaml::to_string(&foreign).unwrap(),
        )
        .unwrap();
        std::fs::write(
            outside.path().join("linked.yaml"),
            serde_yaml::to_string(&foreign).unwrap(),
        )
        .unwrap();
        std::fs::rename(&templates, &parked).unwrap();
        std::os::unix::fs::symlink(outside.path(), &templates).unwrap();
        assert_eq!(
            read_native_document(&directory, Path::new("worker.yaml"), &templates)
                .unwrap()
                .unwrap(),
            local_bytes
        );
        assert_eq!(
            read_native_document(&directory, Path::new("linked.yaml"), &templates)
                .unwrap()
                .unwrap(),
            local_bytes
        );
        let runtime = AgentTeamRuntime::new(workspace.path().join("audit"));
        assert!(runtime
            .load_staging_workspace(workspace.path().to_str().unwrap(), &agent_team, &directory)
            .await
            .is_err());
        assert!(runtime.list_templates().await.is_empty());
        assert!(runtime.loaded_workspace.read().await.is_none());
        std::fs::remove_file(&templates).unwrap();
        std::fs::rename(&parked, &templates).unwrap();
        runtime
            .load_staging_workspace(workspace.path().to_str().unwrap(), &agent_team, &directory)
            .await
            .unwrap();
        let listed = runtime.list_templates().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].template_id, "local-worker");
    }

    #[tokio::test]
    async fn workspace_reads_keep_same_id_templates_separate() {
        let workspace_a = tempfile::tempdir().unwrap();
        let workspace_b = tempfile::tempdir().unwrap();
        let runtime = AgentTeamRuntime::new(workspace_a.path().join("audit"));
        let (template, owner) = fixture();
        let check_workspace = |workspace: String, name: &'static str| {
            let runtime = runtime.clone();
            let mut template = template.clone();
            let owner = owner.clone();
            async move {
                template.display_name = Some(name.into());
                runtime
                    .stage_solution_template(&workspace, template.clone(), owner)
                    .await
                    .unwrap();
                for _ in 0..20 {
                    let observed = runtime
                        .get_template_for_workspace(&workspace, &template.template_id)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(observed.display_name.as_deref(), Some(name));
                    let listed = runtime
                        .list_templates_for_workspace(&workspace)
                        .await
                        .unwrap();
                    assert_eq!(listed.len(), 1);
                    assert_eq!(listed[0].display_name.as_deref(), Some(name));
                    tokio::task::yield_now().await;
                }
            }
        };
        tokio::join!(
            check_workspace(workspace_a.path().to_str().unwrap().into(), "Workspace A"),
            check_workspace(workspace_b.path().to_str().unwrap().into(), "Workspace B"),
        );
    }

    #[tokio::test]
    async fn native_stage_preserves_distinct_dotted_component_ids() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let mut staged = Vec::new();
        for component in ["central.brain", "central_brain"] {
            let (mut template, mut owner) = fixture();
            owner.component_id = component.into();
            template.template_id = solution_resource_id(
                &owner.org_id,
                &owner.workspace_id,
                &owner.deployment_id,
                &owner.instance_id,
                &owner.component_id,
            )
            .unwrap();
            let receipt = runtime
                .stage_solution_template(workspace, template.clone(), owner.clone())
                .await
                .unwrap();
            staged.push((template, owner, receipt));
        }
        let directory = dir.path().join(".tandem/agent-team/templates");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        let restarted = AgentTeamRuntime::new(dir.path().join("restart-audit"));
        for (template, owner, receipt) in staged {
            assert!(directory
                .join(format!("{}.yaml", template.template_id))
                .is_file());
            let observed = restarted
                .get_template_for_workspace(workspace, &template.template_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(observed.solution_owner, Some(owner.clone()));
            assert!(!observed.enabled);
            assert_eq!(
                restarted
                    .stage_solution_template(workspace, template.clone(), owner)
                    .await
                    .unwrap(),
                receipt
            );
            assert!(restarted
                .upsert_template(workspace, template.clone())
                .await
                .is_err());
            assert!(restarted
                .delete_template(workspace, &template.template_id)
                .await
                .is_err());
        }
    }

    #[tokio::test]
    async fn staged_template_survives_restart_and_reconciles_without_activation() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit.jsonl"));
        let (template, owner) = fixture();
        let receipt = runtime
            .stage_solution_template(workspace, template.clone(), owner.clone())
            .await
            .unwrap();
        // Simulate a crash after native publication but before journal receipt.
        let restarted = AgentTeamRuntime::new(dir.path().join("audit.jsonl"));
        let observed = restarted
            .get_template_for_workspace(workspace, &template.template_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!observed.enabled);
        assert_eq!(observed.solution_owner, Some(owner.clone()));
        assert_eq!(sha256(&canonical_json(&observed).unwrap()), receipt);
        assert_eq!(
            restarted
                .stage_solution_template(workspace, template.clone(), owner.clone())
                .await
                .unwrap(),
            receipt
        );
        let mut changed = template.clone();
        changed.system_prompt = Some("changed content".into());
        assert!(restarted
            .stage_solution_template(workspace, changed, owner.clone())
            .await
            .is_err());
        let mut other_owner = owner;
        other_owner.org_id = "org-b".into();
        assert!(restarted
            .stage_solution_template(workspace, template.clone(), other_owner)
            .await
            .is_err());
        assert!(restarted
            .upsert_template(workspace, template.clone())
            .await
            .is_err());
        assert!(restarted
            .delete_template(workspace, &template.template_id)
            .await
            .is_err());
        for alias in [
            format!(" {} ", template.template_id),
            template.template_id.to_ascii_uppercase(),
        ] {
            let mut aliased = template.clone();
            aliased.template_id = alias.clone();
            assert!(restarted.upsert_template(workspace, aliased).await.is_err());
            assert!(restarted.delete_template(workspace, &alias).await.is_err());
        }
        assert!(
            !restarted
                .get_template_for_workspace(workspace, &template.template_id)
                .await
                .unwrap()
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn native_staged_template_denies_spawn_even_with_approval_override() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let (template, owner) = fixture();
        runtime
            .stage_solution_template(workspace, template.clone(), owner)
            .await
            .unwrap();
        let observed = runtime
            .get_template_for_workspace(workspace, &template.template_id)
            .await
            .unwrap()
            .unwrap();
        let state = crate::test_support::test_state().await;
        let policy = serde_json::from_value(serde_json::json!({
            "enabled": true, "require_justification": false
        }))
        .unwrap();
        runtime
            .set_for_test(
                Some(state.workspace_index.snapshot().await.root),
                Some(policy),
                vec![observed],
            )
            .await;
        let request = tandem_orchestrator::SpawnRequest {
            mission_id: Some("staging-test".into()),
            parent_instance_id: None,
            source: tandem_orchestrator::SpawnSource::UiAction,
            parent_role: None,
            role: tandem_orchestrator::AgentRole::Worker,
            template_id: Some(template.template_id),
            justification: "approved request".into(),
            budget_override: None,
        };
        for missing_from_cache in [false, true] {
            if missing_from_cache {
                runtime.templates.write().await.clear();
            }
            for override_approval in [false, true] {
                let result = runtime
                    .spawn_with_approval_override(&state, request.clone(), override_approval)
                    .await;
                assert!(!result.decision.allowed);
                assert_eq!(
                    result.decision.code,
                    Some(tandem_orchestrator::SpawnDenyCode::SpawnTemplateDisabled)
                );
                assert!(result.instance.is_none());
            }
        }
        assert!(runtime.list_spawn_approvals().await.is_empty());
        assert!(runtime.list_instances(None, None, None).await.is_empty());
    }

    #[tokio::test]
    async fn concurrent_native_writers_reconcile_one_complete_resource() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let first = AgentTeamRuntime::new(dir.path().join("first-audit"));
        let second = AgentTeamRuntime::new(dir.path().join("second-audit"));
        let (template, owner) = fixture();
        let (a, b) = tokio::join!(
            first.stage_solution_template(workspace, template.clone(), owner.clone()),
            second.stage_solution_template(workspace, template, owner)
        );
        assert_eq!(a.unwrap(), b.unwrap());
        let files = std::fs::read_dir(dir.path().join(".tandem/agent-team/templates"))
            .unwrap()
            .count();
        assert_eq!(files, 1);
    }

    #[tokio::test]
    async fn native_stage_preserves_manual_file_and_rejects_artifact_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let (mut template, owner) = fixture();
        template.solution_owner = Some(owner.clone());
        assert!(runtime
            .stage_solution_template(workspace, template.clone(), owner.clone())
            .await
            .is_err());
        assert!(!dir.path().join(".tandem").exists());
        template.solution_owner = None;
        let path = dir
            .path()
            .join(".tandem/agent-team/templates")
            .join(format!("{}.yaml", template.template_id));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let manual = canonical_json(&template).unwrap();
        std::fs::write(&path, &manual).unwrap();
        assert!(runtime
            .stage_solution_template(workspace, template, owner)
            .await
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), manual);
    }

    #[tokio::test]
    async fn native_stage_rejects_resource_identity_substitution_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let (template, owner) = fixture();
        for field in ["org", "workspace", "deployment", "instance", "component"] {
            let mut changed = owner.clone();
            match field {
                "org" => changed.org_id.push_str("-other"),
                "workspace" => changed.workspace_id.push_str("-other"),
                "deployment" => changed.deployment_id.push_str("-other"),
                "instance" => changed.instance_id.push_str("-other"),
                "component" => changed.component_id.push_str("-other"),
                _ => unreachable!(),
            }
            assert!(
                runtime
                    .stage_solution_template(workspace, template.clone(), changed)
                    .await
                    .is_err(),
                "{field}"
            );
        }
        let mut arbitrary = template;
        arbitrary.template_id = "solution-unbound-central-brain".into();
        assert!(runtime
            .stage_solution_template(workspace, arbitrary, owner)
            .await
            .is_err());
        assert!(!dir.path().join(".tandem").exists());
        assert!(runtime.templates.read().await.is_empty());
    }

    #[tokio::test]
    async fn native_stage_reconciles_disk_repair_without_restarting_cache() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let (template, owner) = fixture();
        let directory = dir.path().join(".tandem/agent-team/templates");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("operator-alias.json");
        std::fs::write(&path, canonical_json(&template).unwrap()).unwrap();
        assert!(runtime
            .stage_solution_template(workspace, template.clone(), owner.clone())
            .await
            .is_err());
        assert!(runtime
            .templates
            .read()
            .await
            .contains_key(&template.template_id));

        let mut repaired = template.clone();
        repaired.enabled = false;
        repaired.solution_owner = Some(owner.clone());
        let bytes = canonical_json(&repaired).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            runtime
                .stage_solution_template(workspace, template.clone(), owner)
                .await
                .unwrap(),
            sha256(&bytes)
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        assert_eq!(
            canonical_json(&runtime.templates.read().await[&template.template_id]).unwrap(),
            bytes
        );
    }

    #[tokio::test]
    async fn native_stage_reconciles_aliases_and_rechecks_durable_content() {
        for extension in ["json", "yml", "YAML"] {
            let dir = tempfile::tempdir().unwrap();
            let workspace = dir.path().to_str().unwrap();
            let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
            let (template, owner) = fixture();
            let mut installed = template.clone();
            installed.enabled = false;
            installed.solution_owner = Some(owner.clone());
            let directory = dir.path().join(".tandem/agent-team/templates");
            std::fs::create_dir_all(&directory).unwrap();
            let alias = directory.join(format!("existing-resource.{extension}"));
            let bytes = canonical_json(&installed).unwrap();
            std::fs::write(&alias, &bytes).unwrap();
            assert_eq!(
                runtime
                    .stage_solution_template(workspace, template.clone(), owner.clone())
                    .await
                    .unwrap(),
                sha256(&bytes)
            );
            assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
            assert_eq!(std::fs::read(&alias).unwrap(), bytes);
            installed.system_prompt = Some("changed on disk after cache load".into());
            std::fs::write(&alias, canonical_json(&installed).unwrap()).unwrap();
            assert!(runtime
                .stage_solution_template(workspace, template, owner)
                .await
                .is_err());
            assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        }
    }

    #[tokio::test]
    async fn native_stage_rejects_unhashed_alias_fields() {
        for extension in ["json", "yml"] {
            for nested in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let workspace = dir.path().to_str().unwrap();
                let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
                let (template, owner) = fixture();
                let mut installed = template.clone();
                installed.enabled = false;
                installed.solution_owner = Some(owner.clone());
                let mut document = serde_json::to_value(installed).unwrap();
                let target = if nested {
                    &mut document["solution_owner"]
                } else {
                    &mut document
                };
                target.as_object_mut().unwrap().insert(
                    "unreviewed".into(),
                    serde_json::json!({"payload":"not in receipt"}),
                );
                let bytes = if extension == "json" {
                    serde_json::to_vec(&document).unwrap()
                } else {
                    serde_yaml::to_string(&document).unwrap().into_bytes()
                };
                let directory = dir.path().join(".tandem/agent-team/templates");
                std::fs::create_dir_all(&directory).unwrap();
                let alias = directory.join(format!("existing-resource.{extension}"));
                std::fs::write(&alias, &bytes).unwrap();
                assert!(
                    runtime
                        .stage_solution_template(workspace, template, owner)
                        .await
                        .is_err(),
                    "{extension}, nested={nested}"
                );
                assert_eq!(std::fs::read(&alias).unwrap(), bytes);
                assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
            }
        }
    }

    #[tokio::test]
    async fn native_stage_preserves_unrelated_large_templates() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let directory = dir.path().join(".tandem/agent-team/templates");
        std::fs::create_dir_all(&directory).unwrap();
        let mut unrelated = fixture().0;
        unrelated.template_id = "user-worker".into();
        unrelated.system_prompt = Some("x".repeat(MAX_ARTIFACT_BYTES + 1));
        let bytes = canonical_json(&unrelated).unwrap();
        let path = directory.join("worker.json");
        std::fs::write(&path, &bytes).unwrap();
        let (template, owner) = fixture();
        runtime
            .stage_solution_template(workspace, template, owner)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(runtime
            .get_template_for_workspace(workspace, "user-worker")
            .await
            .unwrap()
            .is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_stage_preserves_unrelated_symlinks_but_rejects_matching_alias() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let directory = dir.path().join(".tandem/agent-team/templates");
        std::fs::create_dir_all(&directory).unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let mut unrelated = fixture().0;
        unrelated.template_id = "user-worker".into();
        let external = dir.path().join("external.json");
        let bytes = canonical_json(&unrelated).unwrap();
        std::fs::write(&external, &bytes).unwrap();
        let alias = directory.join("worker.json");
        std::os::unix::fs::symlink(&external, &alias).unwrap();
        let (template, owner) = fixture();
        runtime
            .stage_solution_template(workspace, template.clone(), owner.clone())
            .await
            .unwrap();
        assert_eq!(std::fs::read(&external).unwrap(), bytes);
        assert!(std::fs::symlink_metadata(&alias)
            .unwrap()
            .file_type()
            .is_symlink());
        let mut matching = template.clone();
        matching.enabled = false;
        matching.solution_owner = Some(owner.clone());
        let matching_bytes = canonical_json(&matching).unwrap();
        std::fs::write(&external, &matching_bytes).unwrap();
        assert!(runtime
            .stage_solution_template(workspace, template, owner)
            .await
            .is_err());
        assert_eq!(std::fs::read(&external).unwrap(), matching_bytes);
    }

    #[tokio::test]
    async fn native_stage_persists_in_the_normalized_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_str().unwrap();
        let runtime = AgentTeamRuntime::new(dir.path().join("audit"));
        let (template, owner) = fixture();
        runtime
            .stage_solution_template(&format!(" {workspace} "), template.clone(), owner)
            .await
            .unwrap();
        let restarted = AgentTeamRuntime::new(dir.path().join("audit"));
        let restored = restarted
            .get_template_for_workspace(workspace, &template.template_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!restored.enabled);
        assert_eq!(
            std::fs::read_dir(dir.path().join(".tandem/agent-team/templates"))
                .unwrap()
                .count(),
            1
        );
    }
}
