import { useMemo } from "preact/hooks";
import { Icon } from "../../ui/Icon";

type WorkflowEditFlowMapProps = {
  nodes: any[];
  workflowMcpServers?: string[];
  selectedNodeId?: string;
  onSelectNode?: (nodeId: string) => void;
};

function safeString(value: unknown) {
  return String(value || "").trim();
}

function stringList(value: unknown) {
  return Array.isArray(value)
    ? value.map((entry) => safeString(entry)).filter(Boolean)
    : [];
}

function uniqueStrings(values: string[]) {
  return Array.from(new Set(values.map((value) => safeString(value)).filter(Boolean))).sort();
}

function nodeId(node: any, index = 0) {
  return safeString(node?.nodeId || node?.node_id || node?.id || `node-${index + 1}`);
}

function nodeTitle(node: any, index = 0) {
  return safeString(node?.title || node?.name || node?.nodeId || node?.node_id) || `Step ${index + 1}`;
}

function nodeDependsOn(node: any) {
  return stringList(node?.dependsOn || node?.depends_on);
}

function nodeInputRefs(node: any) {
  const refs = Array.isArray(node?.inputRefs || node?.input_refs)
    ? node.inputRefs || node.input_refs
    : [];
  return refs
    .map((ref: any) => ({
      fromStepId: safeString(ref?.fromStepId || ref?.from_step_id),
      alias: safeString(ref?.alias),
    }))
    .filter((ref) => ref.fromStepId);
}

function toolLooksSendCapable(tool: string) {
  const normalized = safeString(tool).toLowerCase();
  return (
    normalized.includes("send_email") ||
    normalized.includes("sendemail") ||
    normalized.includes("send_draft") ||
    normalized.includes("senddraft") ||
    normalized.includes("send_message") ||
    normalized.includes("sendmessage") ||
    normalized.includes("post_message") ||
    normalized.includes("postmessage")
  );
}

function explicitMcpTools(node: any) {
  return uniqueStrings([...(node?.mcpOtherAllowedTools || []), ...(node?.mcpAllowedTools || [])]);
}

function inferredMcpServersFromTools(tools: string[]) {
  return uniqueStrings(
    tools
      .map((tool) => {
        const match = safeString(tool).match(/^mcp\.([^.]+)\./);
        return match?.[1] || "";
      })
      .filter(Boolean)
  );
}

function mcpServerLabel(server: string) {
  return safeString(server).replace(/^mcp\./, "");
}

function displayMcpServers(servers: string[], max = 2) {
  const labels = servers.map(mcpServerLabel).filter(Boolean);
  if (!labels.length) return "";
  if (labels.length <= max) return labels.join(", ");
  return `${labels.slice(0, max).join(", ")} +${labels.length - max}`;
}

function computeNodeDepths(nodes: any[]) {
  const ids = new Map(nodes.map((node, index) => [nodeId(node, index), node]));
  const cache = new Map<string, number>();
  const visit = (id: string, seen = new Set<string>()): number => {
    if (cache.has(id)) return Number(cache.get(id) || 0);
    if (seen.has(id)) return 0;
    const node = ids.get(id);
    if (!node) return 0;
    const deps = nodeDependsOn(node).filter((dep) => ids.has(dep));
    if (!deps.length) {
      cache.set(id, 0);
      return 0;
    }
    const nextSeen = new Set(seen);
    nextSeen.add(id);
    const depth = deps.reduce((max, dep) => Math.max(max, visit(dep, nextSeen)), 0) + 1;
    cache.set(id, depth);
    return depth;
  };
  for (const id of ids.keys()) visit(id);
  return cache;
}

export function WorkflowEditFlowMap({
  nodes,
  workflowMcpServers = [],
  selectedNodeId,
  onSelectNode,
}: WorkflowEditFlowMapProps) {
  const graph = useMemo(() => {
    const normalizedNodes = Array.isArray(nodes) ? nodes : [];
    const depths = computeNodeDepths(normalizedNodes);
    const byDepth = new Map<number, any[]>();
    const ids = new Set(normalizedNodes.map((node, index) => nodeId(node, index)));
    let edgeCount = 0;
    let missingDependencyCount = 0;
    let customMcpCount = 0;

    normalizedNodes.forEach((node, index) => {
      const id = nodeId(node, index);
      const depth = Number(depths.get(id) || 0);
      byDepth.set(depth, [...(byDepth.get(depth) || []), node]);
      const deps = nodeDependsOn(node);
      edgeCount += deps.length;
      missingDependencyCount += deps.filter((dep) => !ids.has(dep)).length;
      if ((node?.toolAccessMode || "inherit") === "custom" && nodeMcpServers(node).length) {
        customMcpCount += 1;
      }
    });

    return {
      columns: Array.from(byDepth.entries()).sort(([left], [right]) => left - right),
      edgeCount,
      missingDependencyCount,
      customMcpCount,
      startCount: normalizedNodes.filter((node) => nodeDependsOn(node).length === 0).length,
    };
  }, [nodes]);

  function nodeMcpServers(node: any) {
    const tools = explicitMcpTools(node);
    return uniqueStrings([
      ...stringList(node?.mcpAllowedServers || node?.mcp_allowed_servers),
      ...inferredMcpServersFromTools(tools),
    ]);
  }

  if (!nodes?.length) {
    return (
      <div className="rounded-lg border border-slate-800/70 bg-slate-950/30 p-3 text-sm text-slate-400">
        This workflow does not expose flow nodes yet.
      </div>
    );
  }

  return (
    <div className="rounded-xl border border-slate-800/70 bg-slate-950/30 p-3">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="text-xs font-semibold uppercase tracking-[0.16em] text-slate-500">
            Workflow flow
          </div>
          <div className="mt-1 text-sm text-slate-300">
            Select a node to jump to its prompt, model, and MCP controls.
          </div>
        </div>
        <div className="flex flex-wrap gap-2 tcp-text-caption">
          <span className="tcp-badge-info">{nodes.length} nodes</span>
          <span className="tcp-badge-info">{graph.edgeCount} dependencies</span>
          <span className="tcp-badge-info">{graph.startCount} starts</span>
          {graph.customMcpCount ? (
            <span className="tcp-badge-warn">{graph.customMcpCount} task MCP overrides</span>
          ) : null}
          {graph.missingDependencyCount ? (
            <span className="tcp-badge-err">{graph.missingDependencyCount} missing deps</span>
          ) : null}
        </div>
      </div>

      <div className="overflow-x-auto pb-1">
        <div className="flex min-w-max items-stretch gap-3">
          {graph.columns.map(([depth, columnNodes], columnIndex) => (
            <div key={`flow-column-${depth}`} className="contents">
              <div className="grid w-[260px] content-start gap-2">
                <div className="flex items-center justify-between gap-2 text-xs uppercase tracking-wide text-slate-500">
                  <span>Stage {depth + 1}</span>
                  <span>{columnNodes.length}</span>
                </div>
                {columnNodes.map((node, index) => {
                  const id = nodeId(node, index);
                  const active = selectedNodeId === id;
                  const deps = nodeDependsOn(node);
                  const refs = nodeInputRefs(node);
                  const mcpServers =
                    (node?.toolAccessMode || "inherit") === "custom"
                      ? nodeMcpServers(node)
                      : workflowMcpServers;
                  const mcpTools = explicitMcpTools(node);
                  const sendCapable = mcpTools.some(toolLooksSendCapable);
                  const missingDeps = deps.filter(
                    (dep) => !nodes.some((candidate, candidateIndex) => nodeId(candidate, candidateIndex) === dep)
                  );
                  return (
                    <button
                      key={id}
                      type="button"
                      className={`tcp-list-item min-h-[132px] text-left transition ${
                        active ? "border-amber-400/70 bg-amber-400/10" : ""
                      }`}
                      onClick={() => onSelectNode?.(id)}
                    >
                      <div className="flex items-start justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate text-sm font-semibold text-slate-100">
                            {nodeTitle(node, index)}
                          </div>
                          <div className="mt-1 truncate tcp-text-caption text-slate-500">{id}</div>
                        </div>
                        <span className="tcp-badge-info shrink-0">
                          {safeString(node?.agentId || node?.agent_id) || "agent"}
                        </span>
                      </div>
                      <div className="mt-2 line-clamp-2 text-xs leading-5 text-slate-300">
                        {safeString(node?.objective) || "No objective set."}
                      </div>
                      <div className="mt-3 flex flex-wrap gap-1">
                        {deps.length ? (
                          <span className="tcp-badge-muted">{deps.length} upstream</span>
                        ) : (
                          <span className="tcp-badge-info">start</span>
                        )}
                        {refs.length ? <span className="tcp-badge-muted">{refs.length} inputs</span> : null}
                        {safeString(node?.outputKind || node?.output_kind) ? (
                          <span className="tcp-badge-info">
                            {safeString(node?.outputKind || node?.output_kind)}
                          </span>
                        ) : null}
                        {mcpServers.length ? (
                          <span className={sendCapable ? "tcp-badge-err" : "tcp-badge-warn"}>
                            MCP: {displayMcpServers(mcpServers)}
                          </span>
                        ) : null}
                        {missingDeps.length ? (
                          <span className="tcp-badge-err">missing dep</span>
                        ) : null}
                      </div>
                      {deps.length ? (
                        <div className="mt-2 grid gap-1 tcp-text-caption text-slate-500">
                          {deps.slice(0, 3).map((dep) => (
                            <div key={`${id}-${dep}`} className="truncate">
                              from {dep}
                            </div>
                          ))}
                          {deps.length > 3 ? <div>+{deps.length - 3} more upstream</div> : null}
                        </div>
                      ) : null}
                    </button>
                  );
                })}
              </div>
              {columnIndex < graph.columns.length - 1 ? (
                <div
                  className="flex w-8 flex-col items-center justify-center gap-2 text-slate-500"
                  aria-hidden="true"
                >
                  <span className="h-10 w-px bg-slate-800"></span>
                  <Icon name="arrow-right" />
                  <span className="h-10 w-px bg-slate-800"></span>
                </div>
              ) : null}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
