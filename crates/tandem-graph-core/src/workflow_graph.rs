use crate::graph_build::{graph_edge, graph_node, insert_optional, node_id, payload};
use crate::{
    EdgeKind, Freshness, FreshnessSource, GraphEdge, GraphNode, GraphPayload, GraphRetentionPolicy,
    GraphScope, GraphStoragePartition, NodeId, NodeKind, Provenance, StableGraphHashError,
    Visibility,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowGraphSpec {
    pub scope: GraphScope,
    pub template: WorkflowTemplateGraphNode,
    pub version: WorkflowVersionGraphNode,
    pub steps: Vec<WorkflowStepGraphNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowTemplateGraphNode {
    pub template_id: String,
    pub name: String,
    pub owner_id: String,
    pub template_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowVersionGraphNode {
    pub version_id: String,
    pub workflow_hash: String,
    pub policy_hash: Option<String>,
    pub prompt_hash: Option<String>,
    pub tool_schema_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowStepGraphNode {
    pub step_id: String,
    pub title: String,
    pub kind: String,
    pub depends_on: Vec<String>,
    pub required_tools: Vec<String>,
    pub memory_tiers: Vec<String>,
    pub approval_gates: Vec<String>,
    pub policy_scopes: Vec<String>,
    pub artifact_refs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowStepDependencySummary {
    pub depends_on: Vec<String>,
    pub required_tools: Vec<String>,
    pub memory_tiers: Vec<String>,
    pub approval_gates: Vec<String>,
    pub policy_scopes: Vec<String>,
    pub artifact_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowGraph {
    pub partition: GraphStoragePartition,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub step_dependencies: Vec<(String, WorkflowStepDependencySummary)>,
}

impl WorkflowGraph {
    pub fn from_spec(spec: WorkflowGraphSpec) -> Result<Self, StableGraphHashError> {
        let freshness = Freshness::from_revision(
            FreshnessSource::WorkflowVersion,
            &spec.version.workflow_hash,
        );
        let visibility = Visibility::for_scope(&spec.scope);
        let partition = GraphStoragePartition::workflow_version(
            spec.scope.clone(),
            spec.version.workflow_hash.clone(),
            GraphRetentionPolicy::durable_project(),
        );

        let template_id = node_id(
            &spec.scope,
            NodeKind::WorkflowTemplate,
            &spec.template.template_id,
        );
        let version_id = node_id(
            &spec.scope,
            NodeKind::WorkflowVersion,
            &spec.version.version_id,
        );
        let mut nodes = vec![
            graph_node(
                &spec.scope,
                NodeKind::WorkflowTemplate,
                &spec.template.template_id,
                spec.template.name.clone(),
                payload([
                    ("template_id", spec.template.template_id.clone()),
                    ("owner_id", spec.template.owner_id.clone()),
                ]),
                freshness.clone(),
                visibility.clone(),
                Provenance::Configured,
            ),
            graph_node(
                &spec.scope,
                NodeKind::WorkflowVersion,
                &spec.version.version_id,
                spec.version.version_id.clone(),
                version_payload(&spec.version),
                freshness.clone(),
                visibility.clone(),
                Provenance::Configured,
            ),
        ];
        let mut edges = vec![graph_edge(
            &spec.scope,
            EdgeKind::Contains,
            &template_id,
            &version_id,
            GraphPayload::new(),
            freshness.clone(),
            visibility.clone(),
            Provenance::Configured,
        )?];
        let mut step_dependencies = Vec::new();

        for step in spec.steps {
            let step_id = node_id(&spec.scope, NodeKind::WorkflowStep, &step.step_id);
            nodes.push(graph_node(
                &spec.scope,
                NodeKind::WorkflowStep,
                &step.step_id,
                step.title.clone(),
                payload([
                    ("step_id", step.step_id.clone()),
                    ("kind", step.kind.clone()),
                ]),
                freshness.clone(),
                visibility.clone(),
                Provenance::Configured,
            ));
            edges.push(graph_edge(
                &spec.scope,
                EdgeKind::Contains,
                &version_id,
                &step_id,
                GraphPayload::new(),
                freshness.clone(),
                visibility.clone(),
                Provenance::Configured,
            )?);
            add_dependency_edges(
                &mut nodes,
                &mut edges,
                &spec.scope,
                &step_id,
                &step,
                &freshness,
            )?;
            step_dependencies.push((
                step.step_id.clone(),
                WorkflowStepDependencySummary::from(&step),
            ));
        }

        Ok(Self {
            partition,
            nodes,
            edges,
            step_dependencies,
        })
    }

    pub fn dependencies_for_step(&self, step_id: &str) -> Option<&WorkflowStepDependencySummary> {
        self.step_dependencies
            .iter()
            .find_map(|(candidate, summary)| (candidate == step_id).then_some(summary))
    }
}

impl From<&WorkflowStepGraphNode> for WorkflowStepDependencySummary {
    fn from(step: &WorkflowStepGraphNode) -> Self {
        Self {
            depends_on: step.depends_on.clone(),
            required_tools: step.required_tools.clone(),
            memory_tiers: step.memory_tiers.clone(),
            approval_gates: step.approval_gates.clone(),
            policy_scopes: step.policy_scopes.clone(),
            artifact_refs: step.artifact_refs.clone(),
        }
    }
}

fn add_dependency_edges(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    scope: &GraphScope,
    step_id: &NodeId,
    step: &WorkflowStepGraphNode,
    freshness: &Freshness,
) -> Result<(), StableGraphHashError> {
    let visibility = Visibility::for_scope(scope);
    for upstream in &step.depends_on {
        edges.push(edge_to_existing_step(
            scope,
            step_id,
            upstream,
            freshness,
            visibility.clone(),
        )?);
    }
    add_external_dependencies(
        nodes,
        edges,
        scope,
        step_id,
        &step.required_tools,
        NodeKind::ToolDefinition,
        EdgeKind::RequiresTool,
        freshness,
    )?;
    add_external_dependencies(
        nodes,
        edges,
        scope,
        step_id,
        &step.memory_tiers,
        NodeKind::MemoryTier,
        EdgeKind::RequiresMemory,
        freshness,
    )?;
    add_external_dependencies(
        nodes,
        edges,
        scope,
        step_id,
        &step.approval_gates,
        NodeKind::ApprovalGate,
        EdgeKind::RequiresApproval,
        freshness,
    )?;
    add_external_dependencies(
        nodes,
        edges,
        scope,
        step_id,
        &step.policy_scopes,
        NodeKind::PolicyScope,
        EdgeKind::GovernedBy,
        freshness,
    )?;
    add_external_dependencies(
        nodes,
        edges,
        scope,
        step_id,
        &step.artifact_refs,
        NodeKind::Artifact,
        EdgeKind::Produces,
        freshness,
    )?;
    Ok(())
}

fn add_external_dependencies(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    scope: &GraphScope,
    step_id: &NodeId,
    refs: &[String],
    kind: NodeKind,
    edge_kind: EdgeKind,
    freshness: &Freshness,
) -> Result<(), StableGraphHashError> {
    let visibility = Visibility::for_scope(scope);
    for reference in refs {
        let target = node_id(scope, kind.clone(), reference);
        nodes.push(graph_node(
            scope,
            kind.clone(),
            reference,
            reference.clone(),
            payload([("ref", reference.clone())]),
            freshness.clone(),
            visibility.clone(),
            Provenance::Configured,
        ));
        edges.push(graph_edge(
            scope,
            edge_kind.clone(),
            step_id,
            &target,
            GraphPayload::new(),
            freshness.clone(),
            visibility.clone(),
            Provenance::Configured,
        )?);
    }
    Ok(())
}

fn edge_to_existing_step(
    scope: &GraphScope,
    step_id: &NodeId,
    upstream: &str,
    freshness: &Freshness,
    visibility: Visibility,
) -> Result<GraphEdge, StableGraphHashError> {
    let upstream_id = node_id(scope, NodeKind::WorkflowStep, upstream);
    graph_edge(
        scope,
        EdgeKind::DependsOn,
        step_id,
        &upstream_id,
        GraphPayload::new(),
        freshness.clone(),
        visibility,
        Provenance::Configured,
    )
}

fn version_payload(version: &WorkflowVersionGraphNode) -> GraphPayload {
    let mut out = payload([
        ("version_id", version.version_id.clone()),
        ("workflow_hash", version.workflow_hash.clone()),
    ]);
    insert_optional(&mut out, "policy_hash", version.policy_hash.as_deref());
    insert_optional(&mut out, "prompt_hash", version.prompt_hash.as_deref());
    insert_optional(
        &mut out,
        "tool_schema_hash",
        version.tool_schema_hash.as_deref(),
    );
    out
}
