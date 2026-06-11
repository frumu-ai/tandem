use crate::{FileChangeKind, ManifestIndex, RepoScanOptions};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn scan_respects_gitignore_exclusions_and_size_limits() {
    let repo = TempDir::new().unwrap();
    fs::create_dir(repo.path().join(".git")).unwrap();
    write(repo.path().join(".gitignore"), "ignored.txt\n");
    write(repo.path().join("src/lib.rs"), "pub fn visible() {}\n");
    write(repo.path().join("ignored.txt"), "ignored\n");
    write(repo.path().join("target/debug.o"), "artifact\n");
    write(
        repo.path().join("large.md"),
        "0123456789abcdef0123456789abcdef\n",
    );

    let options = RepoScanOptions {
        max_file_size_bytes: 24,
        ..RepoScanOptions::default()
    };
    let manifest = ManifestIndex::scan_with_options(repo.path(), &options).unwrap();
    let paths: Vec<_> = manifest.files().map(|entry| entry.path.as_str()).collect();

    assert_eq!(paths, vec![".gitignore", "src/lib.rs"]);
}

#[test]
fn update_from_scan_tracks_added_modified_deleted_and_unchanged_files() {
    let repo = TempDir::new().unwrap();
    write(repo.path().join("src/lib.rs"), "pub fn first() {}\n");
    write(repo.path().join("README.md"), "hello\n");

    let options = RepoScanOptions::default();
    let mut manifest = ManifestIndex::scan_with_options(repo.path(), &options).unwrap();

    write(repo.path().join("src/lib.rs"), "pub fn second() {}\n");
    fs::remove_file(repo.path().join("README.md")).unwrap();
    write(
        repo.path().join("tests/smoke.rs"),
        "#[test]\nfn smoke() {}\n",
    );

    let delta = manifest.update_from_scan(repo.path(), &options).unwrap();

    assert_eq!(
        delta.change_kind_for_path("src/lib.rs"),
        Some(FileChangeKind::Modified)
    );
    assert_eq!(
        delta.change_kind_for_path("README.md"),
        Some(FileChangeKind::Deleted)
    );
    assert_eq!(
        delta.change_kind_for_path("tests/smoke.rs"),
        Some(FileChangeKind::Added)
    );
    assert_eq!(manifest.len(), 2);
}

#[test]
fn update_from_scan_reports_unchanged_files() {
    let repo = TempDir::new().unwrap();
    write(repo.path().join("src/lib.rs"), "pub fn stable() {}\n");

    let options = RepoScanOptions::default();
    let mut manifest = ManifestIndex::scan_with_options(repo.path(), &options).unwrap();
    let delta = manifest.update_from_scan(repo.path(), &options).unwrap();

    assert_eq!(
        delta.change_kind_for_path("src/lib.rs"),
        Some(FileChangeKind::Unchanged)
    );
    assert_eq!(delta.stats(manifest.len()).unchanged_files, 1);
}

fn write(path: impl AsRef<Path>, body: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}
