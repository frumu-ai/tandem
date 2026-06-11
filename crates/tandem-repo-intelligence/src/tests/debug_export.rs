use crate::tests::repo_with_handler_fixture;
use crate::{
    repo_context_bundle, repo_context_bundle_metrics, repo_debug_export, repo_index_metrics,
    JsonRepoIndexStore, RepoContextBundleOptions,
};

#[test]
fn repo_index_metrics_and_debug_export_summarize_snapshot() {
    let repo = repo_with_handler_fixture();
    let store = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"));
    let snapshot = store.index_repo(repo.path()).unwrap();

    let metrics = repo_index_metrics(&snapshot);
    assert_eq!(metrics.files_indexed, snapshot.manifest.len());
    assert!(metrics.total_bytes > 0);
    assert!(metrics.symbols > 0);
    assert!(metrics.imports > 0);
    assert!(metrics.graph_edges > 0);
    assert!(metrics
        .top_file_sources
        .iter()
        .any(|path| path == "src/handler.rs"));

    let export = repo_debug_export(&snapshot);
    assert_eq!(export.root_label, repo.path().to_string_lossy());
    assert_eq!(export.metrics, metrics);
    assert_eq!(export.graph_edges.len(), metrics.graph_edges);
    assert!(store.debug_export_path().exists());
}

#[test]
fn root_level_store_does_not_overwrite_root_repo_graph_file() {
    let repo = repo_with_handler_fixture();
    let root_graph = repo.path().join("repo-graph.json");
    std::fs::write(&root_graph, "checked in graph docs").unwrap();
    let store = JsonRepoIndexStore::new(repo.path().join("repo-index.json"));

    store.index_repo(repo.path()).unwrap();

    assert_eq!(
        std::fs::read_to_string(&root_graph).unwrap(),
        "checked in graph docs"
    );
    assert_eq!(
        store.debug_export_path(),
        repo.path().join(".tandem/repo-graph.json")
    );
    assert!(store.debug_export_path().exists());
}

#[test]
fn repo_context_bundle_metrics_report_bundle_shape() {
    let repo = repo_with_handler_fixture();
    let snapshot = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"))
        .index_repo(repo.path())
        .unwrap();
    let bundle = repo_context_bundle(
        &snapshot,
        "Update handler login tests",
        RepoContextBundleOptions {
            changed_files: vec!["src/handler.rs".to_string()],
            ..RepoContextBundleOptions::default()
        },
    );

    let metrics = repo_context_bundle_metrics(&bundle);
    assert_eq!(metrics.task, "Update handler login tests");
    assert_eq!(metrics.estimated_chars, bundle.estimated_chars);
    assert_eq!(metrics.budget_chars, bundle.budget_chars);
    assert_eq!(metrics.likely_files, bundle.likely_files.len());
    assert_eq!(metrics.top_sources, bundle.suggested_first_reads);
}
