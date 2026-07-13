import { useMemo } from "preact/hooks";
import { Icon } from "../../ui/Icon";
import {
  buildWorkflowFlowGraph,
  workflowFlowNodeDependencies,
  workflowFlowNodeId,
} from "./workflowFlowModel";

type WorkflowEditFlowMapProps = {
  nodes: any[];
  workflowMcpServers?: string[];
  selectedNodeId?: string;
  onSelectNode?: (nodeId: string) => void;
  executionMode?: string;
  maxParallelAgents?: number | string;
  variant?: "compact" | "full";
};

function safeString(value: unknown) {
  return String(value || "").trim();
}

function stringList(value: unknown) {
  return Array.isArray(value) ? value.map((entry) => safeString(entry)).filter(Boolean) : [];
}

function uniqueStrings(values: string[]) {
  return Array.from(new Set(values.map((value) => safeString(value)).filter(Boolean))).sort();
}

function nodeTitle(node: any, index = 0) {
  return (
    safeString(node?.title || node?.name || node?.nodeId || node?.node_id) || `Step ${index + 1}`
  );
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

export function WorkflowEditFlowMap({
  nodes,
  workflowMcpServers = [],
  selectedNodeId,
  onSelectNode,
  executionMode,
  maxParallelAgents,
  variant = "compact",
}: WorkflowEditFlowMapProps) {
  function nodeMcpServers(node: any) {
    const tools = explicitMcpTools(node);
    return uniqueStrings([
      ...stringList(node?.mcpAllowedServers || node?.mcp_allowed_servers),
      ...inferredMcpServersFromTools(tools),
    ]);
  }

  const graph = useMemo(
    () => buildWorkflowFlowGraph({ nodes, executionMode, maxParallelAgents }),
    [executionMode, maxParallelAgents, nodes]
  );
  const customMcpCount = useMemo(
    () =>
      nodes.filter(
        (node) => (node?.toolAccessMode || "inherit") === "custom" && nodeMcpServers(node).length
      ).length,
    [nodes]
  );

  if (!nodes?.length) {
    return (
      <div className="rounded-lg border border-slate-800/70 bg-slate-950/30 p-3 text-sm text-slate-400">
        This workflow does not expose flow nodes yet.
      </div>
    );
  }

  return (
    <div
      className={`workflow-flow-map rounded-lg border border-slate-800/70 bg-slate-950/30 p-3 ${
        variant === "full" ? "workflow-flow-map-full" : ""
      }`}
    >
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="text-xs font-semibold uppercase tracking-[0.16em] text-slate-500">
            Automation flow
          </div>
          <div className="mt-1 text-sm text-slate-300">
            Dependency stages and concurrent task lanes
          </div>
        </div>
        <div className="flex flex-wrap gap-2 tcp-text-caption">
          <span className="tcp-badge-info">{nodes.length} nodes</span>
          <span className="tcp-badge-info">{graph.edgeCount} dependencies</span>
          <span className="tcp-badge-info">
            {graph.startCount} start{graph.startCount === 1 ? "" : "s"}
          </span>
          <span className="tcp-badge-info">
            {graph.maxConcurrentTasks} concurrent / {graph.concurrencyLimit} max
          </span>
          {graph.parallelStageCount ? (
            <span className="tcp-badge-info">
              {graph.parallelStageCount} parallel stage
              {graph.parallelStageCount === 1 ? "" : "s"}
            </span>
          ) : null}
          {customMcpCount ? (
            <span className="tcp-badge-warn">{customMcpCount} task MCP overrides</span>
          ) : null}
          {graph.missingDependencyCount ? (
            <span className="tcp-badge-err">{graph.missingDependencyCount} missing deps</span>
          ) : null}
        </div>
      </div>

      <div className="overflow-x-auto pb-1">
        <div className="flex min-w-max items-stretch gap-3">
          {graph.stages.map((stage, stageIndex) => (
            <div key={`flow-column-${stage.depth}`} className="contents">
              <div
                className={`grid content-start gap-2 ${variant === "full" ? "w-[290px]" : "w-[260px]"}`}
              >
                <div className="flex min-h-7 items-center justify-between gap-2 text-xs uppercase tracking-wide text-slate-500">
                  <span>{stage.depth === 0 ? "Start" : `Stage ${stage.depth + 1}`}</span>
                  <span className="inline-flex items-center gap-1.5">
                    {stage.hasParallelTasks ? (
                      <span className="tcp-badge-info">Parallel</span>
                    ) : null}
                    <span>{stage.nodes.length}</span>
                  </span>
                </div>
                {stage.nodes.map((node, index) => {
                  const sourceIndex = nodes.indexOf(node);
                  const id = workflowFlowNodeId(node, sourceIndex >= 0 ? sourceIndex : index);
                  const active = selectedNodeId === id;
                  const deps = workflowFlowNodeDependencies(node);
                  const refs = nodeInputRefs(node);
                  const mcpServers =
                    (node?.toolAccessMode || "inherit") === "custom"
                      ? nodeMcpServers(node)
                      : workflowMcpServers;
                  const mcpTools = explicitMcpTools(node);
                  const sendCapable = mcpTools.some(toolLooksSendCapable);
                  const missingDeps = deps.filter(
                    (dep) =>
                      !nodes.some(
                        (candidate, candidateIndex) =>
                          workflowFlowNodeId(candidate, candidateIndex) === dep
                      )
                  );
                  const title = nodeTitle(node, index);
                  return (
                    <button
                      key={id}
                      type="button"
                      aria-label={`Configure ${title}`}
                      className={`tcp-list-item text-left transition ${
                        variant === "full" ? "min-h-[154px]" : "min-h-[132px]"
                      } ${active ? "border-amber-400/70 bg-amber-400/10" : ""}`}
                      onClick={() => onSelectNode?.(id)}
                    >
                      <div className="flex items-start justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate text-sm font-semibold text-slate-100">
                            {title}
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
                        {refs.length ? (
                          <span className="tcp-badge-muted">{refs.length} inputs</span>
                        ) : null}
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
              {stageIndex < graph.stages.length - 1 ? (
                <div
                  className="flex w-8 self-start items-center justify-center pt-24 text-slate-500"
                  aria-hidden="true"
                >
                  <Icon name="arrow-right" />
                </div>
              ) : null}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
