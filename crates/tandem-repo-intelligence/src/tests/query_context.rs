use super::{repo_with_handler_fixture, write};
use crate::{
    edges_by_relation, repo_context_bundle, repo_file, repo_impact, repo_neighbors, repo_search,
    repo_symbol, GraphRelation, JsonRepoIndexStore, RepoContextBundleOptions, SymbolKind,
};
use tempfile::TempDir;

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
fn json_repo_index_store_does_not_index_its_own_snapshot() {
    let repo = TempDir::new().unwrap();
    let store_path = repo.path().join(".tandem/repo-index.json");
    write(repo.path().join("src/lib.rs"), "pub fn indexed() {}\n");

    let store = JsonRepoIndexStore::new(&store_path);
    store.index_repo(repo.path()).unwrap();
    let snapshot = store.index_repo(repo.path()).unwrap();

    assert!(repo_file(&snapshot, ".tandem/repo-index.json").is_none());
    assert!(!repo_search(&snapshot, "root_label", 10, None)
        .iter()
        .any(|result| result.file_path == ".tandem/repo-index.json"));
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
fn repo_search_path_scope_matches_path_components() {
    let repo = TempDir::new().unwrap();
    write(
        repo.path().join("docs/guide.md"),
        "# Indexed\n\nExpected docs.\n",
    );
    write(
        repo.path().join("docs-old/guide.md"),
        "# Indexed\n\nOld docs.\n",
    );
    write(
        repo.path().join("docs2/guide.md"),
        "# Indexed\n\nOther docs.\n",
    );

    let snapshot = JsonRepoIndexStore::new(repo.path().join("repo-index.json"))
        .index_repo(repo.path())
        .unwrap();
    let docs = repo_search(&snapshot, "indexed", 10, Some("docs"));

    assert_eq!(
        docs.iter()
            .map(|result| result.file_path.as_str())
            .collect::<Vec<_>>(),
        vec!["docs/guide.md"]
    );
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

#[test]
fn repo_neighbors_traverses_graph_edges_by_relation() {
    let repo = repo_with_handler_fixture();
    let snapshot = JsonRepoIndexStore::new(repo.path().join("repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    let from_file = repo_neighbors(&snapshot, "src/handler.rs", Some(GraphRelation::Defines), 1);
    let from_symbol = repo_neighbors(&snapshot, "run_login", Some(GraphRelation::Defines), 1);

    assert!(from_file
        .iter()
        .any(|neighbor| neighbor.node == "run_login"));
    assert!(from_symbol
        .iter()
        .any(|neighbor| neighbor.node == "src/handler.rs"));
}

#[test]
fn repo_impact_reports_import_config_and_test_neighbors() {
    let repo = repo_with_handler_fixture();
    let snapshot = JsonRepoIndexStore::new(repo.path().join("repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    let handler_impact = repo_impact(&snapshot, &[String::from("src/handler.rs")]);
    let config_impact = repo_impact(&snapshot, &[String::from("Cargo.toml")]);

    assert!(handler_impact
        .directly_affected
        .iter()
        .any(|item| item.file_path == "src/handler.rs" && item.relation == GraphRelation::Defines));
    assert!(handler_impact
        .likely_test_targets
        .iter()
        .any(|item| item.file_path == "tests/handler_test.rs"));
    assert!(config_impact
        .config_or_docs
        .iter()
        .any(|item| item.file_path == "Cargo.toml"));
}

#[test]
fn repo_context_bundle_is_budgeted_and_prioritizes_agent_reads() {
    let repo = repo_with_handler_fixture();
    let snapshot = JsonRepoIndexStore::new(repo.path().join("repo-index.json"))
        .index_repo(repo.path())
        .unwrap();
    let keyword_hits = repo_search(&snapshot, "handler", 10, None);

    let bundle = repo_context_bundle(
        &snapshot,
        "update login handler behavior",
        RepoContextBundleOptions {
            budget_chars: 450,
            changed_files: vec![String::from("src/handler.rs")],
            result_limit: 2,
            ..RepoContextBundleOptions::default()
        },
    );

    assert!(bundle.estimated_chars <= bundle.budget_chars);
    assert!(keyword_hits.len() > bundle.likely_files.len());
    assert_eq!(bundle.suggested_first_reads[0], "src/handler.rs");
    assert!(bundle
        .test_targets
        .iter()
        .any(|path| path == "tests/handler_test.rs"));
    assert!(!bundle
        .suggested_first_reads
        .iter()
        .any(|path| path == "docs/handler.md"));
}

#[test]
fn repo_context_bundle_scopes_relevant_symbols() {
    let repo = TempDir::new().unwrap();
    write(
        repo.path().join("packages/app/src/login.rs"),
        "pub fn login_flow() {}\n",
    );
    write(
        repo.path().join("packages/admin/src/login.rs"),
        "pub fn login_flow() {}\n",
    );

    let snapshot = JsonRepoIndexStore::new(repo.path().join("repo-index.json"))
        .index_repo(repo.path())
        .unwrap();
    let bundle = repo_context_bundle(
        &snapshot,
        "login flow",
        RepoContextBundleOptions {
            path_scope: Some(String::from("packages/app")),
            result_limit: 10,
            ..RepoContextBundleOptions::default()
        },
    );

    assert!(bundle
        .relevant_symbols
        .iter()
        .any(|result| result.file_path == "packages/app/src/login.rs"));
    assert!(!bundle
        .relevant_symbols
        .iter()
        .any(|result| result.file_path == "packages/admin/src/login.rs"));
}
