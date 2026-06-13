use super::write;
use crate::{
    merge_hybrid_candidates, repo_chunks, repo_context_bundle, repo_search, repo_symbol,
    JsonRepoIndexStore, RepoContextBundleOptions, RepoHybridCandidate,
};
use tempfile::TempDir;

#[test]
fn exact_and_normalized_symbol_queries_return_expected_file_in_top_three() {
    let repo = TempDir::new().unwrap();
    write(
        repo.path()
            .join("crates/tandem-repo-intelligence/src/governed.rs"),
        "pub fn repo_context_bundle_governed() {}\n",
    );
    let snapshot = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    for query in [
        "repo_context_bundle_governed",
        "repo-context-bundle-governed",
        "repo.contextBundleGoverned",
        "repoContextBundleGoverned",
    ] {
        assert_top_three_contains(
            repo_symbol(&snapshot, query, None, 3),
            "crates/tandem-repo-intelligence/src/governed.rs",
            query,
        );
        assert_top_three_contains(
            repo_search(&snapshot, query, 3, None),
            "crates/tandem-repo-intelligence/src/governed.rs",
            query,
        );
    }
}

#[test]
fn retrieval_results_include_compact_trace_evidence() {
    let repo = repo_intelligence_fixture();
    let snapshot = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    let results = repo_search(&snapshot, "where is repo.index implemented?", 5, None);
    let result = results
        .iter()
        .find(|result| result.file_path == "crates/tandem-tools/src/repo_intelligence_tools.rs")
        .expect("repo.index implementation result");
    let trace = result.trace.first().expect("retrieval trace");

    assert_eq!(trace.retriever, "symbol");
    assert!(trace.matched_term.contains("repo"));
    assert!(trace
        .edge_path
        .iter()
        .any(|path| path == "crates/tandem-tools/src/repo_intelligence_tools.rs"));
    assert!(trace.ranking.contains("match_tier="));
}

#[test]
fn context_bundle_returns_chunk_spans_without_raw_content() {
    let repo = TempDir::new().unwrap();
    write(
        repo.path().join("src/lib.rs"),
        "pub fn repo_context_bundle() {}\npub fn unrelated() {}\n",
    );
    write(
        repo.path().join("docs/repo.md"),
        "# Repo Context Bundle\n\nSensitive body text should not be embedded in graph metadata.\n",
    );
    let snapshot = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    let chunks = repo_chunks(&snapshot);
    assert!(chunks
        .iter()
        .any(|chunk| chunk.kind == "source_symbol" && chunk.label == "repo_context_bundle"));
    assert!(chunks
        .iter()
        .any(|chunk| chunk.kind == "doc_section" && chunk.label == "Repo Context Bundle"));

    let bundle = repo_context_bundle(
        &snapshot,
        "repo context bundle",
        RepoContextBundleOptions {
            budget_chars: 6_000,
            result_limit: 5,
            ..RepoContextBundleOptions::default()
        },
    );

    assert!(bundle
        .first_read_spans
        .iter()
        .any(|span| span.file_path == "src/lib.rs"
            && span.kind == "source_symbol"
            && span.start_line == 1));
    let serialized = serde_json::to_string(&bundle).expect("bundle json");
    assert!(!serialized.contains("Sensitive body text"));
}

#[test]
fn hybrid_candidates_merge_vector_and_graph_scores_deterministically() {
    let graph = candidate("chunk-a", "src/lib.rs", 70, "graph", "symbol match");
    let vector = candidate("chunk-a", "src/lib.rs", 20, "vector", "embedding match");
    let other = candidate(
        "chunk-b",
        "docs/guide.md",
        80,
        "vector",
        "semantic neighbor",
    );

    let merged = merge_hybrid_candidates(vec![graph], vec![vector, other], 10);

    assert_eq!(merged[0].chunk_id, "chunk-a");
    assert_eq!(merged[0].score, 90);
    assert_eq!(merged[0].retrievers, vec!["graph", "vector"]);
    assert_eq!(merged[1].chunk_id, "chunk-b");
}

#[test]
fn golden_retrieval_queries_find_agent_relevant_files() {
    let repo = repo_intelligence_fixture();
    let snapshot = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    let cases = [
        RetrievalCase {
            query: "where is repo.index implemented?",
            expected_file: "crates/tandem-tools/src/repo_intelligence_tools.rs",
        },
        RetrievalCase {
            query: "how does ACA call context_bundle?",
            expected_file: "crates/tandem-tools/src/repo_intelligence_tools.rs",
        },
        RetrievalCase {
            query: "what tests cover repo tools?",
            expected_file: "crates/tandem-tools/src/repo_intelligence_tools_tests.rs",
        },
    ];

    for case in cases {
        assert_top_three_contains(
            repo_search(&snapshot, case.query, 5, None),
            case.expected_file,
            case.query,
        );
        let bundle = repo_context_bundle(
            &snapshot,
            case.query,
            RepoContextBundleOptions {
                budget_chars: 6_000,
                result_limit: 5,
                ..RepoContextBundleOptions::default()
            },
        );
        assert!(
            bundle
                .suggested_first_reads
                .iter()
                .take(5)
                .any(|path| path == case.expected_file),
            "context bundle missed {} for query {query}; first reads: {:?}",
            case.expected_file,
            bundle.suggested_first_reads,
            query = case.query
        );
    }
}

#[test]
fn context_bundle_prioritizes_repo_intelligence_sources_over_meta_docs() {
    let repo = repo_intelligence_fixture();
    let snapshot = JsonRepoIndexStore::new(repo.path().join(".tandem/repo-index.json"))
        .index_repo(repo.path())
        .unwrap();

    let bundle = repo_context_bundle(
        &snapshot,
        "trace repo.context_bundle implementation in engine tools and crates",
        RepoContextBundleOptions {
            budget_chars: 6_000,
            result_limit: 6,
            ..RepoContextBundleOptions::default()
        },
    );
    let first_reads = bundle.suggested_first_reads;
    let tool_position = position(
        &first_reads,
        "crates/tandem-tools/src/repo_intelligence_tools.rs",
    )
    .expect("missing tandem-tools implementation");
    let core_position = position(
        &first_reads,
        "crates/tandem-repo-intelligence/src/context/bundle.rs",
    )
    .expect("missing repo-intelligence implementation");
    let docs_position = position(&first_reads, ".agents/repo-intelligence-template.md");

    assert!(
        docs_position.is_none_or(|position| tool_position < position && core_position < position),
        "meta docs outranked implementation files: {first_reads:?}"
    );
}

struct RetrievalCase {
    query: &'static str,
    expected_file: &'static str,
}

fn repo_intelligence_fixture() -> TempDir {
    let repo = TempDir::new().unwrap();
    write(
        repo.path()
            .join("crates/tandem-tools/src/repo_intelligence_tools.rs"),
        r#"
pub struct RepoIndexTool;
pub struct RepoContextBundleTool;
impl RepoIndexTool {
    pub async fn execute_repo_index() {}
}
impl RepoContextBundleTool {
    pub async fn execute_context_bundle() {}
}
"#,
    );
    write(
        repo.path()
            .join("crates/tandem-tools/src/repo_intelligence_tools_tests.rs"),
        r#"
#[tokio::test]
async fn repo_tools_index_and_query_structured_metadata() {}
#[tokio::test]
async fn repo_context_bundle_tool_scopes_symbols() {}
"#,
    );
    write(
        repo.path()
            .join("crates/tandem-repo-intelligence/src/context/bundle.rs"),
        "pub fn repo_context_bundle() {}\n",
    );
    write(
        repo.path()
            .join("crates/tandem-repo-intelligence/src/query.rs"),
        "pub fn repo_search() {}\npub fn repo_symbol() {}\n",
    );
    write(
        repo.path().join(".agents/repo-intelligence-template.md"),
        "# Repo Intelligence Template\n\nACA planning notes explain repo.index and repo.context_bundle.\n",
    );
    write(
        repo.path().join("README.md"),
        "# Tandem\n\nRepo intelligence docs describe tools, tests, and context bundles.\n",
    );
    repo
}

fn assert_top_three_contains(
    results: Vec<crate::RepoSearchResult>,
    expected_file: &str,
    query: &str,
) {
    assert!(
        results
            .iter()
            .take(3)
            .any(|result| result.file_path == expected_file),
        "expected {expected_file} in top three for query {query}; got {results:?}"
    );
}

fn position(paths: &[String], expected: &str) -> Option<usize> {
    paths.iter().position(|path| path == expected)
}

fn candidate(
    chunk_id: &str,
    file_path: &str,
    score: u16,
    retriever: &str,
    provenance: &str,
) -> RepoHybridCandidate {
    RepoHybridCandidate {
        chunk_id: chunk_id.to_string(),
        file_path: file_path.to_string(),
        score,
        retrievers: vec![retriever.to_string()],
        provenance: provenance.to_string(),
    }
}
