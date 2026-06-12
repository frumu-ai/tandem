use crate::graph_build::{insert_optional, node_id, payload, GraphBuildContext};
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
        let context = GraphBuildContext::new(
            &spec.scope,
            freshness.clone(),
            visibility.clone(),
            Provenance::Configured,
        );
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
            context.node(
                NodeKind::WorkflowTemplate,
                &spec.template.template_id,
                spec.template.name.clone(),
                payload([
                    ("template_id", spec.template.template_id.clone()),
                    ("owner_id", spec.template.owner_id.clone()),
                ]),
            ),
            context.node(
                NodeKind::WorkflowVersion,
                &spec.version.version_id,
                spec.version.version_id.clone(),
                version_payload(&spec.version),
            ),
        ];
        let mut edges = vec![context.edge(
            EdgeKind::Contains,
            &template_id,
            &version_id,
            GraphPayload::new(),
        )?];
        let mut step_dependencies = Vec::new();

        for step in spec.steps {
            let step_id = node_id(&spec.scope, NodeKind::WorkflowStep, &step.step_id);
            nodes.push(context.node(
                NodeKind::WorkflowStep,
                &step.step_id,
                step.title.clone(),
                payload([
                    ("step_id", step.step_id.clone()),
                    ("kind", step.kind.clone()),
                ]),
            ));
            edges.push(context.edge(
                EdgeKind::Contains,
                &version_id,
                &step_id,
                GraphPayload::new(),
            )?);
            add_dependency_edges(
                &mut nodes,
                &mut edges,
                &spec.scope,
                &step_id,
                &step,
                &context,
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
    context: &GraphBuildContext,
) -> Result<(), StableGraphHashError> {
    let dependency_context = context.with_scope(scope);
    for upstream in &step.depends_on {
        edges.push(edge_to_existing_step(
            &dependency_context,
            scope,
            step_id,
            upstream,
        )?);
    }
    let external_context = context.with_scope(scope);
    add_external_dependencies(
        nodes,
        edges,
        &external_context,
        step_id,
        &step.required_tools,
        NodeKind::ToolDefinition,
        EdgeKind::RequiresTool,
    )?;
    add_external_dependencies(
        nodes,
        edges,
        &external_context,
        step_id,
        &step.memory_tiers,
        NodeKind::MemoryTier,
        EdgeKind::RequiresMemory,
    )?;
    add_external_dependencies(
        nodes,
        edges,
        &external_context,
        step_id,
        &step.approval_gates,
        NodeKind::ApprovalGate,
        EdgeKind::RequiresApproval,
    )?;
    add_external_dependencies(
        nodes,
        edges,
        &external_context,
        step_id,
        &step.policy_scopes,
        NodeKind::PolicyScope,
        EdgeKind::GovernedBy,
    )?;
    add_external_dependencies(
        nodes,
        edges,
        &external_context,
        step_id,
        &step.artifact_refs,
        NodeKind::Artifact,
        EdgeKind::Produces,
    )?;
    Ok(())
}

fn add_external_dependencies(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    context: &GraphBuildContext,
    step_id: &NodeId,
    refs: &[String],
    kind: NodeKind,
    edge_kind: EdgeKind,
) -> Result<(), StableGraphHashError> {
    for reference in refs {
        let target = node_id(&step_id.scope, kind.clone(), reference);
        nodes.push(context.node(
            kind.clone(),
            reference,
            reference.clone(),
            payload([("ref", reference.clone())]),
        ));
        edges.push(context.edge(edge_kind.clone(), step_id, &target, GraphPayload::new())?);
    }
    Ok(())
}

fn edge_to_existing_step(
    context: &GraphBuildContext,
    scope: &GraphScope,
    step_id: &NodeId,
    upstream: &str,
) -> Result<GraphEdge, StableGraphHashError> {
    let upstream_id = node_id(scope, NodeKind::WorkflowStep, upstream);
    context.edge(
        EdgeKind::DependsOn,
        step_id,
        &upstream_id,
        GraphPayload::new(),
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
