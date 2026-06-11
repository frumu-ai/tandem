use crate::{
    edges_by_relation, extract_file_facts, extract_repo_facts, repo_file, repo_search, repo_symbol,
    FileChangeKind, GraphRelation, JsonRepoIndexStore, ManifestIndex, RepoScanOptions, SymbolKind,
};
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

#[test]
fn extract_file_facts_finds_rust_symbols_and_imports() {
    let facts = extract_file_facts(
        "src/lib.rs",
        r#"
use std::path::Path;
pub struct Runner;
enum Mode { Fast }
trait Work {}
impl Runner {}
pub fn run() {}
"#,
    );

    assert!(facts
        .imports
        .iter()
        .any(|edge| edge.target == "std::path::Path"));
    assert!(facts
        .symbols
        .iter()
        .any(|symbol| symbol.name == "Runner" && symbol.kind == SymbolKind::Struct));
    assert!(facts
        .symbols
        .iter()
        .any(|symbol| symbol.name == "run" && symbol.kind == SymbolKind::Function));
}

#[test]
fn extract_file_facts_finds_typescript_and_python_symbols() {
    let ts = extract_file_facts(
        "src/App.tsx",
        r#"
import React from "react";
export interface Props {}
export type Mode = "fast";
export function App() { return null; }
const localValue = 1;
"#,
    );
    let py = extract_file_facts(
        "service/app.py",
        r#"
import os
from pathlib import Path
class Service:
    pass
async def run_service():
    pass
"#,
    );

    assert!(ts.imports.iter().any(|edge| edge.target == "react"));
    assert!(ts
        .symbols
        .iter()
        .any(|symbol| symbol.name == "App" && symbol.kind == SymbolKind::Function));
    assert!(py.imports.iter().any(|edge| edge.target == "pathlib"));
    assert!(py
        .symbols
        .iter()
        .any(|symbol| symbol.name == "Service" && symbol.kind == SymbolKind::Class));
    assert!(py.symbols.iter().any(|symbol| symbol.name == "run_service"));
}

#[test]
fn extract_file_facts_finds_config_references_and_doc_headings() {
    let cargo = extract_file_facts(
        "Cargo.toml",
        r#"
[package]
name = "demo"
[dependencies]
serde = "1"
"#,
    );
    let docs = extract_file_facts(
        "README.md",
        r#"
# Demo

This repo demonstrates extraction.

## Usage
Run the tests.
"#,
    );

    assert!(cargo
        .config_references
        .iter()
        .any(|reference| reference.key == "dependencies.serde" && reference.value == "1"));
    assert!(docs
        .doc_headings
        .iter()
        .any(|heading| heading.title == "Demo"
            && heading.excerpt == "This repo demonstrates extraction."));
}

#[test]
fn extract_repo_facts_reads_manifest_files() {
    let repo = TempDir::new().unwrap();
    write(repo.path().join("src/lib.rs"), "pub fn indexed() {}\n");
    write(
        repo.path().join("README.md"),
        "# Indexed\n\nFrom manifest.\n",
    );

    let manifest = ManifestIndex::scan(repo.path()).unwrap();
    let files: Vec<_> = manifest.files().cloned().collect();
    let facts = extract_repo_facts(repo.path(), &files).unwrap();

    assert!(facts.symbols.iter().any(|symbol| symbol.name == "indexed"));
    assert!(facts
        .doc_headings
        .iter()
        .any(|heading| heading.title == "Indexed"));
}

#[test]
fn extract_repo_facts_skips_non_utf8_manifest_files() {
    let repo = TempDir::new().unwrap();
    write(repo.path().join("src/lib.rs"), "pub fn indexed() {}\n");
    fs::write(repo.path().join("asset"), [0xff, 0xfe, 0xfd]).unwrap();

    let manifest = ManifestIndex::scan(repo.path()).unwrap();
    let files: Vec<_> = manifest.files().cloned().collect();
    let facts = extract_repo_facts(repo.path(), &files).unwrap();

    assert!(files.iter().any(|entry| entry.path == "asset"));
    assert!(facts.symbols.iter().any(|symbol| symbol.name == "indexed"));
}

#[test]
fn json_repo_index_store_persists_across_reload() {
    let repo = TempDir::new().unwrap();
    let store_path = repo.path().join(".tandem/repo-index.json");
    write(
        repo.path().join("src/lib.rs"),
        "use std::fs;\npub fn indexed() {}\n",
    );
    write(
        repo.path().join("README.md"),
        "# Indexed\n\nSearchable docs.\n",
    );

    let store = JsonRepoIndexStore::new(&store_path);
    let snapshot = store.index_repo(repo.path()).unwrap();
    let reloaded = JsonRepoIndexStore::new(&store_path).load().unwrap();

    assert_eq!(snapshot.manifest, reloaded.manifest);
    assert!(reloaded.indexed_unix_ms > 0);
    assert!(repo_file(&reloaded, "src/lib.rs").is_some());
    assert!(
        repo_symbol(&reloaded, "indexed", Some(SymbolKind::Function), 10)
            .iter()
            .any(|result| result.file_path == "src/lib.rs")
    );
}

#[test]
fn repo_search_is_stable_and_honors_path_scope() {
    let repo = TempDir::new().unwrap();
    let store_path = repo.path().join("repo-index.json");
    write(repo.path().join("src/lib.rs"), "pub fn indexed() {}\n");
    write(
        repo.path().join("docs/guide.md"),
        "# Indexed\n\nSearchable docs.\n",
    );

    let snapshot = JsonRepoIndexStore::new(store_path)
        .index_repo(repo.path())
        .unwrap();
    let all = repo_search(&snapshot, "indexed", 10, None);
    let docs = repo_search(&snapshot, "indexed", 10, Some("docs"));

    assert!(all.iter().any(|result| result.file_path == "src/lib.rs"));
    assert!(all.iter().any(|result| result.file_path == "docs/guide.md"));
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].file_path, "docs/guide.md");
}

#[test]
fn graph_edges_can_be_filtered_by_relation() {
    let repo = TempDir::new().unwrap();
    write(
        repo.path().join("src/lib.rs"),
        "use std::path::Path;\npub fn indexed() {}\n",
    );
    let snapshot = JsonRepoIndexStore::new(repo.path().join("repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    let imports = edges_by_relation(&snapshot, GraphRelation::Imports);
    let definitions = edges_by_relation(&snapshot, GraphRelation::Defines);

    assert!(imports.iter().any(|edge| edge.target == "std::path::Path"));
    assert!(definitions.iter().any(|edge| edge.target == "indexed"));
}

fn write(path: impl AsRef<Path>, body: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}
