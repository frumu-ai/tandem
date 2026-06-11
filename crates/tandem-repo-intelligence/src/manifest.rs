use crate::error::Result;
use crate::model::{FileChangeKind, FileManifestEntry, IndexStats, RepoScanOptions};
use crate::scanner::scan_repo_with_options;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestIndex {
    files: BTreeMap<String, FileManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestDelta {
    pub root: PathBuf,
    pub added: Vec<FileManifestEntry>,
    pub modified: Vec<FileManifestEntry>,
    pub deleted: Vec<String>,
    pub unchanged: Vec<String>,
}

impl ManifestIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_entries(entries: Vec<FileManifestEntry>) -> Self {
        Self {
            files: entries
                .into_iter()
                .map(|entry| (entry.path.clone(), entry))
                .collect(),
        }
    }

    pub fn files(&self) -> impl Iterator<Item = &FileManifestEntry> {
        self.files.values()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn scan(root: impl AsRef<Path>) -> Result<Self> {
        Self::scan_with_options(root, &RepoScanOptions::default())
    }

    pub fn scan_with_options(root: impl AsRef<Path>, options: &RepoScanOptions) -> Result<Self> {
        Ok(Self::from_entries(scan_repo_with_options(root, options)?))
    }

    pub fn update_from_scan(
        &mut self,
        root: impl AsRef<Path>,
        options: &RepoScanOptions,
    ) -> Result<ManifestDelta> {
        let root = root.as_ref();
        let next = ManifestIndex::scan_with_options(root, options)?;
        let delta = self.diff(root, &next);
        *self = next;
        Ok(delta)
    }

    pub fn diff(&self, root: impl AsRef<Path>, next: &ManifestIndex) -> ManifestDelta {
        let old_paths: BTreeSet<_> = self.files.keys().cloned().collect();
        let next_paths: BTreeSet<_> = next.files.keys().cloned().collect();

        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut unchanged = Vec::new();
        for path in next_paths.iter() {
            match (self.files.get(path), next.files.get(path)) {
                (None, Some(entry)) => added.push(entry.clone()),
                (Some(old), Some(entry)) if old.sha256 != entry.sha256 => {
                    modified.push(entry.clone());
                }
                (Some(_), Some(_)) => unchanged.push(path.clone()),
                _ => {}
            }
        }

        let deleted = old_paths.difference(&next_paths).cloned().collect();
        ManifestDelta {
            root: root.as_ref().to_path_buf(),
            added,
            modified,
            deleted,
            unchanged,
        }
    }
}

impl ManifestDelta {
    pub fn change_kind_for_path(&self, path: &str) -> Option<FileChangeKind> {
        if self.added.iter().any(|entry| entry.path == path) {
            return Some(FileChangeKind::Added);
        }
        if self.modified.iter().any(|entry| entry.path == path) {
            return Some(FileChangeKind::Modified);
        }
        if self.deleted.iter().any(|entry| entry == path) {
            return Some(FileChangeKind::Deleted);
        }
        if self.unchanged.iter().any(|entry| entry == path) {
            return Some(FileChangeKind::Unchanged);
        }
        None
    }

    pub fn stats(&self, indexed_files: usize) -> IndexStats {
        IndexStats {
            root: self.root.clone(),
            total_seen: indexed_files,
            indexed_files,
            skipped_files: 0,
            added_files: self.added.len(),
            modified_files: self.modified.len(),
            unchanged_files: self.unchanged.len(),
            deleted_files: self.deleted.len(),
        }
    }
}
