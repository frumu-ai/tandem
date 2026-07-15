// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Compatibility policy for the legacy stateful JSON/JSONL sidecars.
//!
//! Before the one-time import completes, the sidecars remain the durable source
//! and must be read and written. Once the orchestration store is authoritative,
//! they are diagnostic/export mirrors only and require an explicit opt-in.

use std::path::Path;

use anyhow::Context;

use super::{OrchestrationStateStore, OrchestrationStorePaths, StatefulRuntimeStoragePaths};

const LEGACY_IMPORT_BACKUP_DIRECTORY: &str = "stateful_legacy_import_backup";

pub(crate) fn should_write_stateful_runtime_sidecar(authoritative_store_active: bool) -> bool {
    !authoritative_store_active
        || crate::config::env::resolve_stateful_runtime_compatibility_mirrors_enabled()
}

pub(crate) async fn retire_stateful_runtime_sidecars(
    paths: &StatefulRuntimeStoragePaths,
    reliability_path: &Path,
) -> anyhow::Result<usize> {
    if crate::config::env::resolve_stateful_runtime_compatibility_mirrors_enabled()
        || !migration_is_complete(&paths.run_events_path)?
    {
        return Ok(0);
    }
    let runtime_root = paths
        .run_events_path
        .parent()
        .context("stateful event sidecar path has no runtime directory")?;
    let backup_root = runtime_root.join(LEGACY_IMPORT_BACKUP_DIRECTORY);
    let mut retired = 0;
    for path in [
        paths.run_events_path.as_path(),
        paths.snapshots_root.as_path(),
        paths.waits_path.as_path(),
        reliability_path,
    ] {
        retired += retire_path(path, &backup_root).await?;
    }
    Ok(retired)
}

fn migration_is_complete(events_path: &Path) -> anyhow::Result<bool> {
    let paths = OrchestrationStorePaths::from_runtime_events_path(events_path);
    if !crate::stateful_runtime::backend::store_initialized_hint(&paths.database_path)? {
        return Ok(false);
    }
    OrchestrationStateStore::open(paths)?.legacy_runtime_migration_complete()
}

async fn retire_path(path: &Path, backup_root: &Path) -> anyhow::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let backup_path = backup_root.join(
        path.file_name()
            .context("stateful sidecar path has no file name")?,
    );
    if backup_path.exists() {
        if path.is_dir() {
            tokio::fs::remove_dir_all(path).await?;
        } else {
            tokio::fs::remove_file(path).await?;
        }
        return Ok(1);
    }
    tokio::fs::create_dir_all(backup_root).await?;
    tokio::fs::rename(path, &backup_path)
        .await
        .with_context(|| {
            format!(
                "failed to archive retired stateful sidecar {}",
                path.display()
            )
        })?;
    Ok(1)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::stateful_runtime::{
        LegacyRuntimeMigrationPaths, OrchestrationStateStore, StatefulRuntimeStoragePaths,
    };

    #[test]
    fn legacy_sidecars_remain_writable_until_authority_is_available() {
        assert!(should_write_stateful_runtime_sidecar(false));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn retention_archives_import_sources_then_discards_later_mirrors() {
        std::env::remove_var("TANDEM_STATEFUL_RUNTIME_COMPATIBILITY_MIRRORS_ENABLED");
        let directory = tempdir().unwrap();
        let runtime_root = directory.path().join("runtime");
        let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(
            &runtime_root.join("events.jsonl"),
        );
        let reliability_path = runtime_root.join("stateful_reliability.json");
        tokio::fs::create_dir_all(&paths.snapshots_root)
            .await
            .unwrap();
        tokio::fs::write(&paths.run_events_path, "").await.unwrap();
        tokio::fs::write(&paths.waits_path, "[]").await.unwrap();
        tokio::fs::write(&reliability_path, "{}").await.unwrap();

        let store =
            OrchestrationStateStore::from_runtime_events_path(&paths.run_events_path).unwrap();
        store
            .import_legacy_runtime_state(
                &LegacyRuntimeMigrationPaths::from_runtime_root(&runtime_root),
                1,
            )
            .unwrap();

        assert_eq!(
            retire_stateful_runtime_sidecars(&paths, &reliability_path)
                .await
                .unwrap(),
            4
        );
        let backup_root = runtime_root.join(LEGACY_IMPORT_BACKUP_DIRECTORY);
        assert!(backup_root.join("stateful_events.jsonl").exists());
        assert!(backup_root.join("stateful_snapshots").exists());
        assert!(backup_root.join("stateful_waits.json").exists());
        assert!(backup_root.join("stateful_reliability.json").exists());

        tokio::fs::write(&paths.run_events_path, "temporary")
            .await
            .unwrap();
        assert_eq!(
            retire_stateful_runtime_sidecars(&paths, &reliability_path)
                .await
                .unwrap(),
            1
        );
        assert!(!paths.run_events_path.exists());
        assert_eq!(
            tokio::fs::read_to_string(backup_root.join("stateful_events.jsonl"))
                .await
                .unwrap(),
            ""
        );
    }
}
