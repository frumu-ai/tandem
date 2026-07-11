import type { OrchestrationSpec } from "@frumu/tandem-client";
import { analyzeGraph } from "./graph";
import type { Point } from "./model";
import { updatePersistedPositions } from "./model";

export type LayoutOptions = {
  origin?: Point;
  rankSpacing?: number;
  nodeSpacing?: number;
};

export function leftToRightPositions(
  spec: OrchestrationSpec,
  options: LayoutOptions = {}
): Map<string, Point> {
  const origin = options.origin ?? { x: 0, y: 0 };
  const rankSpacing = options.rankSpacing ?? 320;
  const nodeSpacing = options.nodeSpacing ?? 160;
  const analysis = analyzeGraph(spec);
  const componentByNode = new Map<string, number>();
  analysis.stronglyConnectedComponents.forEach((component, componentIndex) => {
    for (const nodeId of component) componentByNode.set(nodeId, componentIndex);
  });
  const componentEdges = new Map<number, Set<number>>();
  const indegree = new Map<number, number>();
  analysis.stronglyConnectedComponents.forEach((_, index) => indegree.set(index, 0));
  for (const edge of spec.edges) {
    const source = componentByNode.get(edge.from_node_id);
    const target = componentByNode.get(edge.to_node_id);
    if (source === undefined || target === undefined || source === target) continue;
    const targets = componentEdges.get(source) ?? new Set<number>();
    if (!targets.has(target)) {
      targets.add(target);
      componentEdges.set(source, targets);
      indegree.set(target, (indegree.get(target) ?? 0) + 1);
    }
  }
  const rootComponent = componentByNode.get(spec.root_node_id);
  const rank = new Map<number, number>();
  if (rootComponent !== undefined) rank.set(rootComponent, 0);
  const queue = [...indegree.entries()]
    .filter(([, count]) => count === 0)
    .map(([index]) => index)
    .sort((a, b) => a - b);
  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    const source = queue[cursor];
    const sourceRank = rank.get(source) ?? (source === rootComponent ? 0 : 0);
    for (const target of componentEdges.get(source) ?? []) {
      rank.set(target, Math.max(rank.get(target) ?? 0, sourceRank + 1));
      const remaining = (indegree.get(target) ?? 1) - 1;
      indegree.set(target, remaining);
      if (remaining === 0) queue.push(target);
    }
  }
  const reachableComponents = new Set(
    [...analysis.reachableNodeIds].map((nodeId) => componentByNode.get(nodeId)!)
  );
  // Re-rank the root subgraph independently so edges from detached components
  // into the root cannot shift the authored entry point to the right.
  for (const componentId of reachableComponents) rank.set(componentId, 0);
  for (const source of queue) {
    if (!reachableComponents.has(source)) continue;
    for (const target of componentEdges.get(source) ?? []) {
      if (!reachableComponents.has(target)) continue;
      rank.set(target, Math.max(rank.get(target) ?? 0, (rank.get(source) ?? 0) + 1));
    }
  }
  const maxReachableRank = Math.max(0, ...[...reachableComponents].map((id) => rank.get(id) ?? 0));
  for (const componentId of componentByNode.values()) {
    if (!reachableComponents.has(componentId))
      rank.set(componentId, (rank.get(componentId) ?? 0) + maxReachableRank + 1);
  }
  const nodesByRank = new Map<number, string[]>();
  for (const node of spec.nodes) {
    const nodeRank = rank.get(componentByNode.get(node.node_id)!) ?? 0;
    const members = nodesByRank.get(nodeRank) ?? [];
    members.push(node.node_id);
    nodesByRank.set(nodeRank, members);
  }
  const positions = new Map<string, Point>();
  for (const [nodeRank, nodeIds] of [...nodesByRank].sort(([a], [b]) => a - b)) {
    nodeIds.sort((a, b) => {
      const ay = analysis.index.nodesById.get(a)?.position.y ?? 0;
      const by = analysis.index.nodesById.get(b)?.position.y ?? 0;
      return ay - by || a.localeCompare(b);
    });
    const offset = ((nodeIds.length - 1) * nodeSpacing) / 2;
    nodeIds.forEach((nodeId, index) => {
      positions.set(nodeId, {
        x: origin.x + nodeRank * rankSpacing,
        y: origin.y + index * nodeSpacing - offset,
      });
    });
  }
  return positions;
}

export function autoLayoutLeftToRight(
  spec: OrchestrationSpec,
  options?: LayoutOptions
): OrchestrationSpec {
  return updatePersistedPositions(spec, leftToRightPositions(spec, options));
}
