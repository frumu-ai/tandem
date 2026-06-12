use super::*;
use tandem_repo_intelligence::{
    extract_repo_facts, graph_scope_for_repo, repo_index_metrics, repo_intelligence_event,
    scan_repo, GraphQueryEnvelope, GraphRelation, JsonRepoIndexStore, RepoIndexSnapshot,
    SymbolKind,
};

pub(crate) fn repo_path_schema() -> Value {
    json!({"type":"object","properties":{"repo_path":{"type":"string"}}})
}

pub(crate) fn load_snapshot_for_query(
    args: &Value,
) -> anyhow::Result<Option<(PathBuf, RepoIndexSnapshot, String)>> {
    let Some(repo_root) = repo_root_from_args(args) else {
        return Ok(None);
    };
    let store = JsonRepoIndexStore::new(store_path(&repo_root));
    match store.load() {
        Ok(snapshot) => Ok(Some((repo_root, snapshot, "stored".to_string()))),
        Err(load_error) => {
            let manifest = scan_repo(&repo_root)?;
            let facts = extract_repo_facts(&repo_root, &manifest)?;
            let snapshot = RepoIndexSnapshot {
                root_label: repo_root.to_string_lossy().to_string(),
                indexed_unix_ms: 0,
                manifest,
                facts,
            };
            Ok(Some((
                repo_root,
                snapshot,
                format!("ephemeral_scan_after_load_error:{load_error}"),
            )))
        }
    }
}

pub(crate) fn snapshot_result(
    tool: &str,
    repo_root: &Path,
    source: &str,
    snapshot: RepoIndexSnapshot,
) -> ToolResult {
    let store = JsonRepoIndexStore::new(store_path(repo_root));
    let metrics = repo_index_metrics(&snapshot);
    json_result(
        tool,
        repo_root,
        source,
        json!({
            "indexed_unix_ms": snapshot.indexed_unix_ms,
            "files": snapshot.manifest.len(),
            "symbols": snapshot.facts.symbols.len(),
            "imports": snapshot.facts.imports.len(),
            "config_references": snapshot.facts.config_references.len(),
            "doc_headings": snapshot.facts.doc_headings.len(),
            "metrics": metrics.clone(),
            "debug_export_path": store.debug_export_path().to_string_lossy(),
            "event": repo_intelligence_event(
                format!("{tool}.completed"),
                repo_root.to_string_lossy(),
                Some(metrics),
                None,
            )
        }),
    )
}

pub(crate) fn json_result(
    tool: &str,
    repo_root: &Path,
    source: &str,
    payload: Value,
) -> ToolResult {
    ToolResult {
        output: serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
        metadata: json!({
            "tool": tool,
            "repo_root": repo_root.to_string_lossy(),
            "store_path": store_path(repo_root).to_string_lossy(),
            "index_source": source,
            "structured": payload
        }),
    }
}

pub(crate) fn repo_root_from_args(args: &Value) -> Option<PathBuf> {
    let root = repo_path_arg(args);
    resolve_walk_root(root, args)
}

pub(crate) fn repo_path_arg(args: &Value) -> &str {
    string_arg(args, "repo_path").unwrap_or(".")
}

pub(crate) fn store_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".tandem/repo-index.json")
}

pub(crate) fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn graph_query_envelope(
    args: &Value,
    snapshot: &RepoIndexSnapshot,
    tool: &str,
    extra_allowed_tools: &[&str],
) -> GraphQueryEnvelope {
    let actor_id = string_arg(args, "actor_id").unwrap_or("local-agent");
    let mut envelope =
        GraphQueryEnvelope::new(graph_scope_for_repo(&snapshot.root_label), actor_id);
    envelope.automation_id = string_arg(args, "automation_id").map(str::to_string);
    envelope.run_id = string_arg(args, "run_id").map(str::to_string);
    envelope.context_assertion = string_arg(args, "context_assertion").map(str::to_string);
    envelope.budget_tokens = args.get("budget_tokens").and_then(Value::as_u64);
    envelope.readable_paths = string_array(args.get("readable_paths"));
    if envelope.readable_paths.is_empty() {
        envelope.readable_paths = string_arg(args, "path_scope")
            .map(|scope| vec![scope.to_string()])
            .unwrap_or_default();
    }
    envelope.writable_paths = string_array(args.get("writable_paths"));
    envelope.allowed_memory_tiers = string_array(args.get("allowed_memory_tiers"));
    envelope.approvals = string_array(args.get("approvals"));
    envelope.allowed_tools = string_array(args.get("allowed_tools"));
    if envelope.allowed_tools.is_empty() && args.get("allowed_tools").is_none() {
        envelope.allowed_tools = std::iter::once(tool.to_string())
            .chain(extra_allowed_tools.iter().map(|tool| tool.to_string()))
            .collect();
    }
    envelope
}

pub(crate) fn limit_arg(args: &Value, default: usize, max: usize) -> usize {
    args.get("limit")
        .or_else(|| args.get("depth"))
        .and_then(Value::as_u64)
        .map(|value| (value as usize).clamp(1, max))
        .unwrap_or(default)
}

pub(crate) fn parse_relation(value: &str) -> Option<GraphRelation> {
    match value.trim().to_ascii_lowercase().as_str() {
        "defines" | "define" => Some(GraphRelation::Defines),
        "imports" | "import" => Some(GraphRelation::Imports),
        "configures" | "config" => Some(GraphRelation::Configures),
        "documents" | "docs" | "doc" => Some(GraphRelation::Documents),
        _ => None,
    }
}

pub(crate) fn parse_symbol_kind(value: &str) -> Option<SymbolKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "function" | "fn" => Some(SymbolKind::Function),
        "struct" => Some(SymbolKind::Struct),
        "enum" => Some(SymbolKind::Enum),
        "trait" => Some(SymbolKind::Trait),
        "impl" => Some(SymbolKind::Impl),
        "module" | "mod" => Some(SymbolKind::Module),
        "class" => Some(SymbolKind::Class),
        "interface" => Some(SymbolKind::Interface),
        "type" | "typealias" | "type_alias" => Some(SymbolKind::TypeAlias),
        "const" | "constant" => Some(SymbolKind::Constant),
        _ => None,
    }
}

pub(crate) fn in_scope(path: &str, path_scope: Option<&str>) -> bool {
    let Some(scope) = path_scope else {
        return true;
    };
    let scope = scope.trim_matches('/');
    if scope.is_empty() {
        return true;
    }
    path == scope
        || path
            .strip_prefix(scope)
            .is_some_and(|rest| rest.starts_with('/'))
}
