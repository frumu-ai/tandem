use super::*;
use tandem_repo_intelligence::{
    extract_repo_facts, scan_repo, GraphRelation, JsonRepoIndexStore, RepoIndexSnapshot, SymbolKind,
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
            "doc_headings": snapshot.facts.doc_headings.len()
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
