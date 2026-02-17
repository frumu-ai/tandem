use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const STORAGE_LAYOUT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedPaths {
    pub canonical_root: PathBuf,
    pub legacy_root: PathBuf,
    pub engine_state_dir: PathBuf,
    pub config_path: PathBuf,
    pub keystore_path: PathBuf,
    pub vault_key_path: PathBuf,
    pub memory_db_path: PathBuf,
    pub sidecar_release_cache_path: PathBuf,
    pub logs_dir: PathBuf,
    pub storage_version_path: PathBuf,
    pub migration_report_path: PathBuf,
}

pub fn normalize_workspace_path(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let as_path = PathBuf::from(trimmed);
    let absolute = if as_path.is_absolute() {
        as_path
    } else {
        std::env::current_dir().ok()?.join(as_path)
    };
    let normalized = if absolute.exists() {
        absolute.canonicalize().ok()?
    } else {
        absolute
    };
    Some(normalized.to_string_lossy().to_string())
}

pub fn is_within_workspace_root(path: &Path, workspace_root: &Path) -> bool {
    let candidate = if path.exists() {
        path.canonicalize().ok()
    } else if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        std::env::current_dir().ok().map(|cwd| cwd.join(path))
    };
    let Some(candidate) = candidate else {
        return false;
    };
    let root = if workspace_root.exists() {
        workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf())
    } else {
        workspace_root.to_path_buf()
    };
    let candidate = normalize_for_workspace_compare(candidate);
    let root = normalize_for_workspace_compare(root);
    candidate.starts_with(root)
}

fn normalize_for_workspace_compare(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        // Canonicalized Windows paths often use the verbatim prefix (\\?\),
        // while runtime paths may not. Strip that prefix so equivalent paths
        // compare consistently for workspace sandbox checks.
        let mut text = path.to_string_lossy().replace('/', "\\");
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            text = format!(r"\\{}", rest);
        } else if let Some(rest) = text.strip_prefix(r"\\?\") {
            text = rest.to_string();
        }
        PathBuf::from(text.to_ascii_lowercase())
    }

    #[cfg(not(windows))]
    {
        path
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    pub performed: bool,
    pub reason: String,
    pub copied: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StorageVersionMarker {
    version: u32,
    timestamp_ms: u64,
}

pub fn resolve_shared_paths() -> anyhow::Result<SharedPaths> {
    let base = dirs::data_dir().ok_or_else(|| anyhow::anyhow!("Failed to resolve data dir"))?;
    let canonical_root = base.join("tandem");
    let legacy_root = base.join("ai.frumu.tandem");

    Ok(SharedPaths {
        canonical_root: canonical_root.clone(),
        legacy_root,
        engine_state_dir: canonical_root.join("data"),
        config_path: canonical_root.join("config.json"),
        keystore_path: canonical_root.join("tandem.keystore"),
        vault_key_path: canonical_root.join("vault.key"),
        memory_db_path: canonical_root.join("memory.sqlite"),
        sidecar_release_cache_path: canonical_root.join("sidecar_release_cache.json"),
        logs_dir: canonical_root.join("logs"),
        storage_version_path: canonical_root.join("storage_version.json"),
        migration_report_path: canonical_root.join("migration_report.json"),
    })
}

pub fn migrate_legacy_storage_if_needed(paths: &SharedPaths) -> anyhow::Result<MigrationResult> {
    fs::create_dir_all(&paths.canonical_root)
        .with_context(|| format!("Failed to create {:?}", paths.canonical_root))?;
    let mut result = MigrationResult {
        performed: false,
        reason: String::new(),
        copied: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
        timestamp_ms: now_ms(),
    };

    let canonical_empty = is_dir_effectively_empty(&paths.canonical_root)?;
    let mut source_found = false;

    let file_artifacts = [
        "vault.key",
        "tandem.keystore",
        "memory.sqlite",
        "memory.sqlite-shm",
        "memory.sqlite-wal",
        "config.json",
        "sidecar_release_cache.json",
    ];
    let dir_artifacts = ["data", "state", "storage", "binaries", "logs"];

    if paths.legacy_root.exists() {
        source_found = true;
        for name in file_artifacts {
            let src = paths.legacy_root.join(name);
            if !src.exists() {
                continue;
            }
            let dst = paths.canonical_root.join(name);
            match copy_file_guarded(&src, &dst) {
                Ok(true) => result.copied.push(name.to_string()),
                Ok(false) => result.skipped.push(name.to_string()),
                Err(err) => result.errors.push(format!("{}: {}", name, err)),
            }
        }

        for name in dir_artifacts {
            let src = paths.legacy_root.join(name);
            if !src.is_dir() {
                continue;
            }
            let dst = paths.canonical_root.join(name);
            match copy_dir_guarded(&src, &dst) {
                Ok((copied, skipped)) => {
                    for entry in copied {
                        result.copied.push(format!("{}/{}", name, entry));
                    }
                    for entry in skipped {
                        result.skipped.push(format!("{}/{}", name, entry));
                    }
                }
                Err(err) => result.errors.push(format!("{}: {}", name, err)),
            }
        }
    }

    if let Some(opencode_root) = resolve_opencode_legacy_root() {
        let src_storage = opencode_root.join("storage");
        if src_storage.is_dir() {
            source_found = true;
            let dst_storage = paths.engine_state_dir.join("storage");
            match copy_dir_guarded(&src_storage, &dst_storage) {
                Ok((copied, skipped)) => {
                    for entry in copied {
                        result.copied.push(format!("opencode/storage/{}", entry));
                    }
                    for entry in skipped {
                        result.skipped.push(format!("opencode/storage/{}", entry));
                    }
                }
                Err(err) => result.errors.push(format!("opencode/storage: {}", err)),
            }
        }
    }

    result.performed = !result.copied.is_empty();
    result.reason = if !source_found {
        "legacy_not_found".to_string()
    } else if result.performed && canonical_empty {
        "migration_copied_into_empty_canonical".to_string()
    } else if result.performed {
        "migration_backfilled_missing_artifacts".to_string()
    } else if !result.errors.is_empty() {
        "migration_partial_error".to_string()
    } else {
        "migration_no_changes".to_string()
    };

    persist_storage_marker(paths)?;
    persist_migration_report(paths, &result)?;
    Ok(result)
}

fn persist_storage_marker(paths: &SharedPaths) -> anyhow::Result<()> {
    let marker = StorageVersionMarker {
        version: STORAGE_LAYOUT_VERSION,
        timestamp_ms: now_ms(),
    };
    write_json(&paths.storage_version_path, &marker)
}

fn persist_migration_report(paths: &SharedPaths, report: &MigrationResult) -> anyhow::Result<()> {
    write_json(&paths.migration_report_path, report)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{}\n", text))?;
    Ok(())
}

fn is_dir_effectively_empty(path: &Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

fn copy_file_guarded(src: &Path, dst: &Path) -> anyhow::Result<bool> {
    if dst.exists() && should_skip_copy(src, dst)? {
        return Ok(false);
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst).with_context(|| format!("copy {:?} -> {:?}", src, dst))?;
    Ok(true)
}

fn copy_dir_guarded(src: &Path, dst: &Path) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let mut copied = Vec::new();
    let mut skipped = Vec::new();
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            let (child_copied, child_skipped) = copy_dir_guarded(&src_path, &dst_path)?;
            copied.extend(child_copied);
            skipped.extend(child_skipped);
        } else {
            let rel = src_path
                .strip_prefix(src)
                .unwrap_or(src_path.as_path())
                .to_string_lossy()
                .to_string();
            if copy_file_guarded(&src_path, &dst_path)? {
                copied.push(rel);
            } else {
                skipped.push(rel);
            }
        }
    }
    Ok((copied, skipped))
}

fn should_skip_copy(src: &Path, dst: &Path) -> anyhow::Result<bool> {
    let src_meta = fs::metadata(src)?;
    let dst_meta = fs::metadata(dst)?;

    if src_meta.len() != dst_meta.len() {
        return Ok(false);
    }

    let src_time = src_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let dst_time = dst_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    Ok(dst_time >= src_time)
}

fn resolve_opencode_legacy_root() -> Option<PathBuf> {
    if let Ok(override_dir) = std::env::var("TANDEM_OPENCODE_LEGACY_DIR") {
        let path = PathBuf::from(override_dir);
        if path.exists() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local").join("share").join("opencode"));
    }
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(local.join("opencode"));
    }
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("opencode"));
    }
    candidates.into_iter().find(|path| path.exists())
}

fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(dur) => dur.as_millis() as u64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn workspace_root_compare_handles_verbatim_prefix_mismatch() {
        let workspace = PathBuf::from(r"\\?\C:\Users\evang\work\tandem-engine\tandem");
        let candidate = PathBuf::from(r"C:\Users\evang\work\tandem-engine\tandem\*");
        assert!(is_within_workspace_root(&candidate, &workspace));

        let workspace_plain = PathBuf::from(r"C:\Users\evang\work\tandem-engine\tandem");
        let candidate_verbatim = PathBuf::from(r"\\?\C:\Users\evang\work\tandem-engine\tandem\src");
        assert!(is_within_workspace_root(
            &candidate_verbatim,
            &workspace_plain
        ));
    }

    #[test]
    fn migration_copies_from_legacy_when_canonical_empty() {
        let temp = tempfile::tempdir().expect("tempdir");
        let legacy = temp.path().join("legacy");
        let canonical = temp.path().join("canonical");
        fs::create_dir_all(&legacy).expect("legacy");
        fs::write(legacy.join("vault.key"), "abc").expect("write");
        fs::write(legacy.join("memory.sqlite"), "db").expect("write");

        let paths = SharedPaths {
            canonical_root: canonical.clone(),
            legacy_root: legacy.clone(),
            engine_state_dir: canonical.join("data"),
            config_path: canonical.join("config.json"),
            keystore_path: canonical.join("tandem.keystore"),
            vault_key_path: canonical.join("vault.key"),
            memory_db_path: canonical.join("memory.sqlite"),
            sidecar_release_cache_path: canonical.join("sidecar_release_cache.json"),
            logs_dir: canonical.join("logs"),
            storage_version_path: canonical.join("storage_version.json"),
            migration_report_path: canonical.join("migration_report.json"),
        };

        let report = migrate_legacy_storage_if_needed(&paths).expect("migrate");
        assert!(
            report.reason == "migration_copied_into_empty_canonical"
                || report.reason == "migration_partial_error"
        );
        assert!(paths.vault_key_path.exists());
        assert!(paths.memory_db_path.exists());
        assert!(paths.storage_version_path.exists());
    }

    #[test]
    fn migration_backfills_keys_when_canonical_already_has_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let legacy = temp.path().join("legacy");
        let canonical = temp.path().join("canonical");
        fs::create_dir_all(&legacy).expect("legacy");
        fs::create_dir_all(canonical.join("logs")).expect("logs");
        fs::write(legacy.join("vault.key"), "abc").expect("write");
        fs::write(legacy.join("tandem.keystore"), "secret").expect("write");

        let paths = SharedPaths {
            canonical_root: canonical.clone(),
            legacy_root: legacy.clone(),
            engine_state_dir: canonical.join("data"),
            config_path: canonical.join("config.json"),
            keystore_path: canonical.join("tandem.keystore"),
            vault_key_path: canonical.join("vault.key"),
            memory_db_path: canonical.join("memory.sqlite"),
            sidecar_release_cache_path: canonical.join("sidecar_release_cache.json"),
            logs_dir: canonical.join("logs"),
            storage_version_path: canonical.join("storage_version.json"),
            migration_report_path: canonical.join("migration_report.json"),
        };

        let report = migrate_legacy_storage_if_needed(&paths).expect("migrate");
        assert_eq!(report.reason, "migration_backfilled_missing_artifacts");
        assert!(paths.vault_key_path.exists());
        assert!(paths.keystore_path.exists());
    }

    #[test]
    fn migration_copies_opencode_storage_into_engine_state_storage() {
        let temp = tempfile::tempdir().expect("tempdir");
        let opencode_root = temp.path().join("opencode");
        let src_storage = opencode_root.join("storage").join("session").join("global");
        fs::create_dir_all(&src_storage).expect("opencode storage");
        fs::write(src_storage.join("ses_abc.json"), r#"{"id":"ses_abc"}"#).expect("write");

        let legacy = temp.path().join("legacy-missing");
        let canonical = temp.path().join("canonical");
        fs::create_dir_all(&canonical).expect("canonical");

        std::env::set_var(
            "TANDEM_OPENCODE_LEGACY_DIR",
            opencode_root.to_string_lossy().to_string(),
        );
        let paths = SharedPaths {
            canonical_root: canonical.clone(),
            legacy_root: legacy,
            engine_state_dir: canonical.join("data"),
            config_path: canonical.join("config.json"),
            keystore_path: canonical.join("tandem.keystore"),
            vault_key_path: canonical.join("vault.key"),
            memory_db_path: canonical.join("memory.sqlite"),
            sidecar_release_cache_path: canonical.join("sidecar_release_cache.json"),
            logs_dir: canonical.join("logs"),
            storage_version_path: canonical.join("storage_version.json"),
            migration_report_path: canonical.join("migration_report.json"),
        };

        let report = migrate_legacy_storage_if_needed(&paths).expect("migrate");
        assert!(report.performed);
        assert!(paths
            .engine_state_dir
            .join("storage")
            .join("session")
            .join("global")
            .join("ses_abc.json")
            .exists());
        std::env::remove_var("TANDEM_OPENCODE_LEGACY_DIR");
    }
}
