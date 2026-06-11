use super::polyglot_fixture_repo;
use crate::{
    edges_by_relation, repo_context_bundle, repo_file, repo_search, repo_symbol, GraphRelation,
    JsonRepoIndexStore, ManifestIndex, RepoContextBundleOptions, SymbolKind,
};

#[test]
fn fixture_manifest_indexes_polyglot_sources_and_skips_generated_files() {
    let repo = polyglot_fixture_repo();
    let manifest = ManifestIndex::scan(repo.path()).unwrap();
    let paths: Vec<_> = manifest.files().map(|entry| entry.path.as_str()).collect();

    for expected in [
        ".gitignore",
        "Cargo.toml",
        "README.md",
        "package.json",
        "service/auth.py",
        "src/lib.rs",
        "src/login.rs",
        "tests/login_test.rs",
        "web/src/LoginPanel.tsx",
        "web/src/api.ts",
    ] {
        assert!(paths.contains(&expected), "missing {expected}");
    }

    for excluded in [
        "coverage/report.txt",
        "dist/bundle.js",
        "generated/client.ts",
        "target/debug/build.log",
        "web/src/LoginPanel.snap",
    ] {
        assert!(
            !paths.contains(&excluded),
            "indexed generated file {excluded}"
        );
    }
}

#[test]
fn fixture_index_and_queries_capture_multilanguage_repo_facts() {
    let repo = polyglot_fixture_repo();
    let snapshot = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    assert!(repo_file(&snapshot, "src/login.rs").is_some());
    assert!(repo_file(&snapshot, "web/src/LoginPanel.tsx").is_some());
    assert!(repo_file(&snapshot, "service/auth.py").is_some());
    assert!(repo_file(&snapshot, "generated/client.ts").is_none());

    assert!(
        repo_symbol(&snapshot, "LoginService", Some(SymbolKind::Struct), 10)
            .iter()
            .any(|result| result.file_path == "src/login.rs")
    );
    assert!(
        repo_symbol(&snapshot, "LoginPanel", Some(SymbolKind::Function), 10)
            .iter()
            .any(|result| result.file_path == "web/src/LoginPanel.tsx")
    );
    assert!(
        repo_symbol(&snapshot, "AuthService", Some(SymbolKind::Class), 10)
            .iter()
            .any(|result| result.file_path == "service/auth.py")
    );

    let imports = edges_by_relation(&snapshot, GraphRelation::Imports);
    assert!(imports
        .iter()
        .any(|edge| edge.source == "src/login.rs" && edge.target == "crate::config::AppConfig"));
    assert!(imports
        .iter()
        .any(|edge| edge.source == "web/src/LoginPanel.tsx" && edge.target == "react"));
    assert!(imports
        .iter()
        .any(|edge| edge.source == "service/auth.py" && edge.target == "pathlib"));

    let web_login = repo_search(&snapshot, "login", 20, Some("web"));
    assert!(!web_login.is_empty());
    assert!(web_login
        .iter()
        .all(|result| result.file_path.starts_with("web/")));
    assert!(repo_search(&snapshot, "react", 10, None)
        .iter()
        .any(|result| result.file_path == "package.json"
            || result.file_path == "web/src/LoginPanel.tsx"));
}

#[test]
fn fixture_context_bundle_prioritizes_changed_files_symbols_and_tests() {
    let repo = polyglot_fixture_repo();
    let snapshot = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    let bundle = repo_context_bundle(
        &snapshot,
        "adjust login auth panel",
        RepoContextBundleOptions {
            budget_chars: 6_000,
            required_files: vec![String::from("web/src/LoginPanel.tsx")],
            changed_files: vec![String::from("src/login.rs")],
            result_limit: 8,
            ..RepoContextBundleOptions::default()
        },
    );

    assert!(bundle.estimated_chars <= bundle.budget_chars);
    assert!(bundle
        .suggested_first_reads
        .iter()
        .any(|path| path == "web/src/LoginPanel.tsx"));
    assert!(bundle
        .suggested_first_reads
        .iter()
        .any(|path| path == "src/login.rs"));
    assert!(bundle
        .relevant_symbols
        .iter()
        .any(|result| result.label == "LoginPanel"));
    assert!(bundle
        .relevant_symbols
        .iter()
        .any(|result| result.label == "AuthService"));
    assert!(bundle
        .graph_edges
        .iter()
        .any(|edge| edge.source == "src/login.rs" && edge.target == "LoginService"));
    assert!(bundle
        .test_targets
        .iter()
        .any(|path| path == "tests/login_test.rs"));
    assert!(!bundle
        .suggested_first_reads
        .iter()
        .any(|path| path.contains("generated") || path.contains("target/")));
}
