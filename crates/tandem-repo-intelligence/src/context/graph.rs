use crate::model::{
    Confidence, GraphEdge, GraphRelation, RepoGraphNeighbor, RepoImpactItem, RepoImpactSummary,
    RepoIndexSnapshot,
};
use crate::repo_file;
use std::collections::{BTreeSet, VecDeque};

pub fn repo_neighbors(
    snapshot: &RepoIndexSnapshot,
    node_or_path: &str,
    relation_filter: Option<GraphRelation>,
    depth: usize,
) -> Vec<RepoGraphNeighbor> {
    if depth == 0 || node_or_path.trim().is_empty() {
        return Vec::new();
    }

    let edges = snapshot.graph_edges();
    let mut visited = BTreeSet::from([node_or_path.to_string()]);
    let mut queue = VecDeque::from([(node_or_path.to_string(), 0usize)]);
    let mut neighbors = Vec::new();

    while let Some((node, current_depth)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }
        for edge in &edges {
            if relation_filter
                .as_ref()
                .is_some_and(|relation| relation != &edge.relation)
            {
                continue;
            }
            let Some(next) = adjacent_node(edge, &node) else {
                continue;
            };
            let next_depth = current_depth + 1;
            neighbors.push(RepoGraphNeighbor {
                node: next.clone(),
                edge: edge.clone(),
                depth: next_depth,
                reason: format!(
                    "{} is connected to {} by {:?} at line {}",
                    node, next, edge.relation, edge.line
                ),
            });
            if visited.insert(next.clone()) {
                queue.push_back((next, next_depth));
            }
        }
    }

    neighbors.sort_by(|left, right| {
        left.depth
            .cmp(&right.depth)
            .then(left.node.cmp(&right.node))
            .then(left.edge.source.cmp(&right.edge.source))
            .then(left.edge.target.cmp(&right.edge.target))
    });
    neighbors
}

pub fn repo_impact(snapshot: &RepoIndexSnapshot, changed_files: &[String]) -> RepoImpactSummary {
    let changed = normalized_files(changed_files);
    let changed_symbols = symbols_for_files(snapshot, &changed);
    let mut directly_affected = Vec::new();
    let mut import_neighbors = Vec::new();
    let mut config_or_docs = Vec::new();
    let mut likely_test_targets = Vec::new();

    for file in &changed {
        if repo_file(snapshot, file).is_some() {
            directly_affected.push(item(
                file,
                1,
                GraphRelation::Defines,
                "changed file is present in the repo index",
                Confidence::Extracted,
            ));
        }
    }

    for edge in snapshot.graph_edges() {
        if changed.contains(&edge.source) {
            match edge.relation {
                GraphRelation::Defines => directly_affected.push(item(
                    &edge.source,
                    edge.line,
                    edge.relation,
                    "changed file defines this graph node",
                    edge.confidence,
                )),
                GraphRelation::Configures | GraphRelation::Documents => config_or_docs.push(item(
                    &edge.source,
                    edge.line,
                    edge.relation,
                    "changed file carries config or documentation context",
                    edge.confidence,
                )),
                GraphRelation::Imports => import_neighbors.push(item(
                    &edge.source,
                    edge.line,
                    edge.relation,
                    "changed file imports this graph target",
                    edge.confidence,
                )),
            }
        } else if edge.relation == GraphRelation::Imports
            && imports_changed_symbol(&edge, &changed_symbols)
        {
            let target = if is_likely_test_file(&edge.source) {
                &mut likely_test_targets
            } else {
                &mut import_neighbors
            };
            target.push(item(
                &edge.source,
                edge.line,
                edge.relation,
                "file imports a symbol or module from a changed file",
                edge.confidence,
            ));
        }
    }

    for file in &snapshot.manifest {
        if !changed.contains(&file.path) && is_likely_test_for_changed(&file.path, &changed) {
            likely_test_targets.push(item(
                &file.path,
                1,
                GraphRelation::Documents,
                "test file path matches a changed source path",
                Confidence::Inferred,
            ));
        }
    }

    RepoImpactSummary {
        changed_files: changed.into_iter().collect(),
        directly_affected: dedupe_items(directly_affected),
        import_neighbors: dedupe_items(import_neighbors),
        config_or_docs: dedupe_items(config_or_docs),
        likely_test_targets: dedupe_items(likely_test_targets),
    }
}

fn adjacent_node(edge: &GraphEdge, node: &str) -> Option<String> {
    if edge.source == node {
        Some(edge.target.clone())
    } else if edge.target == node {
        Some(edge.source.clone())
    } else {
        None
    }
}

fn normalized_files(files: &[String]) -> BTreeSet<String> {
    files
        .iter()
        .map(|file| file.trim_matches('/').to_string())
        .filter(|file| !file.is_empty())
        .collect()
}

fn symbols_for_files(snapshot: &RepoIndexSnapshot, files: &BTreeSet<String>) -> BTreeSet<String> {
    snapshot
        .facts
        .symbols
        .iter()
        .filter(|symbol| files.contains(&symbol.file_path))
        .flat_map(|symbol| {
            [
                symbol.name.clone(),
                module_stem(&symbol.file_path),
                symbol.file_path.clone(),
            ]
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn imports_changed_symbol(edge: &GraphEdge, changed_symbols: &BTreeSet<String>) -> bool {
    changed_symbols
        .iter()
        .any(|symbol| edge.target == *symbol || edge.target.contains(symbol))
}

fn is_likely_test_for_changed(path: &str, changed: &BTreeSet<String>) -> bool {
    is_likely_test_file(path)
        && changed
            .iter()
            .map(|file| module_stem(file))
            .any(|stem| !stem.is_empty() && path.contains(&stem))
}

fn is_likely_test_file(path: &str) -> bool {
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.contains("_test.")
        || path.contains(".test.")
        || path.contains("_spec.")
        || path.contains(".spec.")
}

fn module_stem(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .split('.')
        .next()
        .unwrap_or("")
        .to_string()
}

fn item(
    file_path: &str,
    line: usize,
    relation: GraphRelation,
    reason: &str,
    confidence: Confidence,
) -> RepoImpactItem {
    RepoImpactItem {
        file_path: file_path.to_string(),
        line,
        relation,
        reason: reason.to_string(),
        confidence,
    }
}

fn dedupe_items(items: Vec<RepoImpactItem>) -> Vec<RepoImpactItem> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in items {
        let key = (item.file_path.clone(), item.line, item.relation.clone());
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then(left.line.cmp(&right.line))
            .then(left.relation.cmp(&right.relation))
    });
    deduped
}
