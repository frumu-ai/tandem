import type {
  OrchestrationEdgeSpec,
  OrchestrationNodeSpec,
  OrchestrationSpec,
  OrchestrationTenantContext,
} from "@frumu/tandem-client";

export type Point = { x: number; y: number };

export type OrchestrationFlowNode = {
  id: string;
  type: OrchestrationNodeSpec["kind"];
  position: Point;
  data: {
    node: OrchestrationNodeSpec;
    label: string;
  };
};

export type OrchestrationFlowEdge = {
  id: string;
  source: string;
  target: string;
  label: string;
  data: { edge: OrchestrationEdgeSpec };
};

export type OrchestrationFlowGraph = {
  nodes: OrchestrationFlowNode[];
  edges: OrchestrationFlowEdge[];
};

export type CreateDraftOptions = {
  name?: string;
  description?: string;
  orchestrationId?: string;
  rootNodeId?: string;
  nodes?: readonly OrchestrationNodeSpec[];
  edges?: readonly OrchestrationEdgeSpec[];
  tenantContext?: OrchestrationTenantContext;
  now?: number;
  createId?: () => string;
};

const defaultTenantContext: OrchestrationTenantContext = {
  org_id: "local",
  workspace_id: "local",
  source: "local_implicit",
};

function fallbackId(): string {
  const random = Math.random().toString(36).slice(2, 10);
  return `orchestration-${Date.now().toString(36)}-${random}`;
}

function finitePoint(position: Point | undefined): Point {
  return {
    x: Number.isFinite(position?.x) ? position!.x : 0,
    y: Number.isFinite(position?.y) ? position!.y : 0,
  };
}

function cloneNode(node: OrchestrationNodeSpec, position = node.position): OrchestrationNodeSpec {
  return { ...node, position: finitePoint(position) };
}

function cloneEdge(edge: OrchestrationEdgeSpec): OrchestrationEdgeSpec {
  return { ...edge };
}

export function createOrchestrationDraft(options: CreateDraftOptions = {}): OrchestrationSpec {
  const now = options.now ?? Date.now();
  const nodes = (options.nodes ?? []).map((node) => cloneNode(node));
  const rootNodeId = options.rootNodeId ?? nodes[0]?.node_id ?? "";
  return {
    schema_version: 1,
    orchestration_id: options.orchestrationId ?? (options.createId ?? fallbackId)(),
    name: options.name?.trim() || "Untitled orchestration",
    ...(options.description?.trim() ? { description: options.description.trim() } : {}),
    status: "draft",
    version: 0,
    root_node_id: rootNodeId,
    nodes,
    edges: (options.edges ?? []).map(cloneEdge),
    goal_policy: { max_hops: 100, on_limit: "pause_for_review" },
    tenant_context: { ...(options.tenantContext ?? defaultTenantContext) },
    created_at_ms: now,
    updated_at_ms: now,
  };
}

export function toFlowGraph(spec: OrchestrationSpec): OrchestrationFlowGraph {
  return {
    nodes: spec.nodes.map((node) => {
      const canonical = cloneNode(node);
      return {
        id: node.node_id,
        type: node.kind,
        position: finitePoint(node.position),
        data: { node: canonical, label: node.name },
      };
    }),
    edges: spec.edges.map((edge) => {
      const canonical = cloneEdge(edge);
      return {
        id: edge.edge_id,
        source: edge.from_node_id,
        target: edge.to_node_id,
        label: edge.transition_key,
        data: { edge: canonical },
      };
    }),
  };
}

/** Rebuilds canonical graph fields while preserving all non-graph spec fields. */
export function fromFlowGraph(
  spec: OrchestrationSpec,
  graph: OrchestrationFlowGraph,
  updatedAtMs = spec.updated_at_ms
): OrchestrationSpec {
  return {
    ...spec,
    updated_at_ms: updatedAtMs,
    nodes: graph.nodes.map((flowNode) => ({
      ...flowNode.data.node,
      node_id: flowNode.id,
      name: flowNode.data.label,
      position: finitePoint(flowNode.position),
    })),
    edges: graph.edges.map((flowEdge) => ({
      ...flowEdge.data.edge,
      edge_id: flowEdge.id,
      from_node_id: flowEdge.source,
      to_node_id: flowEdge.target,
      transition_key: flowEdge.label,
    })),
  };
}

export function updatePersistedPositions(
  spec: OrchestrationSpec,
  positions: ReadonlyMap<string, Point> | Readonly<Record<string, Point>>
): OrchestrationSpec {
  const lookup = (nodeId: string): Point | undefined =>
    positions instanceof Map ? positions.get(nodeId) : positions[nodeId];
  let changed = false;
  const nodes = spec.nodes.map((node) => {
    const next = lookup(node.node_id);
    if (!next || !Number.isFinite(next.x) || !Number.isFinite(next.y)) return node;
    if (node.position.x === next.x && node.position.y === next.y) return node;
    changed = true;
    return cloneNode(node, next);
  });
  return changed ? { ...spec, nodes } : spec;
}
