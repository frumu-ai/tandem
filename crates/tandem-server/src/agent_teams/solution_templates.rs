// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Native AgentTemplate staging. Callers must authorize the installation and
//! revalidate its journal, signed artifacts and current host facts first. A
//! returned fingerprint is an observed disabled resource, never activation.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::ensure;
use tandem_orchestrator::{AgentTemplate, SolutionTemplateOwner};
use tandem_solutions::{canonical_json, sha256, MAX_ARTIFACT_BYTES};

use super::AgentTeamRuntime;

impl AgentTeamRuntime {
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
        self.ensure_loaded_for_workspace(workspace_root).await?;
        let path = PathBuf::from(workspace_root)
            .join(".tandem/agent-team/templates")
            .join(Self::template_filename(&template.template_id));
        // Existing YAML/JSON aliases must not create duplicate template IDs.
        if let Some(existing) = self.templates.read().await.get(&template.template_id) {
            ensure!(
                canonical_json(existing)? == payload,
                "solution template ownership or content conflict"
            );
        }
        tokio::task::spawn_blocking(move || persist_disabled(&path, &payload)).await??;
        self.templates
            .write()
            .await
            .insert(template.template_id.clone(), template);
        Ok(fingerprint)
    }
}

fn validate_owner(template: &AgentTemplate, owner: &SolutionTemplateOwner) -> anyhow::Result<()> {
    ensure!(
        template.template_id.starts_with("solution-")
            && template.template_id.len() <= 240
            && template
                .template_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
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
    Ok(())
}

fn persist_disabled(path: &Path, payload: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing template directory"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".solution-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(payload)?;
        file.sync_all()?;
        drop(file);
        // Hard-link publication is atomic and never replaces the destination.
        // An interrupted writer leaves only an ignored .tmp or the whole file.
        match std::fs::hard_link(&temporary, path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        ensure!(
            std::fs::symlink_metadata(path)?.file_type().is_file(),
            "solution template destination is not a regular file"
        );
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        let mut existing = Vec::new();
        (&file)
            .take(MAX_ARTIFACT_BYTES as u64 + 1)
            .read_to_end(&mut existing)?;
        ensure!(
            existing.len() <= MAX_ARTIFACT_BYTES,
            "existing template is too large"
        );
        let observed: AgentTemplate = serde_yaml::from_slice(&existing)?;
        ensure!(
            canonical_json(&observed)? == payload,
            "solution template ownership or content conflict"
        );
        file.sync_all()?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    let _ = std::fs::remove_file(temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (AgentTemplate, SolutionTemplateOwner) {
        let mut template: AgentTemplate = serde_json::from_str(include_str!(
            "../../../tandem-solutions/fixtures/company-brain-text/agents/central-brain.json"
        ))
        .unwrap();
        template.template_id = "solution-test-central-brain".into();
        let owner = SolutionTemplateOwner {
            org_id: "org-a".into(),
            workspace_id: "workspace-a".into(),
            deployment_id: "deployment-a".into(),
            instance_id: "brain-a".into(),
            component_id: "central-brain".into(),
            composition_sha256: "a".repeat(64),
        };
        (template, owner)
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
            .join(".tandem/agent-team/templates/solution-test-central-brain.yaml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let manual = canonical_json(&template).unwrap();
        std::fs::write(&path, &manual).unwrap();
        assert!(runtime
            .stage_solution_template(workspace, template, owner)
            .await
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), manual);
    }
}
