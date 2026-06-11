use crate::error::{RepoIntelligenceError, Result};
use crate::model::{FileManifestEntry, RepoScanOptions};
use ignore::{DirEntry, WalkBuilder};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub fn scan_repo(root: impl AsRef<Path>) -> Result<Vec<FileManifestEntry>> {
    scan_repo_with_options(root, &RepoScanOptions::default())
}

pub fn scan_repo_with_options(
    root: impl AsRef<Path>,
    options: &RepoScanOptions,
) -> Result<Vec<FileManifestEntry>> {
    let root = root.as_ref();
    validate_root(root)?;

    let excluded_dirs = normalized_set(&options.excluded_dirs);
    let excluded_extensions = normalized_set(&options.excluded_extensions);
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .ignore(true)
        .filter_entry(move |entry| include_entry(entry, &excluded_dirs))
        .build();

    let mut entries = Vec::new();
    for item in walker {
        let entry = item.map_err(|error| RepoIntelligenceError::Walk(error.to_string()))?;
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }

        let path = entry.path();
        if has_excluded_extension(path, &excluded_extensions) {
            continue;
        }

        let metadata =
            std::fs::metadata(path).map_err(|source| RepoIntelligenceError::Metadata {
                path: path.to_path_buf(),
                source,
            })?;
        if metadata.len() > options.max_file_size_bytes {
            continue;
        }

        entries.push(manifest_entry(root, path, metadata)?);
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn validate_root(root: &Path) -> Result<()> {
    if !root.exists() {
        return Err(RepoIntelligenceError::RootMissing(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(RepoIntelligenceError::RootNotDirectory(root.to_path_buf()));
    }
    Ok(())
}

fn include_entry(entry: &DirEntry, excluded_dirs: &HashSet<String>) -> bool {
    if entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir())
    {
        if let Some(name) = entry.file_name().to_str() {
            return !excluded_dirs.contains(&name.to_lowercase());
        }
    }
    true
}

fn normalized_set(values: &[String]) -> HashSet<String> {
    values.iter().map(|value| value.to_lowercase()).collect()
}

fn has_excluded_extension(path: &Path, excluded_extensions: &HashSet<String>) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| excluded_extensions.contains(&extension.to_lowercase()))
        .unwrap_or(false)
}

fn manifest_entry(
    root: &Path,
    path: &Path,
    metadata: std::fs::Metadata,
) -> Result<FileManifestEntry> {
    let relative_path = normalize_relative_path(root, path);
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    Ok(FileManifestEntry {
        path: relative_path,
        size_bytes: metadata.len(),
        modified_unix_ms,
        sha256: sha256_file(path)?,
    })
}

fn normalize_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|source| RepoIntelligenceError::ReadFile {
        path: PathBuf::from(path),
        source,
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
