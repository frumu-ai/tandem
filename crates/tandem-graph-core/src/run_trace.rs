use crate::graph_build::{insert_optional, node_id, payload, GraphBuildContext};
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
        let workflow_scope = workflow_graph_scope(&spec.scope);
        let freshness = Freshness::from_revision(FreshnessSource::Run, &spec.run_id);
        let visibility = Visibility::for_scope(&spec.scope).redacted();
        let run_context = GraphBuildContext::new(
            &spec.scope,
            freshness.clone(),
            visibility.clone(),
            Provenance::Observed,
        );
        let partition = GraphStoragePartition::run_ephemeral(
            spec.scope.clone(),
            GraphRetentionPolicy::audit_retained(86_400_000),
        );
        let run_id = node_id(&spec.scope, NodeKind::Run, &spec.run_id);
        let mut nodes = vec![run_context.node(
            NodeKind::Run,
            &spec.run_id,
            spec.run_id.clone(),
            run_payload(&spec),
        )];
        let mut edges = Vec::new();

        if let Some(version_id) = &spec.workflow_version_id {
            let workflow_version = node_id(&workflow_scope, NodeKind::WorkflowVersion, version_id);
            edges.push(run_context.edge(
                EdgeKind::ObservedIn,
                &run_id,
                &workflow_version,
                GraphPayload::new(),
            )?);
        }

        let mut denied = 0;
        for event in &spec.events {
            denied += event.policy_denied as u64;
            let event_id = node_id(&spec.scope, event.kind.node_kind(), &event.event_id);
            nodes.push(run_context.node(
                event.kind.node_kind(),
                &event.event_id,
                event.event_id.clone(),
                event_payload(event),
            ));
            edges.push(run_context.edge(
                EdgeKind::Contains,
                &run_id,
                &event_id,
                GraphPayload::new(),
            )?);
            add_trace_links(
                &mut nodes,
                &mut edges,
                &run_context,
                &workflow_scope,
                &event_id,
                event,
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
    run_context: &GraphBuildContext,
    workflow_scope: &GraphScope,
    event_id: &NodeId,
    event: &RunTraceEvent,
) -> Result<(), StableGraphHashError> {
    if let Some(step_id) = &event.workflow_step_id {
        edges.push(run_context.edge(
            EdgeKind::ObservedIn,
            event_id,
            &node_id(workflow_scope, NodeKind::WorkflowStep, step_id),
            GraphPayload::new(),
        )?);
    }
    if let Some(tool_name) = &event.tool_name {
        add_link(
            nodes,
            edges,
            run_context,
            event_id,
            LinkTarget::new(NodeKind::ToolDefinition, tool_name, EdgeKind::RequiresTool),
        )?;
    }
    if let Some(memory_tier) = &event.memory_tier {
        add_link(
            nodes,
            edges,
            run_context,
            event_id,
            LinkTarget::new(NodeKind::MemoryTier, memory_tier, EdgeKind::RequiresMemory),
        )?;
    }
    if let Some(policy_scope) = &event.policy_scope {
        add_link(
            nodes,
            edges,
            run_context,
            event_id,
            LinkTarget::new(NodeKind::PolicyScope, policy_scope, EdgeKind::GovernedBy),
        )?;
    }
    if let Some(artifact_ref) = &event.artifact_ref {
        add_link(
            nodes,
            edges,
            run_context,
            event_id,
            LinkTarget::new(NodeKind::Artifact, artifact_ref, EdgeKind::Produces),
        )?;
    }
    Ok(())
}

struct LinkTarget<'a> {
    kind: NodeKind,
    key: &'a str,
    edge_kind: EdgeKind,
}

impl<'a> LinkTarget<'a> {
    fn new(kind: NodeKind, key: &'a str, edge_kind: EdgeKind) -> Self {
        Self {
            kind,
            key,
            edge_kind,
        }
    }
}

fn add_link(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    context: &GraphBuildContext,
    source: &NodeId,
    target: LinkTarget<'_>,
) -> Result<(), StableGraphHashError> {
    let target_id = node_id(&source.scope, target.kind.clone(), target.key);
    nodes.push(context.node(
        target.kind,
        target.key,
        target.key.to_string(),
        payload([("ref", target.key.to_string())]),
    ));
    edges.push(context.edge(target.edge_kind, source, &target_id, GraphPayload::new())?);
    Ok(())
}

fn workflow_graph_scope(scope: &GraphScope) -> GraphScope {
    GraphScope {
        run_id: None,
        ..scope.clone()
    }
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
