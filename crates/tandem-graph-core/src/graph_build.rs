use crate::Freshness;
use crate::{
    stable_graph_hash, EdgeId, EdgeKind, GraphEdge, GraphNode, GraphPayload, GraphScope, NodeId,
    NodeKind, PolicyDecision, Provenance, StableGraphHashError, Visibility,
};

pub(crate) fn graph_node(
    scope: &GraphScope,
    kind: NodeKind,
    key: &str,
    label: String,
    payload: GraphPayload,
    freshness: Freshness,
    visibility: Visibility,
    provenance: Provenance,
) -> GraphNode {
    GraphNode {
        id: node_id(scope, kind.clone(), key),
        kind,
        label,
        payload,
        provenance,
        freshness,
        visibility,
        policy: PolicyDecision::Allowed,
    }
}

pub(crate) fn graph_edge(
    scope: &GraphScope,
    kind: EdgeKind,
    source: &NodeId,
    target: &NodeId,
    payload: GraphPayload,
    freshness: Freshness,
    visibility: Visibility,
    provenance: Provenance,
) -> Result<GraphEdge, StableGraphHashError> {
    let fact_hash = stable_graph_hash(&(kind.stable_id(), &source.key, &target.key, &payload))?;
    Ok(GraphEdge {
        id: EdgeId::new(
            scope.clone(),
            kind.stable_id(),
            &source.key,
            &target.key,
            fact_hash,
        ),
        kind,
        source: source.clone(),
        target: target.clone(),
        payload,
        provenance,
        freshness,
        visibility,
        policy: PolicyDecision::Allowed,
    })
}

pub(crate) fn node_id(scope: &GraphScope, kind: NodeKind, key: &str) -> NodeId {
    NodeId::new(scope.clone(), kind.stable_id(), key)
}

pub(crate) fn payload(items: impl IntoIterator<Item = (&'static str, String)>) -> GraphPayload {
    items
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

pub(crate) fn insert_optional(payload: &mut GraphPayload, key: &'static str, value: Option<&str>) {
    if let Some(value) = value {
        payload.insert(key.to_string(), value.to_string());
    }
}
