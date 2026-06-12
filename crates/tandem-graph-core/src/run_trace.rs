use crate::graph_build::{graph_edge, graph_node, insert_optional, node_id, payload};
use crate::{
    EdgeKind, Freshness, FreshnessSource, GraphAuditEvent, GraphAuditEventType, GraphAuditTarget,
    GraphEdge, GraphNode, GraphPayload, GraphRetentionPolicy, GraphScope, GraphStoragePartition,
    NodeId, NodeKind, Provenance, StableGraphHashError, Visibility,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunTraceGraphSpec {
    pub scope: GraphScope,
    pub run_id: String,
    pub workflow_version_id: Option<String>,
    pub events: Vec<RunTraceEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunTraceEvent {
    pub event_id: String,
    pub kind: RunTraceEventKind,
    pub workflow_step_id: Option<String>,
    pub tool_name: Option<String>,
    pub memory_tier: Option<String>,
    pub policy_scope: Option<String>,
    pub artifact_ref: Option<String>,
    pub safe_summary: Option<String>,
    pub policy_denied: bool,
    pub latency_ms: Option<u64>,
    pub cost_microunits: Option<u64>,
    pub occurred_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RunTraceEventKind {
    #[serde(rename = "model_call")]
    ModelCall,
    #[serde(rename = "tool_call")]
    ToolCall,
    #[serde(rename = "memory_read")]
    MemoryRead,
    #[serde(rename = "memory_write")]
    MemoryWrite,
    #[serde(rename = "approval")]
    Approval,
    #[serde(rename = "policy_check")]
    PolicyCheck,
    #[serde(rename = "artifact")]
    Artifact,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "retry")]
    Retry,
    #[serde(rename = "cost")]
    Cost,
    #[serde(rename = "output")]
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunTraceGraph {
    pub partition: GraphStoragePartition,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub audit_event: GraphAuditEvent,
}

impl RunTraceGraph {
    pub fn from_spec(
        mut spec: RunTraceGraphSpec,
        actor_id: impl Into<String>,
    ) -> Result<Self, StableGraphHashError> {
        spec.scope.run_id = Some(spec.run_id.clone());
        let freshness = Freshness::from_revision(FreshnessSource::Run, &spec.run_id);
        let visibility = Visibility::for_scope(&spec.scope).redacted();
        let partition = GraphStoragePartition::run_ephemeral(
            spec.scope.clone(),
            GraphRetentionPolicy::audit_retained(86_400_000),
        );
        let run_id = node_id(&spec.scope, NodeKind::Run, &spec.run_id);
        let mut nodes = vec![graph_node(
            &spec.scope,
            NodeKind::Run,
            &spec.run_id,
            spec.run_id.clone(),
            run_payload(&spec),
            freshness.clone(),
            visibility.clone(),
            Provenance::Observed,
        )];
        let mut edges = Vec::new();

        if let Some(version_id) = &spec.workflow_version_id {
            let workflow_version = node_id(&spec.scope, NodeKind::WorkflowVersion, version_id);
            nodes.push(graph_node(
                &spec.scope,
                NodeKind::WorkflowVersion,
                version_id,
                version_id.clone(),
                payload([("version_id", version_id.clone())]),
                freshness.clone(),
                visibility.clone(),
                Provenance::Observed,
            ));
            edges.push(graph_edge(
                &spec.scope,
                EdgeKind::ObservedIn,
                &run_id,
                &workflow_version,
                GraphPayload::new(),
                freshness.clone(),
                visibility.clone(),
                Provenance::Observed,
            )?);
        }

        let mut denied = 0;
        for event in &spec.events {
            denied += event.policy_denied as u64;
            let event_id = node_id(&spec.scope, event.kind.node_kind(), &event.event_id);
            nodes.push(graph_node(
                &spec.scope,
                event.kind.node_kind(),
                &event.event_id,
                event.event_id.clone(),
                event_payload(event),
                freshness.clone(),
                visibility.clone(),
                Provenance::Observed,
            ));
            edges.push(graph_edge(
                &spec.scope,
                EdgeKind::Contains,
                &run_id,
                &event_id,
                GraphPayload::new(),
                freshness.clone(),
                visibility.clone(),
                Provenance::Observed,
            )?);
            add_trace_links(
                &mut nodes,
                &mut edges,
                &spec.scope,
                &event_id,
                event,
                &freshness,
            )?;
        }

        let audit_event = GraphAuditEvent::new(
            GraphAuditEventType::RunTraceCaptured,
            spec.scope.clone(),
            actor_id,
            GraphAuditTarget::partition(partition.key()),
        )
        .with_metric_counts(nodes.len() as u64, edges.len() as u64, denied, 0)
        .with_safe_detail("run_id", spec.run_id);

        Ok(Self {
            partition,
            nodes,
            edges,
            audit_event,
        })
    }
}

impl RunTraceEventKind {
    pub fn node_kind(&self) -> NodeKind {
        match self {
            Self::ModelCall => NodeKind::ModelCall,
            Self::ToolCall => NodeKind::ToolCall,
            Self::MemoryRead => NodeKind::RetrievedMemory,
            Self::MemoryWrite => NodeKind::MemoryWriteCandidate,
            Self::Approval => NodeKind::ApprovalGate,
            Self::PolicyCheck => NodeKind::PolicyScope,
            Self::Artifact => NodeKind::Artifact,
            Self::Error => NodeKind::Error,
            Self::Retry => NodeKind::Retry,
            Self::Cost => NodeKind::Cost,
            Self::Output => NodeKind::Output,
        }
    }

    pub fn stable_id(&self) -> &'static str {
        match self {
            Self::ModelCall => "model_call",
            Self::ToolCall => "tool_call",
            Self::MemoryRead => "memory_read",
            Self::MemoryWrite => "memory_write",
            Self::Approval => "approval",
            Self::PolicyCheck => "policy_check",
            Self::Artifact => "artifact",
            Self::Error => "error",
            Self::Retry => "retry",
            Self::Cost => "cost",
            Self::Output => "output",
        }
    }
}

fn add_trace_links(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    scope: &GraphScope,
    event_id: &NodeId,
    event: &RunTraceEvent,
    freshness: &Freshness,
) -> Result<(), StableGraphHashError> {
    let visibility = Visibility::for_scope(scope).redacted();
    if let Some(step_id) = &event.workflow_step_id {
        add_link(
            nodes,
            edges,
            scope,
            event_id,
            NodeKind::WorkflowStep,
            step_id,
            EdgeKind::ObservedIn,
            freshness,
            &visibility,
        )?;
    }
    if let Some(tool_name) = &event.tool_name {
        add_link(
            nodes,
            edges,
            scope,
            event_id,
            NodeKind::ToolDefinition,
            tool_name,
            EdgeKind::RequiresTool,
            freshness,
            &visibility,
        )?;
    }
    if let Some(memory_tier) = &event.memory_tier {
        add_link(
            nodes,
            edges,
            scope,
            event_id,
            NodeKind::MemoryTier,
            memory_tier,
            EdgeKind::RequiresMemory,
            freshness,
            &visibility,
        )?;
    }
    if let Some(policy_scope) = &event.policy_scope {
        add_link(
            nodes,
            edges,
            scope,
            event_id,
            NodeKind::PolicyScope,
            policy_scope,
            EdgeKind::GovernedBy,
            freshness,
            &visibility,
        )?;
    }
    if let Some(artifact_ref) = &event.artifact_ref {
        add_link(
            nodes,
            edges,
            scope,
            event_id,
            NodeKind::Artifact,
            artifact_ref,
            EdgeKind::Produces,
            freshness,
            &visibility,
        )?;
    }
    Ok(())
}

fn add_link(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    scope: &GraphScope,
    source: &NodeId,
    kind: NodeKind,
    key: &str,
    edge_kind: EdgeKind,
    freshness: &Freshness,
    visibility: &Visibility,
) -> Result<(), StableGraphHashError> {
    let target = node_id(scope, kind.clone(), key);
    nodes.push(graph_node(
        scope,
        kind,
        key,
        key.to_string(),
        payload([("ref", key.to_string())]),
        freshness.clone(),
        visibility.clone(),
        Provenance::Observed,
    ));
    edges.push(graph_edge(
        scope,
        edge_kind,
        source,
        &target,
        GraphPayload::new(),
        freshness.clone(),
        visibility.clone(),
        Provenance::Observed,
    )?);
    Ok(())
}

fn run_payload(spec: &RunTraceGraphSpec) -> GraphPayload {
    let mut out = payload([("run_id", spec.run_id.clone())]);
    insert_optional(
        &mut out,
        "workflow_version_id",
        spec.workflow_version_id.as_deref(),
    );
    out
}

fn event_payload(event: &RunTraceEvent) -> GraphPayload {
    let mut out = payload([
        ("event_id", event.event_id.clone()),
        ("kind", event.kind.stable_id().to_string()),
    ]);
    insert_optional(
        &mut out,
        "workflow_step_id",
        event.workflow_step_id.as_deref(),
    );
    insert_optional(&mut out, "tool_name", event.tool_name.as_deref());
    insert_optional(&mut out, "memory_tier", event.memory_tier.as_deref());
    insert_optional(&mut out, "policy_scope", event.policy_scope.as_deref());
    insert_optional(&mut out, "artifact_ref", event.artifact_ref.as_deref());
    insert_optional(&mut out, "safe_summary", event.safe_summary.as_deref());
    out.insert("policy_denied".to_string(), event.policy_denied.to_string());
    if let Some(latency_ms) = event.latency_ms {
        out.insert("latency_ms".to_string(), latency_ms.to_string());
    }
    if let Some(cost_microunits) = event.cost_microunits {
        out.insert("cost_microunits".to_string(), cost_microunits.to_string());
    }
    if let Some(occurred_at_unix_ms) = event.occurred_at_unix_ms {
        out.insert(
            "occurred_at_unix_ms".to_string(),
            occurred_at_unix_ms.to_string(),
        );
    }
    out
}
