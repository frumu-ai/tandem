use std::io::Write;

use super::*;

#[cfg(test)]
#[path = "permissions_batch_tests.rs"]
mod tests;

impl PermissionManager {
    /// Install a session's complete rule batch as one guarded transaction.
    ///
    /// The guard runs after all store-lock waits and file staging. It must call
    /// `commit` exactly once under current authority and return its result; it must
    /// not await or reenter this manager. The blocking worker owns the store
    /// guards, so cancelling the caller cannot unlock an in-flight publication.
    pub async fn add_rules_for_session_with_commit_guard(
        &self,
        tenant_context: &TenantContext,
        session_id: &str,
        inputs: Vec<(String, String, PermissionAction)>,
        guard: impl FnOnce(&mut dyn FnMut() -> anyhow::Result<()>) -> anyhow::Result<()>
            + Send
            + 'static,
    ) -> anyhow::Result<()> {
        self.add_rules_for_session_with_staging_hook(
            tenant_context,
            session_id,
            inputs,
            guard,
            || {},
        )
        .await
    }

    async fn add_rules_for_session_with_staging_hook(
        &self,
        tenant_context: &TenantContext,
        session_id: &str,
        inputs: Vec<(String, String, PermissionAction)>,
        guard: impl FnOnce(&mut dyn FnMut() -> anyhow::Result<()>) -> anyhow::Result<()>
            + Send
            + 'static,
        after_staging: impl FnOnce() + Send + 'static,
    ) -> anyhow::Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }
        let transaction_guard = self.state_write_lock.clone().lock_owned().await;
        let rules_guard = self.rules.clone().write_owned().await;
        let mut candidate = rules_guard.clone();
        let mut changed = false;
        for (permission, pattern, action) in inputs {
            if candidate.iter().any(|existing| {
                permission_tenant_matches(&existing.tenant_context, tenant_context)
                    && existing.session_id.as_deref() == Some(session_id)
                    && existing.permission == permission
                    && existing.pattern == pattern
                    && std::mem::discriminant(&existing.action) == std::mem::discriminant(&action)
            }) {
                continue;
            }
            candidate.push(PermissionRule {
                id: Uuid::new_v4().to_string(),
                tenant_context: tenant_context.clone(),
                session_id: Some(session_id.to_string()),
                permission,
                pattern,
                action,
                created_at_ms: Some(now_ms()),
                created_by: Some("system".to_string()),
                source_request_id: None,
                provenance: Some("default_or_system_rule".to_string()),
            });
            changed = true;
        }
        // Read the other collections before entering the synchronous commit.
        // Reusing persist_state_unlocked while retaining rules_guard would
        // deadlock when that helper tries to read the rules lock again.
        let mut file = PermissionStateFile {
            schema_version: PERMISSION_STATE_SCHEMA_VERSION,
            requests: self.requests.read().await.clone(),
            rules: candidate,
            decisions: self.decisions.read().await.clone(),
        };
        let path = self.state_path.read().await.clone();
        tokio::task::spawn_blocking(move || {
            let _transaction_guard = transaction_guard;
            let mut live_rules = rules_guard;
            // Staging is not authority publication. Finish serialization and
            // fallible file I/O before the final guard so claim/grant expiry
            // during preparation cannot admit an already-stale batch.
            let mut prepared = Some(if changed {
                path.as_ref()
                    .map(|path| prepare_permission_state_file(path, &file))
                    .transpose()
            } else {
                Ok(None)
            });
            after_staging();
            let mut committed = false;
            let result = {
                let mut commit = || {
                    anyhow::ensure!(!committed, "permission rule batch already committed");
                    let staged = prepared
                        .take()
                        .context("permission rule batch commit already attempted")??;
                    if changed {
                        if let Some(staged) = staged.as_ref() {
                            staged.publish()?;
                        }
                        *live_rules = std::mem::take(&mut file.rules);
                    }
                    committed = true;
                    if let Some(staged) = staged {
                        staged.sync_parent();
                    }
                    Ok(())
                };
                guard(&mut commit)
            };
            if committed {
                return result.context("permission rule guard failed after batch commit");
            }
            result?;
            anyhow::bail!("permission rule guard skipped batch commit")
        })
        .await
        .context("permission rule batch worker failed")?
    }
}

struct PreparedPermissionStateFile {
    path: PathBuf,
    temporary: PathBuf,
}

impl PreparedPermissionStateFile {
    fn publish(&self) -> anyhow::Result<()> {
        std::fs::rename(&self.temporary, &self.path)
            .context("failed to replace permission state file")
    }

    fn sync_parent(&self) {
        // Atomic publication has completed. A directory-sync error cannot
        // honestly be reported as an unapplied transaction.
        if let Some(parent) = self.path.parent() {
            if let Ok(directory) = std::fs::File::open(parent) {
                let _ = directory.sync_all();
            }
        }
    }
}

impl Drop for PreparedPermissionStateFile {
    fn drop(&mut self) {
        // Only the unique temporary file created by this transaction is owned.
        let _ = std::fs::remove_file(&self.temporary);
    }
}

fn prepare_permission_state_file(
    path: &Path,
    file: &PermissionStateFile,
) -> anyhow::Result<PreparedPermissionStateFile> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create permission state directory")?;
    }
    let payload =
        serde_json::to_vec_pretty(file).context("failed to serialize permission state file")?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("permissions");
    let tmp = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .context("failed to create temporary permission state file")?;
    let staged = PreparedPermissionStateFile {
        path: path.to_path_buf(),
        temporary: tmp,
    };
    let result: anyhow::Result<()> = (|| {
        output
            .write_all(&payload)
            .context("failed to write temporary permission state file")?;
        output
            .sync_all()
            .context("failed to sync temporary permission state file")?;
        Ok(())
    })();
    drop(output);
    result?;
    Ok(staged)
}
