import type {
  OrchestrationEdgeSpec,
  OrchestrationNodeSpec,
  OrchestrationSpec,
  OrchestrationValueBinding,
  OrchestrationWaitSpec,
} from "@frumu/tandem-client";

function mapBinding(
  binding: OrchestrationValueBinding,
  from: string,
  to: string
): OrchestrationValueBinding {
  return binding.source === "node_output" && binding.node_id === from
    ? { ...binding, node_id: to }
    : binding;
}

function mapWaitBindings(
  wait: OrchestrationWaitSpec,
  from: string,
  to: string
): OrchestrationWaitSpec {
  if (wait.kind === "timer" && wait.wake_at)
    return { ...wait, wake_at: mapBinding(wait.wake_at, from, to) };
  if (wait.kind === "webhook") {
    return {
      ...wait,
      correlation: { ...wait.correlation, value: mapBinding(wait.correlation.value, from, to) },
    };
  }
  if (wait.kind === "external_condition")
    return { ...wait, condition_key: mapBinding(wait.condition_key, from, to) };
  return wait;
}

export function addNode(
  spec: OrchestrationSpec,
  node: OrchestrationNodeSpec,
  asRoot = false
): OrchestrationSpec {
  if (spec.nodes.some((candidate) => candidate.node_id === node.node_id))
    throw new Error(`Duplicate node ID: ${node.node_id}`);
  return {
    ...spec,
    root_node_id: asRoot || !spec.root_node_id ? node.node_id : spec.root_node_id,
    nodes: [...spec.nodes, { ...node, position: { ...node.position } }],
  };
}

export function updateNode(
  spec: OrchestrationSpec,
  nodeId: string,
  update: (node: OrchestrationNodeSpec) => OrchestrationNodeSpec
): OrchestrationSpec {
  let changed = false;
  const nodes = spec.nodes.map((node) => {
    if (node.node_id !== nodeId) return node;
    changed = true;
    const next = update(node);
    if (next.node_id !== nodeId) throw new Error("Use renameNode to change node IDs");
    return next;
  });
  return changed ? { ...spec, nodes } : spec;
}

export function renameNode(
  spec: OrchestrationSpec,
  nodeId: string,
  nextNodeId: string
): OrchestrationSpec {
  if (!nextNodeId.trim()) throw new Error("Node ID cannot be empty");
  if (nodeId !== nextNodeId && spec.nodes.some((node) => node.node_id === nextNodeId))
    throw new Error(`Duplicate node ID: ${nextNodeId}`);
  if (!spec.nodes.some((node) => node.node_id === nodeId) || nodeId === nextNodeId) return spec;
  return {
    ...spec,
    root_node_id: spec.root_node_id === nodeId ? nextNodeId : spec.root_node_id,
    nodes: spec.nodes.map((node) => {
      const renamed = node.node_id === nodeId ? { ...node, node_id: nextNodeId } : node;
      return renamed.kind === "wait"
        ? { ...renamed, wait: mapWaitBindings(renamed.wait, nodeId, nextNodeId) }
        : renamed;
    }),
    edges: spec.edges.map((edge) => ({
      ...edge,
      from_node_id: edge.from_node_id === nodeId ? nextNodeId : edge.from_node_id,
      to_node_id: edge.to_node_id === nodeId ? nextNodeId : edge.to_node_id,
    })),
  };
}

export function removeNode(spec: OrchestrationSpec, nodeId: string): OrchestrationSpec {
  if (!spec.nodes.some((node) => node.node_id === nodeId)) return spec;
  const nodes = spec.nodes.filter((node) => node.node_id !== nodeId);
  return {
    ...spec,
    root_node_id: spec.root_node_id === nodeId ? (nodes[0]?.node_id ?? "") : spec.root_node_id,
    nodes,
    edges: spec.edges.filter((edge) => edge.from_node_id !== nodeId && edge.to_node_id !== nodeId),
  };
}

export function reorderNode(
  spec: OrchestrationSpec,
  nodeId: string,
  toIndex: number
): OrchestrationSpec {
  const fromIndex = spec.nodes.findIndex((node) => node.node_id === nodeId);
  if (fromIndex < 0) return spec;
  const boundedIndex = Math.max(0, Math.min(Math.floor(toIndex), spec.nodes.length - 1));
  if (fromIndex === boundedIndex) return spec;
  const nodes = [...spec.nodes];
  const [node] = nodes.splice(fromIndex, 1);
  nodes.splice(boundedIndex, 0, node);
  return { ...spec, nodes };
}

export function setRootNode(spec: OrchestrationSpec, nodeId: string): OrchestrationSpec {
  if (!spec.nodes.some((node) => node.node_id === nodeId))
    throw new Error(`Unknown root node: ${nodeId}`);
  return spec.root_node_id === nodeId ? spec : { ...spec, root_node_id: nodeId };
}

export function addEdge(spec: OrchestrationSpec, edge: OrchestrationEdgeSpec): OrchestrationSpec {
  if (spec.edges.some((candidate) => candidate.edge_id === edge.edge_id))
    throw new Error(`Duplicate edge ID: ${edge.edge_id}`);
  return { ...spec, edges: [...spec.edges, { ...edge }] };
}

export function updateEdge(
  spec: OrchestrationSpec,
  edgeId: string,
  update: (edge: OrchestrationEdgeSpec) => OrchestrationEdgeSpec
): OrchestrationSpec {
  let changed = false;
  const edges = spec.edges.map((edge) => {
    if (edge.edge_id !== edgeId) return edge;
    changed = true;
    const next = update(edge);
    if (next.edge_id !== edgeId) throw new Error("Remove and add an edge to change its ID");
    return next;
  });
  return changed ? { ...spec, edges } : spec;
}

export function removeEdge(spec: OrchestrationSpec, edgeId: string): OrchestrationSpec {
  const edges = spec.edges.filter((edge) => edge.edge_id !== edgeId);
  return edges.length === spec.edges.length ? spec : { ...spec, edges };
}

export function removeEdgesBetween(
  spec: OrchestrationSpec,
  source: string,
  target: string
): OrchestrationSpec {
  const edges = spec.edges.filter(
    (edge) => edge.from_node_id !== source || edge.to_node_id !== target
  );
  return edges.length === spec.edges.length ? spec : { ...spec, edges };
}

export function pruneDanglingEdges(spec: OrchestrationSpec): OrchestrationSpec {
  const nodeIds = new Set(spec.nodes.map((node) => node.node_id));
  const edges = spec.edges.filter(
    (edge) => nodeIds.has(edge.from_node_id) && nodeIds.has(edge.to_node_id)
  );
  return edges.length === spec.edges.length ? spec : { ...spec, edges };
}
