import { EmptyState, PageCard } from "./ui";
import { McpConnectionGrantPicker } from "../components/McpConnectionGrantPicker";
import { McpToolAllowlistEditor } from "../components/McpToolAllowlistEditor";
import type { StudioRole } from "../features/studio/schema";
import type { McpConnectionSummary } from "../features/mcp/mcpConnections";
import {
  ROLE_OPTIONS,
  composePromptSections,
  joinCsv,
  modelsForProvider,
  safeString,
  splitCsv,
  type ProviderOption,
  type StudioRepairState,
} from "./workflowStudioUtils";

type InspectorPanelsProps = {
  draft: any;
  selectedNode: any;
  selectedNodeInputFiles: string[];
  selectedNodeOutputFiles: string[];
  selectedNodeOutputPathPreview: any;
  selectedAgent: any;
  selectedTemplateLoadId: string;
  templateRows: Array<{ templateId: string; displayName?: string }>;
  templateMap: Map<string, any>;
  repairState: StudioRepairState | null;
  providerOptions: ProviderOption[];
  mcpServers: string[];
  mcpServerRows: Array<{ name: string; toolCache: string[] }>;
  mcpConnections: McpConnectionSummary[];
  removeSelectedNode: () => void;
  removeSelectedAgent: () => void;
  updateNode: (nodeId: string, patch: any) => void;
  updateAgent: (agentId: string, patch: any) => void;
  setSelectedAgentId: (agentId: string) => void;
  setSelectedNodeId: (nodeId: string) => void;
  setSelectedTemplateLoadId: (templateId: string) => void;
  loadTemplateIntoSelectedAgent: () => void;
};

function toolLooksSendCapable(tool: string) {
  const normalized = String(tool || "").toLowerCase();
  return (
    normalized.includes("send_email") ||
    normalized.includes("sendemail") ||
    normalized.includes("send_draft") ||
    normalized.includes("senddraft")
  );
}

function normalizeMcpNamespaceSegment(raw: string) {
  let out = "";
  let previousUnderscore = false;
  for (const ch of String(raw || "").trim()) {
    if (/^[a-z0-9]$/i.test(ch)) {
      out += ch.toLowerCase();
      previousUnderscore = false;
    } else if (!previousUnderscore) {
      out += "_";
      previousUnderscore = true;
    }
  }
  return out.replace(/^_+|_+$/g, "") || "mcp";
}

function uniqueStrings(values: string[]) {
  return Array.from(
    new Set(values.map((value) => String(value || "").trim()).filter(Boolean))
  ).sort();
}

function mcpToolBelongsToServer(toolName: string, serverName: string) {
  const prefix = `mcp.${normalizeMcpNamespaceSegment(serverName)}.`;
  return String(toolName || "").startsWith(prefix);
}

function inferMcpServersFromTools(
  tools: string[],
  serverRows: Array<{ name: string; toolCache: string[] }>
) {
  return uniqueStrings(
    serverRows
      .filter((server) => tools.some((tool) => mcpToolBelongsToServer(tool, server.name)))
      .map((server) => server.name)
  );
}

function mcpToolsForServers(
  serverRows: Array<{ name: string; toolCache: string[] }>,
  selectedServerNames: string[]
) {
  const selectedSet = new Set(selectedServerNames);
  return serverRows
    .filter((server) => selectedSet.has(server.name))
    .flatMap((server) => server.toolCache || []);
}

export function WorkflowStudioInspectorPanels(props: InspectorPanelsProps) {
  const {
    draft,
    selectedNode,
    selectedNodeInputFiles,
    selectedNodeOutputFiles,
    selectedNodeOutputPathPreview,
    selectedAgent,
    selectedTemplateLoadId,
    templateRows,
    templateMap,
    repairState,
    providerOptions,
    mcpServers,
    mcpServerRows,
    mcpConnections,
    removeSelectedNode,
    removeSelectedAgent,
    updateNode,
    updateAgent,
    setSelectedAgentId,
    setSelectedNodeId,
    setSelectedTemplateLoadId,
    loadTemplateIntoSelectedAgent,
  } = props;
  const selectedNodeToolMode = selectedNode?.toolAccessMode || "inherit";
  const selectedNodeTools =
    selectedNodeToolMode === "custom"
      ? selectedNode?.toolAllowlist || []
      : selectedAgent?.toolAllowlist || [];
  const selectedNodeMcpTools =
    selectedNodeToolMode === "custom"
      ? selectedNode?.mcpAllowedTools === null
        ? null
        : [...(selectedNode?.mcpOtherAllowedTools || []), ...(selectedNode?.mcpAllowedTools || [])]
      : selectedAgent?.mcpAllowedTools === null
        ? null
        : [
            ...(selectedAgent?.mcpOtherAllowedTools || []),
            ...(selectedAgent?.mcpAllowedTools || []),
          ];
  const selectedNodeSendCapable =
    (selectedNodeTools || []).some(toolLooksSendCapable) ||
    (selectedNodeMcpTools || []).some(toolLooksSendCapable);
  const selectedNodeMcpSummary =
    selectedNodeToolMode === "custom"
      ? selectedNode?.mcpAllowedTools === null
        ? "MCP: all selected"
        : selectedNodeMcpTools?.length
          ? `MCP: ${selectedNodeMcpTools.length} tools`
          : "MCP: none"
      : "inherits agent tools";
  const selectedNodeExplicitMcpTools = uniqueStrings([
    ...(selectedNode?.mcpOtherAllowedTools || []),
    ...(selectedNode?.mcpAllowedTools || []),
  ]);
  const selectedNodeMcpServerNames = uniqueStrings([
    ...(selectedNode?.mcpAllowedServers || []),
    ...inferMcpServersFromTools(selectedNodeExplicitMcpTools, mcpServerRows),
  ]);

  return (
    <>
      <PageCard
        title={selectedNode ? `Stage: ${selectedNode.title}` : "Stage"}
        subtitle="Edit stage behavior, dependencies, and handoff aliases."
        actions={
          <button
            className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
            onClick={removeSelectedNode}
            disabled={!selectedNode}
          >
            <i data-lucide="trash-2"></i>
            Remove Stage
          </button>
        }
      >
        {selectedNode ? (
          <div className="grid gap-3">
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Title</span>
              <input
                className="tcp-input text-sm"
                value={selectedNode.title}
                onInput={(event) => {
                  updateNode(selectedNode.nodeId, {
                    title: (event.target as HTMLInputElement).value,
                  });
                }}
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Bound Agent</span>
              <select
                className="tcp-input text-sm"
                value={selectedNode.agentId}
                onInput={(event) => {
                  const agentId = (event.target as HTMLSelectElement).value;
                  updateNode(selectedNode.nodeId, { agentId });
                  setSelectedAgentId(agentId);
                }}
              >
                {draft.agents.map((agent) => (
                  <option key={agent.agentId} value={agent.agentId}>
                    {agent.displayName || agent.agentId}
                  </option>
                ))}
              </select>
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Objective</span>
              <textarea
                className="tcp-input min-h-[110px] text-sm"
                value={selectedNode.objective}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    objective: (event.target as HTMLTextAreaElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Output Kind</span>
              <input
                className="tcp-input text-sm"
                value={selectedNode.outputKind}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    outputKind: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Required Output File</span>
              <input
                className="tcp-input text-sm"
                placeholder="marketing-brief.md"
                value={selectedNode.outputPath}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    outputPath: (event.target as HTMLInputElement).value,
                  })
                }
              />
              {selectedNodeOutputPathPreview ? (
                <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 px-3 py-2 tcp-text-caption text-slate-300">
                  <div>
                    Saved as:{" "}
                    <code>
                      {selectedNodeOutputPathPreview.canonical || selectedNodeOutputPathPreview.raw}
                    </code>
                  </div>
                  <div>
                    Next run preview: <code>{selectedNodeOutputPathPreview.resolved}</code>
                  </div>
                  {selectedNodeOutputPathPreview.warning ? (
                    <div className="text-amber-200">{selectedNodeOutputPathPreview.warning}</div>
                  ) : null}
                </div>
              ) : (
                <span className="tcp-text-caption text-slate-500">
                  Use the same runtime tokens here as the workflow output targets.
                </span>
              )}
            </label>
            <label className="grid gap-1 sm:col-span-2">
              <span className="text-xs text-slate-400">Input Files Contract</span>
              <textarea
                className="tcp-input min-h-[72px] text-sm"
                placeholder="Comma-separated relative paths this stage should read"
                value={joinCsv(selectedNode.inputFiles)}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    inputFiles: splitCsv((event.target as HTMLTextAreaElement).value),
                  })
                }
              />
              <span className="tcp-text-caption text-slate-500">
                Effective contract:{" "}
                {selectedNodeInputFiles.length
                  ? joinCsv(selectedNodeInputFiles)
                  : "No file inputs inferred from upstream stages."}
              </span>
            </label>
            <label className="grid gap-1 sm:col-span-2">
              <span className="text-xs text-slate-400">Output Files Contract</span>
              <textarea
                className="tcp-input min-h-[72px] text-sm"
                placeholder="Comma-separated relative paths this stage must create"
                value={joinCsv(selectedNode.outputFiles)}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    outputFiles: splitCsv((event.target as HTMLTextAreaElement).value),
                  })
                }
              />
              <span className="tcp-text-caption text-slate-500">
                Effective contract:{" "}
                {selectedNodeOutputFiles.length
                  ? joinCsv(selectedNodeOutputFiles)
                  : "No file outputs declared for this stage."}
              </span>
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Task Kind</span>
              <input
                className="tcp-input text-sm"
                placeholder="code_change"
                value={selectedNode.taskKind || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    taskKind: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Project Backlog Tasks</span>
              <input
                type="checkbox"
                checked={Boolean(selectedNode.projectBacklogTasks)}
                onChange={(event) =>
                  updateNode(selectedNode.nodeId, {
                    projectBacklogTasks: (event.target as HTMLInputElement).checked,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Backlog Task ID</span>
              <input
                className="tcp-input text-sm"
                placeholder="BACKLOG-123"
                value={selectedNode.backlogTaskId || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    backlogTaskId: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Repo Root</span>
              <input
                className="tcp-input text-sm"
                placeholder="."
                value={selectedNode.repoRoot || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    repoRoot: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Write Scope</span>
              <input
                className="tcp-input text-sm"
                placeholder="src/api, tests/api, Cargo.toml"
                value={selectedNode.writeScope || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    writeScope: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1 sm:col-span-2">
              <span className="text-xs text-slate-400">Acceptance Criteria</span>
              <input
                className="tcp-input text-sm"
                placeholder="Describe what must be true for this coding task to count as done."
                value={selectedNode.acceptanceCriteria || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    acceptanceCriteria: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Backlog Dependencies</span>
              <input
                className="tcp-input text-sm"
                placeholder="BACKLOG-101, BACKLOG-102"
                value={selectedNode.taskDependencies || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    taskDependencies: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Verification State</span>
              <input
                className="tcp-input text-sm"
                placeholder="pending"
                value={selectedNode.verificationState || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    verificationState: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Task Owner / Claimer</span>
              <input
                className="tcp-input text-sm"
                placeholder="implementer"
                value={selectedNode.taskOwner || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    taskOwner: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="text-xs text-slate-400">Verification Command</span>
              <input
                className="tcp-input text-sm"
                placeholder="cargo test -p tandem-server"
                value={selectedNode.verificationCommand || ""}
                onInput={(event) =>
                  updateNode(selectedNode.nodeId, {
                    verificationCommand: (event.target as HTMLInputElement).value,
                  })
                }
              />
            </label>
            <div className="grid gap-2">
              <div className="text-xs text-slate-400">Dependencies</div>
              <div className="flex flex-wrap gap-2">
                {draft.nodes
                  .filter((node) => node.nodeId !== selectedNode.nodeId)
                  .map((node) => {
                    const enabled = selectedNode.dependsOn.includes(node.nodeId);
                    return (
                      <button
                        key={`${selectedNode.nodeId}-${node.nodeId}`}
                        className={
                          enabled
                            ? "tcp-btn-primary inline-flex h-7 items-center gap-2 px-2 text-xs"
                            : "tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
                        }
                        onClick={() => {
                          const dependsOn = enabled
                            ? selectedNode.dependsOn.filter((dep) => dep !== node.nodeId)
                            : [...selectedNode.dependsOn, node.nodeId];
                          updateNode(selectedNode.nodeId, { dependsOn });
                        }}
                      >
                        <i data-lucide={enabled ? "check" : "plus"}></i>
                        {node.title}
                      </button>
                    );
                  })}
              </div>
            </div>
            {selectedNode.inputRefs.length ? (
              <div className="grid gap-2">
                <div className="text-xs text-slate-400">Input Aliases</div>
                {selectedNode.inputRefs.map((ref) => (
                  <label key={`${selectedNode.nodeId}-${ref.fromStepId}`} className="grid gap-1">
                    <span className="text-xs text-slate-500">{ref.fromStepId}</span>
                    <input
                      className="tcp-input text-sm"
                      value={ref.alias}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          inputRefs: selectedNode.inputRefs.map((entry) =>
                            entry.fromStepId === ref.fromStepId
                              ? { ...entry, alias: (event.target as HTMLInputElement).value }
                              : entry
                          ),
                        })
                      }
                    />
                  </label>
                ))}
              </div>
            ) : null}
            <details className="rounded-xl border border-slate-700/60 bg-slate-950/30 p-3">
              <summary className="flex cursor-pointer flex-wrap items-center justify-between gap-2 text-sm text-slate-100">
                <span className="inline-flex items-center gap-2 font-medium">
                  <i data-lucide="wrench"></i>
                  <span>Task Tool Access</span>
                </span>
                <span className="flex flex-wrap items-center gap-2 tcp-text-caption">
                  <span className="rounded-full border border-slate-700 px-2 py-1 text-slate-300">
                    {selectedNodeToolMode === "custom"
                      ? "custom task tools"
                      : "inherits agent tools"}
                  </span>
                  <span className="rounded-full border border-slate-700 px-2 py-1 text-slate-300">
                    {selectedNodeMcpSummary}
                  </span>
                  {selectedNodeSendCapable ? (
                    <span className="rounded-full border border-red-400/40 bg-red-500/10 px-2 py-1 text-red-200">
                      send-capable
                    </span>
                  ) : null}
                  <span className="text-slate-500">Expand to change tools for this task only.</span>
                </span>
              </summary>
              <div className="mt-3 grid gap-3">
                <div className="grid gap-2 md:grid-cols-2">
                  <button
                    type="button"
                    className={
                      selectedNodeToolMode === "inherit"
                        ? "tcp-btn-primary justify-start"
                        : "tcp-btn justify-start"
                    }
                    onClick={() =>
                      updateNode(selectedNode.nodeId, {
                        toolAccessMode: "inherit",
                        toolAllowlist: [],
                        toolDenylist: [],
                        mcpAllowedServers: [],
                        mcpAllowedTools: null,
                        mcpOtherAllowedTools: [],
                        mcpAllowedConnections: [],
                      })
                    }
                  >
                    Inherit agent tools
                  </button>
                  <button
                    type="button"
                    className={
                      selectedNodeToolMode === "custom"
                        ? "tcp-btn-primary justify-start"
                        : "tcp-btn justify-start"
                    }
                    onClick={() =>
                      updateNode(selectedNode.nodeId, {
                        toolAccessMode: "custom",
                        toolAllowlist: selectedNode.toolAllowlist?.length
                          ? selectedNode.toolAllowlist
                          : selectedAgent?.toolAllowlist || ["read"],
                        toolDenylist: selectedNode.toolDenylist || [],
                        mcpAllowedServers: selectedNode.mcpAllowedServers?.length
                          ? selectedNode.mcpAllowedServers
                          : selectedAgent?.mcpAllowedServers || [],
                        mcpAllowedTools:
                          selectedNode.mcpAllowedTools === undefined
                            ? selectedAgent?.mcpAllowedTools || null
                            : selectedNode.mcpAllowedTools,
                        mcpOtherAllowedTools:
                          selectedNode.mcpOtherAllowedTools ||
                          selectedAgent?.mcpOtherAllowedTools ||
                          [],
                        mcpAllowedConnections: selectedNode.mcpAllowedConnections?.length
                          ? selectedNode.mcpAllowedConnections
                          : selectedAgent?.mcpAllowedConnections || [],
                      })
                    }
                  >
                    Customize this task
                  </button>
                </div>
                {selectedNodeToolMode === "custom" ? (
                  <>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Task Tool Allowlist</span>
                      <input
                        className="tcp-input text-sm"
                        value={joinCsv(selectedNode.toolAllowlist || [])}
                        onInput={(event) =>
                          updateNode(selectedNode.nodeId, {
                            toolAllowlist: splitCsv((event.target as HTMLInputElement).value),
                          })
                        }
                      />
                    </label>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Task Tool Denylist</span>
                      <input
                        className="tcp-input text-sm"
                        value={joinCsv(selectedNode.toolDenylist || [])}
                        onInput={(event) =>
                          updateNode(selectedNode.nodeId, {
                            toolDenylist: splitCsv((event.target as HTMLInputElement).value),
                          })
                        }
                      />
                    </label>
                    <div className="grid gap-2">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <span className="text-xs text-slate-400">Task MCP Servers</span>
                        <span className="tcp-text-caption text-slate-500">
                          Select runtime servers to reveal all available task tools.
                        </span>
                      </div>
                      {mcpServerRows.length ? (
                        <div className="flex flex-wrap gap-2">
                          {mcpServerRows.map((server) => {
                            const selected = selectedNodeMcpServerNames.includes(server.name);
                            return (
                              <button
                                key={server.name}
                                type="button"
                                className={
                                  selected
                                    ? "tcp-btn-primary h-7 px-2 text-xs"
                                    : "tcp-btn h-7 px-2 text-xs"
                                }
                                onClick={() => {
                                  const currentServers = uniqueStrings([
                                    ...(selectedNode.mcpAllowedServers || []),
                                    ...inferMcpServersFromTools(
                                      selectedNodeExplicitMcpTools,
                                      mcpServerRows
                                    ),
                                  ]);
                                  const nextServers = selected
                                    ? currentServers.filter((name) => name !== server.name)
                                    : uniqueStrings([...currentServers, server.name]);
                                  const nextAllowedTools = Array.isArray(
                                    selectedNode.mcpAllowedTools
                                  )
                                    ? selectedNode.mcpAllowedTools.filter(
                                        (toolName: string) =>
                                          !selected ||
                                          !mcpToolBelongsToServer(toolName, server.name)
                                      )
                                    : selectedNode.mcpAllowedTools;
                                  const nextAllowedConnections = selected
                                    ? (selectedNode.mcpAllowedConnections || []).filter(
                                        (grant: any) => grant?.server !== server.name
                                      )
                                    : selectedNode.mcpAllowedConnections || [];
                                  updateNode(selectedNode.nodeId, {
                                    mcpAllowedServers: nextServers,
                                    mcpAllowedTools: nextAllowedTools,
                                    mcpAllowedConnections: nextAllowedConnections,
                                  });
                                }}
                              >
                                {server.name}
                              </button>
                            );
                          })}
                        </div>
                      ) : (
                        <div className="text-xs text-slate-500">
                          No MCP servers are currently visible to the runtime.
                        </div>
                      )}
                    </div>
                    <McpConnectionGrantPicker
                      title="Task MCP acting connections"
                      subtitle="Choose the account, shared connection, or service principal this task may use."
                      connections={mcpConnections}
                      value={selectedNode.mcpAllowedConnections || []}
                      selectedServers={selectedNodeMcpServerNames}
                      onSelectedServersChange={(nextServers) =>
                        updateNode(selectedNode.nodeId, { mcpAllowedServers: nextServers })
                      }
                      onChange={(nextConnections) =>
                        updateNode(selectedNode.nodeId, {
                          mcpAllowedConnections: nextConnections,
                          mcpAllowedServers: uniqueStrings([
                            ...(selectedNode.mcpAllowedServers || []),
                            ...nextConnections.map((grant) => grant.server),
                          ]),
                        })
                      }
                    />
                    <McpToolAllowlistEditor
                      title="Task MCP tool access"
                      subtitle="This list applies only to the selected task. Unchecked tools stay visible so they can be restored."
                      discoveredTools={mcpToolsForServers(
                        mcpServerRows,
                        selectedNodeMcpServerNames
                      )}
                      value={selectedNode.mcpAllowedTools ?? null}
                      onChange={(next) =>
                        updateNode(selectedNode.nodeId, { mcpAllowedTools: next })
                      }
                      collapsible
                      defaultCollapsed
                    />
                  </>
                ) : null}
              </div>
            </details>
          </div>
        ) : (
          <EmptyState text="Select a stage to edit it." />
        )}
      </PageCard>

      <PageCard
        title={
          selectedAgent ? `Agent: ${selectedAgent.displayName || selectedAgent.agentId}` : "Agent"
        }
        subtitle="Role prompt, policies, reusable template link, and model settings."
        actions={
          <button
            className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
            onClick={removeSelectedAgent}
            disabled={!selectedAgent}
          >
            <i data-lucide="trash-2"></i>
            Remove Agent
          </button>
        }
      >
        {selectedAgent ? (
          <div className="grid gap-3">
            <div className="grid gap-3 md:grid-cols-2">
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Display Name</span>
                <input
                  className="tcp-input text-sm"
                  value={selectedAgent.displayName}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      displayName: (event.target as HTMLInputElement).value,
                    })
                  }
                />
              </label>
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Role</span>
                <select
                  className="tcp-input text-sm"
                  value={selectedAgent.role}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      role: (event.target as HTMLSelectElement).value as StudioRole,
                    })
                  }
                >
                  {ROLE_OPTIONS.map((role) => (
                    <option key={role} value={role}>
                      {role}
                    </option>
                  ))}
                </select>
              </label>
              <label className="grid gap-1 md:col-span-2">
                <span className="text-xs text-slate-400">Skills</span>
                <input
                  className="tcp-input text-sm"
                  value={joinCsv(selectedAgent.skills)}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      skills: splitCsv((event.target as HTMLInputElement).value),
                    })
                  }
                  placeholder="copywriting, websearch, qa"
                />
              </label>
            </div>

            <div className="rounded-xl border border-slate-700/60 bg-slate-950/30 p-3">
              <div className="mb-2 flex items-center justify-between gap-2">
                <div className="text-xs uppercase tracking-wide text-slate-500">Template Link</div>
                {selectedAgent.linkedTemplateId ? (
                  <button
                    className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
                    onClick={() =>
                      updateAgent(selectedAgent.agentId, {
                        linkedTemplateId: "",
                        templateId: "",
                      })
                    }
                  >
                    <i data-lucide="unlink"></i>
                    Detach
                  </button>
                ) : null}
              </div>
              <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto]">
                <select
                  className="tcp-input text-sm"
                  value={selectedTemplateLoadId}
                  onInput={(event) =>
                    setSelectedTemplateLoadId((event.target as HTMLSelectElement).value)
                  }
                >
                  <option value="">Select an existing agent template...</option>
                  {templateRows.map((template) => (
                    <option key={template.templateId} value={template.templateId}>
                      {template.displayName || template.templateId}
                    </option>
                  ))}
                </select>
                <button
                  className="tcp-btn inline-flex h-10 items-center gap-2 px-3 text-sm"
                  disabled={!selectedTemplateLoadId}
                  onClick={loadTemplateIntoSelectedAgent}
                >
                  <i data-lucide="download"></i>
                  Load Template
                </button>
              </div>
              <div className="mt-2 text-xs text-slate-400">
                {repairState?.repairedAgentIds.includes(selectedAgent.agentId)
                  ? "This agent had a missing shared template link. Studio repaired it into a workflow-local prompt."
                  : selectedAgent.linkedTemplateId
                    ? templateMap.has(selectedAgent.linkedTemplateId)
                      ? `Linked template: ${selectedAgent.linkedTemplateId}`
                      : `Missing template link repaired locally: ${selectedAgent.linkedTemplateId}`
                    : "This agent is currently workflow-local unless you save reusable templates."}
              </div>
              <div className="mt-1 text-xs text-slate-500">
                Local means Studio stores the prompt in workflow metadata. Linked means runtime
                depends on a shared Agent Team template.
              </div>
            </div>

            <div className="grid gap-3 md:grid-cols-2">
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Model Provider</span>
                <select
                  className="tcp-input text-sm"
                  value={selectedAgent.modelProvider}
                  disabled={draft.useSharedModel}
                  onInput={(event) =>
                    updateAgent(
                      selectedAgent.agentId,
                      (() => {
                        const provider = (event.target as HTMLSelectElement).value;
                        const models = modelsForProvider(providerOptions, provider);
                        return {
                          modelProvider: provider,
                          modelId: models.includes(selectedAgent.modelId)
                            ? selectedAgent.modelId
                            : models[0] || selectedAgent.modelId,
                        };
                      })()
                    )
                  }
                >
                  <option value="">Select provider...</option>
                  {providerOptions.map((provider) => (
                    <option key={provider.id} value={provider.id}>
                      {provider.id}
                    </option>
                  ))}
                </select>
              </label>
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Model ID</span>
                {modelsForProvider(providerOptions, selectedAgent.modelProvider).length ? (
                  <select
                    className="tcp-input text-sm"
                    value={selectedAgent.modelId}
                    disabled={draft.useSharedModel}
                    onInput={(event) =>
                      updateAgent(selectedAgent.agentId, {
                        modelId: (event.target as HTMLSelectElement).value,
                      })
                    }
                  >
                    {modelsForProvider(providerOptions, selectedAgent.modelProvider).map(
                      (model) => (
                        <option key={model} value={model}>
                          {model}
                        </option>
                      )
                    )}
                  </select>
                ) : (
                  <input
                    className="tcp-input text-sm"
                    value={selectedAgent.modelId}
                    disabled={draft.useSharedModel}
                    onInput={(event) =>
                      updateAgent(selectedAgent.agentId, {
                        modelId: (event.target as HTMLInputElement).value,
                      })
                    }
                    placeholder="provider-specific model id"
                  />
                )}
              </label>
              {draft.useSharedModel ? (
                <div className="text-xs text-amber-200 md:col-span-2">
                  Per-agent model controls are locked because this workflow is using one shared
                  model for all agents.
                </div>
              ) : null}
              <label className="grid gap-1 md:col-span-2">
                <span className="text-xs text-slate-400">Tool Allowlist</span>
                <input
                  className="tcp-input text-sm"
                  value={joinCsv(selectedAgent.toolAllowlist)}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      toolAllowlist: splitCsv((event.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>
              <label className="grid gap-1 md:col-span-2">
                <span className="text-xs text-slate-400">Tool Denylist</span>
                <input
                  className="tcp-input text-sm"
                  value={joinCsv(selectedAgent.toolDenylist)}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      toolDenylist: splitCsv((event.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>
              <label className="grid gap-1 md:col-span-2">
                <span className="text-xs text-slate-400">Allowed MCP Servers</span>
                <input
                  className="tcp-input text-sm"
                  value={joinCsv(selectedAgent.mcpAllowedServers)}
                  onInput={(event) =>
                    updateAgent(
                      selectedAgent.agentId,
                      (() => {
                        const mcpAllowedServers = splitCsv(
                          (event.target as HTMLInputElement).value
                        );
                        return {
                          mcpAllowedServers,
                          mcpAllowedConnections: (selectedAgent.mcpAllowedConnections || []).filter(
                            (grant: any) => mcpAllowedServers.includes(grant?.server)
                          ),
                        };
                      })()
                    )
                  }
                  placeholder={joinCsv(mcpServers) || "No MCP servers detected"}
                />
              </label>
              <div className="md:col-span-2">
                <McpConnectionGrantPicker
                  title="Agent MCP acting connections"
                  subtitle="Choose explicit accounts, shared connections, or service principals for this agent."
                  connections={mcpConnections}
                  value={selectedAgent.mcpAllowedConnections || []}
                  selectedServers={selectedAgent.mcpAllowedServers || []}
                  onSelectedServersChange={(nextServers) =>
                    updateAgent(selectedAgent.agentId, { mcpAllowedServers: nextServers })
                  }
                  onChange={(nextConnections) =>
                    updateAgent(selectedAgent.agentId, {
                      mcpAllowedConnections: nextConnections,
                      mcpAllowedServers: uniqueStrings([
                        ...(selectedAgent.mcpAllowedServers || []),
                        ...nextConnections.map((grant) => grant.server),
                      ]),
                    })
                  }
                />
              </div>
              {selectedAgent.mcpAllowedServers.length ? (
                <div className="md:col-span-2">
                  <McpToolAllowlistEditor
                    title="Agent MCP tool access"
                    subtitle="Leave all discovered tools selected to inherit full access from the chosen MCP servers, or uncheck tools to save an exact MCP allowlist for this agent."
                    discoveredTools={mcpServerRows
                      .filter((server) => selectedAgent.mcpAllowedServers.includes(server.name))
                      .flatMap((server) => server.toolCache)}
                    value={selectedAgent.mcpAllowedTools}
                    onChange={(next) =>
                      updateAgent(selectedAgent.agentId, {
                        mcpAllowedTools: next,
                      })
                    }
                  />
                </div>
              ) : null}
            </div>

            <div className="grid gap-3">
              <div className="text-xs uppercase tracking-wide text-slate-500">Role Prompt</div>
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Role</span>
                <textarea
                  className="tcp-input min-h-[72px] text-sm"
                  value={selectedAgent.prompt.role}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      prompt: {
                        ...selectedAgent.prompt,
                        role: (event.target as HTMLTextAreaElement).value,
                      },
                    })
                  }
                />
              </label>
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Mission</span>
                <textarea
                  className="tcp-input min-h-[92px] text-sm"
                  value={selectedAgent.prompt.mission}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      prompt: {
                        ...selectedAgent.prompt,
                        mission: (event.target as HTMLTextAreaElement).value,
                      },
                    })
                  }
                />
              </label>
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Inputs</span>
                <textarea
                  className="tcp-input min-h-[72px] text-sm"
                  value={selectedAgent.prompt.inputs}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      prompt: {
                        ...selectedAgent.prompt,
                        inputs: (event.target as HTMLTextAreaElement).value,
                      },
                    })
                  }
                />
              </label>
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Output Contract</span>
                <textarea
                  className="tcp-input min-h-[72px] text-sm"
                  value={selectedAgent.prompt.outputContract}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      prompt: {
                        ...selectedAgent.prompt,
                        outputContract: (event.target as HTMLTextAreaElement).value,
                      },
                    })
                  }
                />
              </label>
              <label className="grid gap-1">
                <span className="text-xs text-slate-400">Guardrails</span>
                <textarea
                  className="tcp-input min-h-[72px] text-sm"
                  value={selectedAgent.prompt.guardrails}
                  onInput={(event) =>
                    updateAgent(selectedAgent.agentId, {
                      prompt: {
                        ...selectedAgent.prompt,
                        guardrails: (event.target as HTMLTextAreaElement).value,
                      },
                    })
                  }
                />
              </label>
            </div>

            <div className="rounded-xl border border-slate-700/60 bg-slate-950/40 p-3">
              <div className="mb-2 text-xs uppercase tracking-wide text-slate-500">
                Composed System Prompt
              </div>
              <pre className="whitespace-pre-wrap break-words text-xs text-slate-200">
                {composePromptSections(selectedAgent.prompt) || "Prompt preview will appear here."}
              </pre>
            </div>
          </div>
        ) : (
          <EmptyState text="Select or add an agent to edit it." />
        )}
      </PageCard>
    </>
  );
}
