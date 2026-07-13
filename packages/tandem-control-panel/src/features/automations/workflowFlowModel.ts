function safeString(value: unknown) {
  return String(value || "").trim();
}

function stringList(value: unknown) {
  return Array.isArray(value) ? value.map((entry) => safeString(entry)).filter(Boolean) : [];
}

export function workflowFlowNodeId(node: any, index = 0) {
  return safeString(node?.nodeId || node?.node_id || node?.id || `node-${index + 1}`);
}

export function workflowFlowNodeDependencies(node: any) {
  return stringList(node?.dependsOn || node?.depends_on);
}

function computeNodeDepths(nodes: any[]) {
  const nodesById = new Map(nodes.map((node, index) => [workflowFlowNodeId(node, index), node]));
  const cache = new Map<string, number>();
  const visit = (id: string, seen = new Set<string>()): number => {
    if (cache.has(id)) return Number(cache.get(id) || 0);
    if (seen.has(id)) return 0;
    const node = nodesById.get(id);
    if (!node) return 0;
    const dependencies = workflowFlowNodeDependencies(node).filter((dependency) =>
      nodesById.has(dependency)
    );
    if (!dependencies.length) {
      cache.set(id, 0);
      return 0;
    }
    const nextSeen = new Set(seen);
    nextSeen.add(id);
    const depth =
      dependencies.reduce(
        (maximum, dependency) => Math.max(maximum, visit(dependency, nextSeen)),
        0
      ) + 1;
    cache.set(id, depth);
    return depth;
  };
  for (const id of nodesById.keys()) visit(id);
  return cache;
}

export function buildWorkflowFlowGraph({
  nodes,
  executionMode,
  maxParallelAgents,
}: {
  nodes: any[];
  executionMode?: string;
  maxParallelAgents?: number | string;
}) {
  const normalizedNodes = Array.isArray(nodes) ? nodes : [];
  const nodeIds = new Set(normalizedNodes.map((node, index) => workflowFlowNodeId(node, index)));
  const depths = computeNodeDepths(normalizedNodes);
  const nodesByDepth = new Map<number, any[]>();
  let edgeCount = 0;
  let missingDependencyCount = 0;

  normalizedNodes.forEach((node, index) => {
    const id = workflowFlowNodeId(node, index);
    const depth = Number(depths.get(id) || 0);
    nodesByDepth.set(depth, [...(nodesByDepth.get(depth) || []), node]);
    const dependencies = workflowFlowNodeDependencies(node);
    edgeCount += dependencies.length;
    missingDependencyCount += dependencies.filter((dependency) => !nodeIds.has(dependency)).length;
  });

  const configuredLimit = Math.max(1, Math.floor(Number(maxParallelAgents) || 1));
  const concurrencyLimit = executionMode === "single" ? 1 : configuredLimit;
  const stages = Array.from(nodesByDepth.entries())
    .sort(([left], [right]) => left - right)
    .map(([depth, stageNodes]) => ({
      depth,
      nodes: stageNodes,
      concurrentTasks: Math.min(stageNodes.length, concurrencyLimit),
      hasParallelTasks: stageNodes.length > 1,
    }));

  return {
    stages,
    edgeCount,
    missingDependencyCount,
    startCount: normalizedNodes.filter((node) => workflowFlowNodeDependencies(node).length === 0)
      .length,
    parallelStageCount: stages.filter((stage) => stage.hasParallelTasks).length,
    maxConcurrentTasks: stages.reduce(
      (maximum, stage) => Math.max(maximum, stage.concurrentTasks),
      0
    ),
    concurrencyLimit,
  };
}
