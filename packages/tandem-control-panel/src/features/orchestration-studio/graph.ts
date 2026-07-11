import type {
  OrchestrationEdgeSpec,
  OrchestrationNodeSpec,
  OrchestrationSpec,
} from "@frumu/tandem-client";

export type GraphIndex = {
  nodesById: ReadonlyMap<string, OrchestrationNodeSpec>;
  outgoing: ReadonlyMap<string, readonly OrchestrationEdgeSpec[]>;
  incoming: ReadonlyMap<string, readonly OrchestrationEdgeSpec[]>;
};

export type GraphCounts = {
  nodes: number;
  edges: number;
  workflows: number;
  waits: number;
  terminals: number;
  reachable: number;
  unreachable: number;
  orphanNodes: number;
  loops: number;
};

export type GraphAnalysis = {
  index: GraphIndex;
  counts: GraphCounts;
  reachableNodeIds: ReadonlySet<string>;
  orphanNodeIds: readonly string[];
  terminalNodeIds: readonly string[];
  reachableTerminalIds: readonly string[];
  canReachTerminalNodeIds: ReadonlySet<string>;
  stronglyConnectedComponents: readonly (readonly string[])[];
  loopComponents: readonly (readonly string[])[];
};

export function buildGraphIndex(
  nodes: readonly OrchestrationNodeSpec[],
  edges: readonly OrchestrationEdgeSpec[]
): GraphIndex {
  const nodesById = new Map(nodes.map((node) => [node.node_id, node]));
  const outgoing = new Map<string, OrchestrationEdgeSpec[]>();
  const incoming = new Map<string, OrchestrationEdgeSpec[]>();
  for (const edge of edges) {
    if (!nodesById.has(edge.from_node_id) || !nodesById.has(edge.to_node_id)) continue;
    const out = outgoing.get(edge.from_node_id) ?? [];
    out.push(edge);
    outgoing.set(edge.from_node_id, out);
    const inc = incoming.get(edge.to_node_id) ?? [];
    inc.push(edge);
    incoming.set(edge.to_node_id, inc);
  }
  return { nodesById, outgoing, incoming };
}

export function reachableFrom(
  roots: readonly string[],
  adjacency: ReadonlyMap<string, readonly OrchestrationEdgeSpec[]>,
  direction: "outgoing" | "incoming" = "outgoing"
): Set<string> {
  const seen = new Set<string>();
  const queue = [...roots];
  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    const nodeId = queue[cursor];
    if (seen.has(nodeId)) continue;
    seen.add(nodeId);
    for (const edge of adjacency.get(nodeId) ?? []) {
      queue.push(direction === "outgoing" ? edge.to_node_id : edge.from_node_id);
    }
  }
  return seen;
}

export function stronglyConnectedComponents(
  nodeIds: readonly string[],
  outgoing: ReadonlyMap<string, readonly OrchestrationEdgeSpec[]>
): string[][] {
  let nextIndex = 0;
  const index = new Map<string, number>();
  const lowLink = new Map<string, number>();
  const stack: string[] = [];
  const onStack = new Set<string>();
  const components: string[][] = [];

  const visit = (nodeId: string): void => {
    index.set(nodeId, nextIndex);
    lowLink.set(nodeId, nextIndex);
    nextIndex += 1;
    stack.push(nodeId);
    onStack.add(nodeId);
    for (const edge of outgoing.get(nodeId) ?? []) {
      const target = edge.to_node_id;
      if (!index.has(target)) {
        visit(target);
        lowLink.set(nodeId, Math.min(lowLink.get(nodeId)!, lowLink.get(target)!));
      } else if (onStack.has(target)) {
        lowLink.set(nodeId, Math.min(lowLink.get(nodeId)!, index.get(target)!));
      }
    }
    if (lowLink.get(nodeId) !== index.get(nodeId)) return;
    const component: string[] = [];
    while (stack.length) {
      const member = stack.pop()!;
      onStack.delete(member);
      component.push(member);
      if (member === nodeId) break;
    }
    components.push(component.sort());
  };

  for (const nodeId of nodeIds) if (!index.has(nodeId)) visit(nodeId);
  return components;
}

export function analyzeGraph(spec: OrchestrationSpec): GraphAnalysis {
  const index = buildGraphIndex(spec.nodes, spec.edges);
  const reachableNodeIds = index.nodesById.has(spec.root_node_id)
    ? reachableFrom([spec.root_node_id], index.outgoing)
    : new Set<string>();
  const terminalNodeIds = spec.nodes
    .filter((node) => node.kind === "terminal")
    .map((node) => node.node_id);
  const canReachTerminalNodeIds = reachableFrom(terminalNodeIds, index.incoming, "incoming");
  const reachableTerminalIds = terminalNodeIds.filter((id) => reachableNodeIds.has(id));
  const orphanNodeIds = spec.nodes
    .filter(
      (node) => node.node_id !== spec.root_node_id && !index.incoming.get(node.node_id)?.length
    )
    .map((node) => node.node_id);
  const components = stronglyConnectedComponents(
    spec.nodes.map((node) => node.node_id),
    index.outgoing
  );
  const loopComponents = components.filter(
    (component) =>
      component.length > 1 ||
      (index.outgoing.get(component[0]) ?? []).some((edge) => edge.to_node_id === component[0])
  );
  return {
    index,
    reachableNodeIds,
    orphanNodeIds,
    terminalNodeIds,
    reachableTerminalIds,
    canReachTerminalNodeIds,
    stronglyConnectedComponents: components,
    loopComponents,
    counts: {
      nodes: spec.nodes.length,
      edges: spec.edges.length,
      workflows: spec.nodes.filter((node) => node.kind === "workflow").length,
      waits: spec.nodes.filter((node) => node.kind === "wait").length,
      terminals: terminalNodeIds.length,
      reachable: reachableNodeIds.size,
      unreachable: spec.nodes.length - reachableNodeIds.size,
      orphanNodes: orphanNodeIds.length,
      loops: loopComponents.length,
    },
  };
}
