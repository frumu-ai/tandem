use super::*;

#[tokio::test]
async fn repo_tools_index_and_query_structured_metadata() {
    let workspace = tempfile::tempdir().expect("workspace");
    std::fs::create_dir_all(workspace.path().join("src")).expect("src dir");
    std::fs::write(
        workspace.path().join("src/lib.rs"),
        "pub fn indexed_repo() {}\n",
    )
    .expect("write source");
    let args = json!({"__workspace_root": workspace.path(), "repo_path": "."});

    let index = RepoIndexTool.execute(args.clone()).await.expect("index");
    assert_eq!(index.metadata["structured"]["files"], json!(1));
    assert_eq!(
        index.metadata["structured"]["metrics"]["files_indexed"],
        json!(1)
    );
    assert!(index.metadata["structured"]["debug_export_path"]
        .as_str()
        .is_some_and(|path| path.ends_with(".tandem/repo-graph.json")));

    let search = RepoSearchTool
        .execute(json!({
            "__workspace_root": workspace.path(),
            "repo_path": ".",
            "query": "indexed"
        }))
        .await
        .expect("search");
    assert_eq!(search.metadata["structured"]["count"], json!(1));
    assert_eq!(search.metadata["index_source"], json!("stored"));
}

#[tokio::test]
async fn repo_context_bundle_tool_scopes_symbols() {
    let workspace = tempfile::tempdir().expect("workspace");
    std::fs::create_dir_all(workspace.path().join("packages/app/src")).expect("app dir");
    std::fs::create_dir_all(workspace.path().join("packages/admin/src")).expect("admin dir");
    std::fs::write(
        workspace.path().join("packages/app/src/login.rs"),
        "pub fn login_flow() {}\n",
    )
    .expect("write app");
    std::fs::write(
        workspace.path().join("packages/admin/src/login.rs"),
        "pub fn login_flow() {}\n",
    )
    .expect("write admin");
    let base = json!({"__workspace_root": workspace.path(), "repo_path": "."});
    RepoIndexTool.execute(base).await.expect("index");

    let result = RepoContextBundleTool
        .execute(json!({
            "__workspace_root": workspace.path(),
            "repo_path": ".",
            "task": "login flow",
            "path_scope": "packages/app",
            "limit": 10
        }))
        .await
        .expect("bundle");
    let symbols = result.metadata["structured"]["relevant_symbols"]
        .as_array()
        .expect("symbols");
    assert!(result.metadata["metrics"]["likely_files"]
        .as_u64()
        .is_some_and(|count| count >= 1));
    assert_eq!(result.metadata["metrics"]["relevant_symbols"], json!(1));
    assert!(symbols
        .iter()
        .any(|item| item["file_path"] == "packages/app/src/login.rs"));
    assert!(!symbols
        .iter()
        .any(|item| item["file_path"] == "packages/admin/src/login.rs"));
}
