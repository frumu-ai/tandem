use super::*;
use crate::repo_intelligence_tool_support::*;
use tandem_repo_intelligence::{
    repo_context_bundle_governed, repo_context_bundle_metrics, repo_impact_governed,
    repo_neighbors_governed, repo_search_governed, repo_symbol_governed, JsonRepoIndexStore,
    RepoContextBundleOptions,
};

pub(crate) struct RepoIndexTool;
pub(crate) struct RepoUpdateChangedFilesTool;
pub(crate) struct RepoSearchTool;
pub(crate) struct RepoSymbolTool;
pub(crate) struct RepoNeighborsTool;
pub(crate) struct RepoImpactTool;
pub(crate) struct RepoContextBundleTool;
pub(crate) struct RepoTestTargetsTool;

#[async_trait]
impl Tool for RepoIndexTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "repo.index",
            "Build and persist the deterministic repo intelligence index for the workspace repository.",
            repo_path_schema(),
            workspace_write_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some(repo_root) = repo_root_from_args(&args) else {
            return Ok(sandbox_path_denied_result(repo_path_arg(&args), &args));
        };
        let store = JsonRepoIndexStore::new(store_path(&repo_root));
        let snapshot = store.index_repo(&repo_root)?;
        Ok(snapshot_result(
            "repo.index",
            &repo_root,
            "stored",
            snapshot,
        ))
    }
}

#[async_trait]
impl Tool for RepoUpdateChangedFilesTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "repo.update_changed_files",
            "Refresh the repo intelligence index after changed files. The current MVP performs a safe full refresh.",
            json!({
                "type":"object",
                "properties":{
                    "repo_path":{"type":"string"},
                    "changed_files":{"type":"array","items":{"type":"string"}}
                }
            }),
            workspace_write_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some(repo_root) = repo_root_from_args(&args) else {
            return Ok(sandbox_path_denied_result(repo_path_arg(&args), &args));
        };
        let changed_files = string_array(args.get("changed_files"));
        let store = JsonRepoIndexStore::new(store_path(&repo_root));
        let snapshot = store.index_repo(&repo_root)?;
        let mut result =
            snapshot_result("repo.update_changed_files", &repo_root, "stored", snapshot);
        result.metadata["changed_files"] = json!(changed_files);
        result.metadata["refresh_mode"] = json!("full_rescan");
        Ok(result)
    }
}

#[async_trait]
impl Tool for RepoSearchTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "repo.search",
            "Search indexed repo files, symbols, imports, config, and docs before broad grep/glob discovery.",
            json!({
                "type":"object",
                "properties":{
                    "repo_path":{"type":"string"},
                    "query":{"type":"string"},
                    "limit":{"type":"integer"},
                    "path_scope":{"type":"string"}
                },
                "required":["query"]
            }),
            workspace_search_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some((repo_root, snapshot, source)) = load_snapshot_for_query(&args)? else {
            return Ok(sandbox_path_denied_result(repo_path_arg(&args), &args));
        };
        let query = args["query"].as_str().unwrap_or("").trim();
        let limit = limit_arg(&args, 20, 100);
        let envelope = graph_query_envelope(&args, &snapshot, "repo.search", &[]);
        let governed = repo_search_governed(
            &envelope,
            &snapshot,
            query,
            limit,
            string_arg(&args, "path_scope"),
        );
        let mut result = json_result(
            "repo.search",
            &repo_root,
            &source,
            json!({"query": query, "count": governed.value.len(), "results": governed.value}),
        );
        result.metadata["graph_query"] = json!(governed.audit);
        Ok(result)
    }
}

#[async_trait]
impl Tool for RepoSymbolTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "repo.symbol",
            "Find indexed repo symbols by name and optional kind.",
            json!({
                "type":"object",
                "properties":{
                    "repo_path":{"type":"string"},
                    "query":{"type":"string"},
                    "kind":{"type":"string"},
                    "limit":{"type":"integer"},
                    "path_scope":{"type":"string"}
                },
                "required":["query"]
            }),
            workspace_search_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some((repo_root, snapshot, source)) = load_snapshot_for_query(&args)? else {
            return Ok(sandbox_path_denied_result(repo_path_arg(&args), &args));
        };
        let query = args["query"].as_str().unwrap_or("").trim();
        let kind = string_arg(&args, "kind").and_then(parse_symbol_kind);
        let path_scope = string_arg(&args, "path_scope");
        let envelope = graph_query_envelope(&args, &snapshot, "repo.symbol", &[]);
        let mut governed =
            repo_symbol_governed(&envelope, &snapshot, query, kind, limit_arg(&args, 20, 100));
        governed.value = governed
            .value
            .into_iter()
            .filter(|result| in_scope(&result.file_path, path_scope))
            .collect::<Vec<_>>();
        let mut result = json_result(
            "repo.symbol",
            &repo_root,
            &source,
            json!({"query": query, "count": governed.value.len(), "results": governed.value}),
        );
        result.metadata["graph_query"] = json!(governed.audit);
        Ok(result)
    }
}

#[async_trait]
impl Tool for RepoNeighborsTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "repo.neighbors",
            "Traverse indexed repo graph neighbors from a file, symbol, or graph node.",
            json!({
                "type":"object",
                "properties":{
                    "repo_path":{"type":"string"},
                    "node_or_path":{"type":"string"},
                    "relation":{"type":"string"},
                    "depth":{"type":"integer"}
                },
                "required":["node_or_path"]
            }),
            workspace_search_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some((repo_root, snapshot, source)) = load_snapshot_for_query(&args)? else {
            return Ok(sandbox_path_denied_result(repo_path_arg(&args), &args));
        };
        let node = args["node_or_path"].as_str().unwrap_or("").trim();
        let relation = string_arg(&args, "relation").and_then(parse_relation);
        let envelope = graph_query_envelope(&args, &snapshot, "repo.neighbors", &[]);
        let governed =
            repo_neighbors_governed(&envelope, &snapshot, node, relation, limit_arg(&args, 1, 4));
        let mut result = json_result(
            "repo.neighbors",
            &repo_root,
            &source,
            json!({"node_or_path": node, "count": governed.value.len(), "neighbors": governed.value}),
        );
        result.metadata["graph_query"] = json!(governed.audit);
        Ok(result)
    }
}

#[async_trait]
impl Tool for RepoImpactTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "repo.impact",
            "Summarize changed-file impact using indexed graph edges and likely test targets.",
            json!({
                "type":"object",
                "properties":{
                    "repo_path":{"type":"string"},
                    "changed_files":{"type":"array","items":{"type":"string"}}
                },
                "required":["changed_files"]
            }),
            workspace_search_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some((repo_root, snapshot, source)) = load_snapshot_for_query(&args)? else {
            return Ok(sandbox_path_denied_result(repo_path_arg(&args), &args));
        };
        let envelope = graph_query_envelope(&args, &snapshot, "repo.impact", &[]);
        let governed = repo_impact_governed(
            &envelope,
            &snapshot,
            &string_array(args.get("changed_files")),
        );
        let mut result = json_result("repo.impact", &repo_root, &source, json!(governed.value));
        result.metadata["graph_query"] = json!(governed.audit);
        Ok(result)
    }
}

#[async_trait]
impl Tool for RepoContextBundleTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "repo.context_bundle",
            "Build a deterministic, budgeted context bundle for an autonomous coding task.",
            json!({
                "type":"object",
                "properties":{
                    "repo_path":{"type":"string"},
                    "task":{"type":"string"},
                    "budget_chars":{"type":"integer"},
                    "required_files":{"type":"array","items":{"type":"string"}},
                    "changed_files":{"type":"array","items":{"type":"string"}},
                    "path_scope":{"type":"string"},
                    "limit":{"type":"integer"}
                },
                "required":["task"]
            }),
            workspace_search_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some((repo_root, snapshot, source)) = load_snapshot_for_query(&args)? else {
            return Ok(sandbox_path_denied_result(repo_path_arg(&args), &args));
        };
        let task = args["task"].as_str().unwrap_or("").trim();
        let envelope = graph_query_envelope(&args, &snapshot, "repo.context_bundle", &[]);
        let governed = repo_context_bundle_governed(
            &envelope,
            &snapshot,
            task,
            RepoContextBundleOptions {
                budget_chars: args["budget_chars"].as_u64().unwrap_or(6_000) as usize,
                required_files: string_array(args.get("required_files")),
                changed_files: string_array(args.get("changed_files")),
                path_scope: string_arg(&args, "path_scope").map(str::to_string),
                result_limit: limit_arg(&args, 12, 50),
            },
        );
        let mut result = json_result(
            "repo.context_bundle",
            &repo_root,
            &source,
            json!(governed.value),
        );
        result.metadata["metrics"] = json!(repo_context_bundle_metrics(&governed.value));
        result.metadata["graph_query"] = json!(governed.audit);
        Ok(result)
    }
}

#[async_trait]
impl Tool for RepoTestTargetsTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "repo.test_targets",
            "Return likely test targets for changed files using repo impact analysis.",
            json!({
                "type":"object",
                "properties":{
                    "repo_path":{"type":"string"},
                    "changed_files":{"type":"array","items":{"type":"string"}}
                },
                "required":["changed_files"]
            }),
            workspace_search_capabilities(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some((repo_root, snapshot, source)) = load_snapshot_for_query(&args)? else {
            return Ok(sandbox_path_denied_result(repo_path_arg(&args), &args));
        };
        let envelope =
            graph_query_envelope(&args, &snapshot, "repo.test_targets", &["repo.impact"]);
        let governed = repo_impact_governed(
            &envelope,
            &snapshot,
            &string_array(args.get("changed_files")),
        );
        let targets = governed
            .value
            .likely_test_targets
            .iter()
            .map(|item| item.file_path.clone())
            .collect::<Vec<_>>();
        let mut result = json_result(
            "repo.test_targets",
            &repo_root,
            &source,
            json!({"count": targets.len(), "test_targets": targets, "impact": governed.value}),
        );
        result.metadata["graph_query"] = json!(governed.audit);
        Ok(result)
    }
}
