use crate::Freshness;
use crate::{
    stable_graph_hash, EdgeId, EdgeKind, GraphEdge, GraphNode, GraphPayload, GraphScope, NodeId,
    NodeKind, PolicyDecision, Provenance, StableGraphHashError, Visibility,
};

#[derive(Debug, Clone)]
pub(crate) struct GraphBuildContext {
    scope: GraphScope,
    freshness: Freshness,
    visibility: Visibility,
    provenance: Provenance,
}

impl GraphBuildContext {
    pub(crate) fn new(
        scope: &GraphScope,
        freshness: Freshness,
        visibility: Visibility,
        provenance: Provenance,
    ) -> Self {
        Self {
            scope: scope.clone(),
            freshness,
            visibility,
            provenance,
        }
    }

    pub(crate) fn with_scope(&self, scope: &GraphScope) -> Self {
        Self {
            scope: scope.clone(),
            freshness: self.freshness.clone(),
            visibility: self.visibility.clone(),
            provenance: self.provenance.clone(),
        }
    }

    pub(crate) fn node(
        &self,
        kind: NodeKind,
        key: &str,
        label: String,
        payload: GraphPayload,
    ) -> GraphNode {
        GraphNode {
            id: node_id(&self.scope, kind.clone(), key),
            kind,
            label,
            payload,
            provenance: self.provenance.clone(),
            freshness: self.freshness.clone(),
            visibility: self.visibility.clone(),
            policy: PolicyDecision::Allowed,
        }
    }

    pub(crate) fn edge(
        &self,
        kind: EdgeKind,
        source: &NodeId,
        target: &NodeId,
        payload: GraphPayload,
    ) -> Result<GraphEdge, StableGraphHashError> {
        let fact_hash = stable_graph_hash(&(kind.stable_id(), &source.key, &target.key, &payload))?;
        Ok(GraphEdge {
            id: EdgeId::new(
                self.scope.clone(),
                kind.stable_id(),
                &source.key,
                &target.key,
                fact_hash,
            ),
            kind,
            source: source.clone(),
            target: target.clone(),
            payload,
            provenance: self.provenance.clone(),
            freshness: self.freshness.clone(),
            visibility: self.visibility.clone(),
            policy: PolicyDecision::Allowed,
        })
    }
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
