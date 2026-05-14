import { useEffect, useRef, useState } from "preact/hooks";
import { motion } from "motion/react";
import { McpToolAllowlistEditor } from "../../components/McpToolAllowlistEditor";
import { ProviderModelSelector } from "../../components/ProviderModelSelector";
import { renderIcons } from "../../app/icons.js";
import { ScheduleBuilder } from "./ScheduleBuilder";
import { TimezoneField } from "./TimezoneField";
import { ScopeInspector } from "./ScopeInspector";
import { WatchConditionEditor } from "./WatchConditionEditor";
import { ScopePolicyEditor } from "./ScopePolicyEditor";
import { HandoffConfigEditor } from "./HandoffConfigEditor";
import { HandoffPanel } from "./HandoffPanel";
import { ExecutionProfileToggle } from "./ExecutionProfileToggle";

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

function mcpServerToolCache(server: any): string[] {
  return Array.isArray(server?.toolCache)
    ? server.toolCache.map((tool: any) => String(tool || "").trim()).filter(Boolean)
    : [];
}

function mcpToolBelongsToServer(toolName: string, serverName: string) {
  const prefix = `mcp.${normalizeMcpNamespaceSegment(serverName)}.`;
  return String(toolName || "").startsWith(prefix);
}

function inferMcpServersFromTools(tools: string[], servers: any[]) {
  return uniqueStrings(
    servers
      .filter((server) => tools.some((tool) => mcpToolBelongsToServer(tool, server.name)))
      .map((server) => server.name)
  );
}

function mcpToolsForServers(servers: any[], selectedServerNames: string[]) {
  const selectedSet = new Set(selectedServerNames);
  return servers
    .filter((server) => selectedSet.has(server.name))
    .flatMap((server) => mcpServerToolCache(server));
}

function useDialogIconRender(active: boolean) {
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!active) return;
    if (rootRef.current) renderIcons(rootRef.current);
  }, [active]);

  return rootRef;
}

function safeWorkflowExportName(raw: string) {
  const cleaned = String(raw || "workflow-automation")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return cleaned || "workflow-automation";
}

function downloadWorkflowRecoveryBundle(workflowEditDraft: any) {
  const automationId = String(workflowEditDraft?.automationId || "").trim();
  const bundle = {
    exported_at: new Date().toISOString(),
    export_kind: "tandem_workflow_recovery_bundle",
    export_version: 1,
    automation_id: automationId,
    name: String(workflowEditDraft?.name || automationId || "Workflow automation").trim(),
    description: String(workflowEditDraft?.description || "").trim(),
    recovery_prompt: String(workflowEditDraft?.recoveryPrompt || "").trim(),
    source_automation: workflowEditDraft?.sourceAutomation || null,
    editable_snapshot: {
      schedule_kind: workflowEditDraft?.scheduleKind || "",
      workspace_root: workflowEditDraft?.workspaceRoot || "",
      execution_mode: workflowEditDraft?.executionMode || "",
      selected_mcp_servers: workflowEditDraft?.selectedMcpServers || [],
      selected_mcp_tools: workflowEditDraft?.selectedMcpTools || null,
      nodes: workflowEditDraft?.nodes || [],
    },
  };
  const blob = new Blob([JSON.stringify(bundle, null, 2)], {
    type: "application/json;charset=utf-8",
  });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `${safeWorkflowExportName(automationId || workflowEditDraft?.name)}-recovery.json`;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

function toolLooksSendCapable(tool: string) {
  const value = String(tool || "").toLowerCase();
  return /\bsend\b/.test(value) || value.includes("_send") || value.includes("send_");
}

export function LegacyAutomationEditDialog({
  editDraft,
  setEditDraft,
  updateAutomationMutation,
}: any) {
  const dialogRef = useDialogIconRender(!!editDraft);
  if (!editDraft) return null;

  return (
    <motion.div
      ref={dialogRef}
      className="tcp-confirm-overlay"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={() => setEditDraft(null)}
    >
      <motion.div
        className="tcp-confirm-dialog w-[min(40rem,96vw)]"
        initial={{ opacity: 0, y: 8, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 6, scale: 0.98 }}
        onClick={(event) => event.stopPropagation()}
      >
        <h3 className="tcp-confirm-title">Edit automation</h3>
        <div className="grid gap-3">
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Name</label>
            <input
              className="tcp-input"
              value={editDraft.name}
              onInput={(e) =>
                setEditDraft((current: any) =>
                  current ? { ...current, name: (e.target as HTMLInputElement).value } : current
                )
              }
            />
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Objective</label>
            <textarea
              className="tcp-input min-h-[96px]"
              value={editDraft.objective}
              onInput={(e) =>
                setEditDraft((current: any) =>
                  current
                    ? { ...current, objective: (e.target as HTMLTextAreaElement).value }
                    : current
                )
              }
            />
          </div>
          <div className="grid gap-1 sm:grid-cols-2 sm:gap-2">
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Mode</label>
              <select
                className="tcp-input"
                value={editDraft.mode}
                onInput={(e) =>
                  setEditDraft((current: any) =>
                    current
                      ? {
                          ...current,
                          mode: (e.target as HTMLSelectElement).value as
                            | "standalone"
                            | "orchestrated",
                        }
                      : current
                  )
                }
              >
                <option value="standalone">standalone</option>
                <option value="orchestrated">orchestrated</option>
              </select>
            </div>
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Approval policy</label>
              <button
                className={`tcp-input flex h-10 items-center justify-between px-3 text-xs ${
                  editDraft.requiresApproval ? "border-amber-400/60 bg-amber-400/10" : ""
                }`}
                role="switch"
                aria-checked={editDraft.requiresApproval}
                onClick={() =>
                  setEditDraft((current: any) =>
                    current ? { ...current, requiresApproval: !current.requiresApproval } : current
                  )
                }
              >
                <span className="flex items-center gap-2">
                  <i data-lucide={editDraft.requiresApproval ? "shield-alert" : "shield-check"}></i>
                  {editDraft.requiresApproval
                    ? "Manual approvals enabled"
                    : "Fully automated enabled"}
                </span>
                <span
                  className={`relative h-5 w-9 rounded-full transition ${
                    editDraft.requiresApproval ? "bg-amber-500/40" : "bg-emerald-500/30"
                  }`}
                >
                  <span
                    className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-slate-100 transition ${
                      editDraft.requiresApproval ? "" : "translate-x-4"
                    }`}
                  />
                </span>
              </button>
            </div>
          </div>
          <div className="grid gap-1 sm:grid-cols-2 sm:gap-2">
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Schedule type</label>
              <select
                className="tcp-input"
                value={editDraft.scheduleKind}
                onInput={(e) =>
                  setEditDraft((current: any) =>
                    current
                      ? {
                          ...current,
                          scheduleKind: (e.target as HTMLSelectElement).value as
                            | "cron"
                            | "interval",
                        }
                      : current
                  )
                }
              >
                <option value="interval">interval</option>
                <option value="cron">cron</option>
              </select>
            </div>
          </div>
          {editDraft.scheduleKind === "cron" ? (
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Cron expression</label>
              <input
                className="tcp-input font-mono"
                value={editDraft.cronExpression}
                onInput={(e) =>
                  setEditDraft((current: any) =>
                    current
                      ? { ...current, cronExpression: (e.target as HTMLInputElement).value }
                      : current
                  )
                }
                placeholder="0 9 * * *"
              />
            </div>
          ) : (
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Interval seconds</label>
              <input
                type="number"
                min="1"
                className="tcp-input"
                value={editDraft.intervalSeconds}
                onInput={(e) =>
                  setEditDraft((current: any) =>
                    current
                      ? { ...current, intervalSeconds: (e.target as HTMLInputElement).value }
                      : current
                  )
                }
              />
            </div>
          )}
        </div>
        <div className="tcp-confirm-actions mt-3">
          <button className="tcp-btn" onClick={() => setEditDraft(null)}>
            <i data-lucide="x-circle"></i>
            Cancel
          </button>
          <button
            className="tcp-btn-primary"
            onClick={() => editDraft && updateAutomationMutation.mutate(editDraft)}
            disabled={updateAutomationMutation.isPending}
          >
            <i data-lucide="check"></i>
            Save
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}

export function AccordionSection({
  title,
  defaultOpen = false,
  children,
  description = "",
  headerStyle = "",
  icon = "",
}: {
  title: string;
  defaultOpen?: boolean;
  children: any;
  description?: string;
  headerStyle?: string;
  icon?: string;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div
      className={`rounded-xl border border-slate-700/50 bg-slate-900/30 overflow-hidden ${headerStyle === "violet" ? "border-violet-500/20 bg-violet-900/5" : ""}`}
    >
      <button
        type="button"
        className="w-full flex items-center justify-between px-4 py-3 text-left hover:bg-slate-800/40 transition-colors focus:outline-none"
        onClick={() => setOpen(!open)}
      >
        <div className="flex items-center gap-2">
          {icon && (
            <i
              data-lucide={icon}
              className={`h-4 w-4 ${headerStyle === "violet" ? "text-violet-400" : "text-slate-400"}`}
            ></i>
          )}
          <div>
            <div
              className={`text-xs uppercase tracking-wide font-medium ${headerStyle === "violet" ? "text-violet-300" : "text-slate-500"}`}
            >
              {title}
            </div>
            {description && open && (
              <div
                className={`text-xs mt-0.5 ${headerStyle === "violet" ? "text-slate-500" : "text-slate-400"}`}
              >
                {description}
              </div>
            )}
          </div>
        </div>
        <i
          data-lucide={open ? "chevron-up" : "chevron-down"}
          className="h-4 w-4 text-slate-500 shrink-0 ml-4"
        ></i>
      </button>
      {open && (
        <div
          className={`px-4 pb-4 border-t ${headerStyle === "violet" ? "border-violet-900/40" : "border-slate-800/60"} pt-3`}
        >
          <div className="grid gap-3">{children}</div>
        </div>
      )}
    </div>
  );
}

export function WorkflowAutomationEditDialog({
  workflowEditDraft,
  setWorkflowEditDraft,
  validateWorkspaceRootInput,
  validateModelInput,
  validatePlannerModelInput,
  automationWizardConfig,
  providerOptions,
  mcpServers,
  overlapHistoryEntries,
  runNowV2Mutation,
  updateWorkflowAutomationMutation,
  automationsV2List = [],
  client,
  onRecreateWorkflowAutomation,
}: any) {
  const dialogRef = useDialogIconRender(!!workflowEditDraft);
  if (!workflowEditDraft) return null;

  const selectedExecutionMode =
    automationWizardConfig.executionModes.find(
      (mode: any) => mode.id === workflowEditDraft.executionMode
    ) || automationWizardConfig.executionModes[0];
  const executionModeNotes: Record<string, string> = {
    single: "One focused operator handles the full workflow from start to finish.",
    team: "A planner coordinates a small set of specialized agents. This is best when the work has multiple steps but still needs tight sequencing and review.",
    swarm:
      "Tandem fans work out into parallel sub-tasks. Use this when breadth and throughput matter more than one tightly coordinated thread.",
  };

  return (
    <motion.div
      ref={dialogRef}
      className="tcp-confirm-overlay"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={() => setWorkflowEditDraft(null)}
    >
      <motion.div
        className="tcp-confirm-dialog tcp-workflow-editor-modal"
        initial={{ opacity: 0, y: 8, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 6, scale: 0.98 }}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-3 border-b border-slate-800/70 px-4 py-4">
          <div>
            <h3 className="tcp-confirm-title">Edit workflow automation</h3>
            <div className="mt-1 text-sm text-slate-400">
              Update scheduling, model routing, MCP access, and the actual step prompts.
            </div>
          </div>
          <button className="tcp-btn h-9 w-9 px-0" onClick={() => setWorkflowEditDraft(null)}>
            <i data-lucide="x"></i>
          </button>
        </div>
        <div className="flex flex-1 flex-col gap-4 overflow-y-auto px-4 py-4">
          <div className="grid content-start gap-4 min-w-0">
            <AccordionSection title="General setup" defaultOpen={true}>
              <div className="grid gap-1">
                <label className="text-xs text-slate-400">Automation name</label>
                <input
                  className="tcp-input"
                  value={workflowEditDraft.name}
                  onInput={(e) =>
                    setWorkflowEditDraft((current: any) =>
                      current ? { ...current, name: (e.target as HTMLInputElement).value } : current
                    )
                  }
                />
              </div>
              <div className="grid gap-1">
                <label className="text-xs text-slate-400">Notes / description</label>
                <textarea
                  className="tcp-input min-h-[120px]"
                  value={workflowEditDraft.description}
                  onInput={(e) =>
                    setWorkflowEditDraft((current: any) =>
                      current
                        ? {
                            ...current,
                            description: (e.target as HTMLTextAreaElement).value,
                          }
                        : current
                    )
                  }
                  placeholder="Add notes, delivery expectations, or operator guidance."
                />
              </div>
              <div className="grid gap-1">
                <label className="text-xs text-slate-400">Workspace root</label>
                <input
                  className={`tcp-input ${
                    validateWorkspaceRootInput(workflowEditDraft.workspaceRoot)
                      ? "border-red-500/70 text-red-100"
                      : ""
                  }`}
                  value={workflowEditDraft.workspaceRoot}
                  onInput={(e) =>
                    setWorkflowEditDraft((current: any) =>
                      current
                        ? { ...current, workspaceRoot: (e.target as HTMLInputElement).value }
                        : current
                    )
                  }
                />
                {validateWorkspaceRootInput(workflowEditDraft.workspaceRoot) ? (
                  <div className="text-xs text-red-300">
                    {validateWorkspaceRootInput(workflowEditDraft.workspaceRoot)}
                  </div>
                ) : null}
              </div>
            </AccordionSection>

            <AccordionSection
              title="Recovery"
              description="Export the compiled workflow and preserved planning metadata, or seed a clean workflow generation from the original prompt."
              icon="archive"
            >
              <div className="rounded-lg border border-slate-800/70 bg-slate-950/30 p-3">
                <div className="text-xs uppercase tracking-[0.16em] text-slate-500">
                  Original prompt
                </div>
                {String(workflowEditDraft.recoveryPrompt || "").trim() ? (
                  <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap rounded-md border border-slate-800/70 bg-black/20 p-3 text-xs leading-5 text-slate-300">
                    {String(workflowEditDraft.recoveryPrompt || "").trim()}
                  </pre>
                ) : (
                  <div className="mt-2 text-xs text-amber-200">
                    No original workflow prompt was found in this automation metadata. Export still
                    includes the compiled automation for manual recovery.
                  </div>
                )}
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  className="tcp-btn h-9 px-3 text-sm"
                  onClick={() => downloadWorkflowRecoveryBundle(workflowEditDraft)}
                >
                  <i data-lucide="download"></i>
                  Export recovery JSON
                </button>
                <button
                  type="button"
                  className="tcp-btn h-9 px-3 text-sm"
                  onClick={() =>
                    onRecreateWorkflowAutomation?.({
                      automationId: workflowEditDraft.automationId,
                      prompt: workflowEditDraft.recoveryPrompt,
                    })
                  }
                  disabled={!String(workflowEditDraft.recoveryPrompt || "").trim()}
                >
                  <i data-lucide="refresh-cw"></i>
                  Re-create from prompt
                </button>
              </div>
              <div className="text-xs text-slate-500">
                Re-create does not mutate this automation. It opens the current workflow generator
                with the recovered prompt so upgrades to the compiler/runtime can produce a fresh
                workflow.
              </div>
            </AccordionSection>

            <AccordionSection title="Execution" defaultOpen={true}>
              <div className="grid gap-3">
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Execution profile</label>
                  <ExecutionProfileToggle
                    value={workflowEditDraft.executionProfile || ""}
                    clearable
                    onChange={(next) =>
                      setWorkflowEditDraft((current: any) =>
                        current ? { ...current, executionProfile: next } : current
                      )
                    }
                  />
                  <div className="text-xs text-slate-500">
                    {workflowEditDraft.executionProfile === "yolo"
                      ? "Non-critical validation failures continue as experimental; spend caps and approvals still enforced."
                      : workflowEditDraft.executionProfile === "guided"
                        ? "Non-critical validation failures become warnings; critical failures still block."
                        : workflowEditDraft.executionProfile === "strict"
                          ? "All validators enforced."
                          : "System default is selected. Guided is the fallback when no tenant default is set; use Lenient when recoverable checks are still blocking runs."}
                  </div>
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Schedule</label>
                  <ScheduleBuilder
                    value={{
                      scheduleKind: workflowEditDraft.scheduleKind,
                      cronExpression: workflowEditDraft.cronExpression,
                      intervalSeconds: workflowEditDraft.intervalSeconds,
                    }}
                    timezone={workflowEditDraft.timezone}
                    onChange={(value) =>
                      setWorkflowEditDraft((current: any) =>
                        current
                          ? {
                              ...current,
                              scheduleKind: value.scheduleKind,
                              cronExpression: value.cronExpression,
                              intervalSeconds: value.intervalSeconds,
                            }
                          : current
                      )
                    }
                  />
                </div>
                <TimezoneField
                  value={workflowEditDraft.timezone}
                  onChange={(value) =>
                    setWorkflowEditDraft((current: any) =>
                      current ? { ...current, timezone: value } : current
                    )
                  }
                  hint="Use the timezone that matches when this workflow should fire."
                />
              </div>
              <div className="grid gap-2">
                <label className="text-xs text-slate-400">Agent type</label>
                <div className="grid gap-2">
                  {automationWizardConfig.executionModes.map((mode: any) => {
                    const selected = workflowEditDraft.executionMode === mode.id;
                    return (
                      <button
                        key={mode.id}
                        type="button"
                        className={`tcp-list-item text-left ${
                          selected ? "border-amber-400/60 bg-amber-400/10" : ""
                        }`}
                        onClick={() =>
                          setWorkflowEditDraft((current: any) =>
                            current ? { ...current, executionMode: mode.id } : current
                          )
                        }
                      >
                        <div className="flex items-start justify-between gap-3">
                          <div className="flex items-start gap-3">
                            <div className="text-xl leading-none">{mode.icon}</div>
                            <div className="grid gap-1">
                              <div className="flex flex-wrap items-center gap-2">
                                <span className="text-sm font-semibold text-slate-100">
                                  {mode.label}
                                </span>
                                {mode.id === "team" ? (
                                  <span className="rounded-full border border-amber-400/40 bg-amber-400/10 px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.16em] text-amber-200">
                                    Recommended
                                  </span>
                                ) : null}
                              </div>
                              <div className="text-sm text-slate-300">{mode.desc}</div>
                              <div className="text-xs text-slate-500">{mode.bestFor}</div>
                            </div>
                          </div>
                          <span
                            className={`mt-1 h-3 w-3 rounded-full border ${
                              selected
                                ? "border-amber-300 bg-amber-300 shadow-[0_0_0_3px_rgba(251,191,36,0.12)]"
                                : "border-slate-600 bg-transparent"
                            }`}
                            aria-hidden="true"
                          />
                        </div>
                      </button>
                    );
                  })}
                </div>
                <div className="rounded-lg border border-slate-800/70 bg-slate-950/30 px-3 py-3 text-xs text-slate-400">
                  <div className="font-medium uppercase tracking-[0.16em] text-slate-500">
                    Execution behavior
                  </div>
                  <div className="mt-1 text-sm text-slate-300">{selectedExecutionMode?.label}</div>
                  <div className="mt-1">
                    {executionModeNotes[selectedExecutionMode?.id || "single"] ||
                      executionModeNotes.single}
                  </div>
                </div>
              </div>
              <div className="grid gap-2 sm:grid-cols-2">
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Execution mode key</label>
                  <div className="tcp-input flex h-10 items-center text-sm text-slate-300">
                    {workflowEditDraft.executionMode}
                  </div>
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Max parallel agents</label>
                  <input
                    type="number"
                    min="2"
                    max="16"
                    className="tcp-input"
                    value={workflowEditDraft.maxParallelAgents}
                    onInput={(e) =>
                      setWorkflowEditDraft((current: any) =>
                        current
                          ? {
                              ...current,
                              maxParallelAgents: (e.target as HTMLInputElement).value,
                            }
                          : current
                      )
                    }
                    disabled={workflowEditDraft.executionMode === "single"}
                  />
                  <div className="text-xs text-slate-500">
                    {workflowEditDraft.executionMode === "single"
                      ? "Single Agent ignores this value because it stays on one coordinated thread."
                      : "Team and swarm runs use this value as the concurrency cap for parallel sub-tasks."}
                  </div>
                </div>
              </div>
            </AccordionSection>

            <div id="workflow-model-selection">
              <AccordionSection title="Model Selection">
                <ProviderModelSelector
                  providerLabel="Model provider"
                  modelLabel="Model"
                  draft={{
                    provider: workflowEditDraft.modelProvider,
                    model: workflowEditDraft.modelId,
                  }}
                  providers={providerOptions}
                  onChange={(draft) =>
                    setWorkflowEditDraft((current: any) =>
                      current
                        ? {
                            ...current,
                            modelProvider: draft.provider,
                            modelId: draft.model,
                          }
                        : current
                    )
                  }
                  inheritLabel="Workspace default"
                />
                {validateModelInput(workflowEditDraft.modelProvider, workflowEditDraft.modelId) ? (
                  <div className="text-xs text-red-300">
                    {validateModelInput(workflowEditDraft.modelProvider, workflowEditDraft.modelId)}
                  </div>
                ) : null}
                <div className="grid gap-2 rounded-lg border border-slate-800/70 bg-slate-950/30 p-3">
                  <div className="text-xs uppercase tracking-wide text-slate-500">
                    Planner fallback model
                  </div>
                  <div className="text-xs text-slate-400">
                    Optional. Leave blank to use the workflow default model for planning and
                    revisions.
                  </div>
                  <ProviderModelSelector
                    providerLabel="Planner provider"
                    modelLabel="Planner model"
                    draft={{
                      provider: workflowEditDraft.plannerModelProvider,
                      model: workflowEditDraft.plannerModelId,
                    }}
                    providers={providerOptions}
                    onChange={(draft) =>
                      setWorkflowEditDraft((current: any) =>
                        current
                          ? {
                              ...current,
                              plannerModelProvider: draft.provider,
                              plannerModelId: draft.model,
                            }
                          : current
                      )
                    }
                    inheritLabel="Use workflow model"
                  />
                  {validatePlannerModelInput(
                    workflowEditDraft.plannerModelProvider,
                    workflowEditDraft.plannerModelId
                  ) ? (
                    <div className="text-xs text-red-300">
                      {validatePlannerModelInput(
                        workflowEditDraft.plannerModelProvider,
                        workflowEditDraft.plannerModelId
                      )}
                    </div>
                  ) : null}
                </div>
              </AccordionSection>
            </div>

            <AccordionSection title="Tool Access">
              <div className="grid gap-2 sm:grid-cols-2">
                <button
                  type="button"
                  className={`tcp-list-item text-left ${workflowEditDraft.toolAccessMode === "all" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
                  onClick={() =>
                    setWorkflowEditDraft((current: any) =>
                      current ? { ...current, toolAccessMode: "all" } : current
                    )
                  }
                >
                  <div className="font-medium">All tools</div>
                  <div className="tcp-subtle text-xs">
                    Grant full built-in tool access to workflow agents.
                  </div>
                </button>
                <button
                  type="button"
                  className={`tcp-list-item text-left ${workflowEditDraft.toolAccessMode === "custom" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
                  onClick={() =>
                    setWorkflowEditDraft((current: any) =>
                      current ? { ...current, toolAccessMode: "custom" } : current
                    )
                  }
                >
                  <div className="font-medium">Custom allowlist</div>
                  <div className="tcp-subtle text-xs">
                    Restrict built-in tools manually. MCP tools still follow the selected servers.
                  </div>
                </button>
              </div>
              {workflowEditDraft.toolAccessMode === "custom" ? (
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Allowed built-in tools</label>
                  <textarea
                    className="tcp-input min-h-[96px] font-mono text-xs"
                    value={workflowEditDraft.customToolsText}
                    onInput={(e) =>
                      setWorkflowEditDraft((current: any) =>
                        current
                          ? {
                              ...current,
                              customToolsText: (e.target as HTMLTextAreaElement).value,
                            }
                          : current
                      )
                    }
                    placeholder={`read\nwrite\nedit\nbash\nls\nglob\nwebsearch`}
                  />
                </div>
              ) : (
                <div className="text-xs text-slate-500">
                  All built-in tools are allowed for this automation.
                </div>
              )}
            </AccordionSection>

            <div id="workflow-connector-bindings">
              <AccordionSection
                title="Connector bindings"
                description="Edit the connector binding snapshot that the scope inspector reads back. Save will persist the new binding set into the automation metadata. Each binding must include an explicit status (mapped, unresolved_required, or unresolved_optional)."
              >
                <textarea
                  className="tcp-input min-h-[220px] font-mono text-xs leading-5"
                  value={workflowEditDraft.connectorBindingsJson}
                  onInput={(e) =>
                    setWorkflowEditDraft((current: any) =>
                      current
                        ? {
                            ...current,
                            connectorBindingsJson: (e.target as HTMLTextAreaElement).value,
                          }
                        : current
                    )
                  }
                  placeholder={`[\n  {\n    "capability": "github",\n    "binding_type": "oauth",\n    "binding_id": "github-primary",\n    "allowlist_pattern": "github.com/*",\n    "status": "mapped"\n  },\n  {\n    "capability": "slack",\n    "binding_type": null,\n    "binding_id": null,\n    "allowlist_pattern": null,\n    "status": "unresolved_required"\n  }\n]`}
                />
                <div className="text-xs text-slate-500">
                  Keep this as a JSON array of binding objects with capability, binding_type,
                  binding_id, allowlist_pattern, and an explicit status: mapped,
                  unresolved_required, or unresolved_optional.
                </div>
              </AccordionSection>
            </div>

            <AccordionSection
              title="Shared workflow context"
              description="Bind approved shared workflow contexts here, one context id per line. The ids are validated against this workflow's workspace and kept on the saved automation metadata so later runs can reuse the same approved context."
            >
              <textarea
                className="tcp-input min-h-[120px] font-mono text-xs leading-5"
                value={workflowEditDraft.sharedContextPackIdsText}
                onInput={(e) =>
                  setWorkflowEditDraft((current: any) =>
                    current
                      ? {
                          ...current,
                          sharedContextPackIdsText: (e.target as HTMLTextAreaElement).value,
                        }
                      : current
                  )
                }
                placeholder={`context-context-123\ncontext-context-456`}
              />
              <div className="text-xs text-slate-500">
                Use the copy context id button in the Shared workflow context panel to paste ids
                quickly.
              </div>
            </AccordionSection>

            <AccordionSection title="MCP Servers">
              {mcpServers.length ? (
                <div className="grid gap-3">
                  <div className="flex flex-wrap gap-2">
                    {mcpServers.map((server: any) => {
                      const isSelected = workflowEditDraft.selectedMcpServers.includes(server.name);
                      return (
                        <button
                          key={server.name}
                          className={`tcp-btn h-7 px-2 text-xs ${
                            isSelected ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""
                          }`}
                          onClick={() =>
                            setWorkflowEditDraft((current: any) => {
                              if (!current) return current;
                              const serverPrefix = `mcp.${normalizeMcpNamespaceSegment(server.name)}.`;
                              const nextSelectedServers = isSelected
                                ? current.selectedMcpServers.filter(
                                    (name: string) => name !== server.name
                                  )
                                : [...current.selectedMcpServers, server.name].sort();
                              const nextSelectedTools = isSelected
                                ? Array.isArray(current.selectedMcpTools)
                                  ? current.selectedMcpTools.filter(
                                      (toolName: string) =>
                                        !String(toolName || "").startsWith(serverPrefix)
                                    )
                                  : current.selectedMcpTools
                                : current.selectedMcpTools;
                              return {
                                ...current,
                                selectedMcpServers: nextSelectedServers,
                                selectedMcpTools: nextSelectedTools,
                              };
                            })
                          }
                        >
                          {server.name} {server.connected ? "• connected" : "• disconnected"}
                        </button>
                      );
                    })}
                  </div>
                  {workflowEditDraft.selectedMcpServers.length ? (
                    <McpToolAllowlistEditor
                      title="Workflow MCP tool access"
                      subtitle="Leave all discovered tools selected to inherit full access from the chosen MCP servers, or uncheck tools to save an exact MCP allowlist for this workflow."
                      discoveredTools={mcpServers
                        .filter((server: any) =>
                          workflowEditDraft.selectedMcpServers.includes(server.name)
                        )
                        .flatMap((server: any) =>
                          Array.isArray(server.toolCache) ? server.toolCache : []
                        )}
                      value={workflowEditDraft.selectedMcpTools}
                      onChange={(next) =>
                        setWorkflowEditDraft((current: any) =>
                          current ? { ...current, selectedMcpTools: next } : current
                        )
                      }
                    />
                  ) : null}
                </div>
              ) : (
                <div className="text-xs text-slate-400">No MCP servers configured yet.</div>
              )}
            </AccordionSection>

            {/* ─── Connected Agent Handoffs ─────────────────────────────── */}
            <AccordionSection
              title="Connected agents"
              icon="network"
              headerStyle="violet"
              description="Handoff configuration, watch conditions, and scope policy for agent-to-agent messaging."
            >
              <HandoffConfigEditor
                value={workflowEditDraft.handoffConfig}
                onChange={(next: any) =>
                  setWorkflowEditDraft((current: any) =>
                    current ? { ...current, handoffConfig: next } : current
                  )
                }
              />

              <WatchConditionEditor
                value={workflowEditDraft.watchConditions ?? []}
                automations={automationsV2List}
                onChange={(next: any) =>
                  setWorkflowEditDraft((current: any) =>
                    current ? { ...current, watchConditions: next } : current
                  )
                }
              />

              <ScopePolicyEditor
                value={workflowEditDraft.scopePolicy}
                onChange={(next: any) =>
                  setWorkflowEditDraft((current: any) =>
                    current ? { ...current, scopePolicy: next } : current
                  )
                }
              />

              {workflowEditDraft.automationId && client?.automationsV2?.listHandoffs && (
                <HandoffPanel automationId={workflowEditDraft.automationId} client={client} />
              )}
            </AccordionSection>

            <AccordionSection title="Scope Inspector">
              <ScopeInspector
                title=""
                planPackage={workflowEditDraft.scopeSnapshot}
                planPackageBundle={workflowEditDraft.planPackageBundle}
                planPackageReplay={workflowEditDraft.planPackageReplay}
                validationReport={workflowEditDraft.scopeValidation}
                runtimeContext={workflowEditDraft.runtimeContext}
                approvedPlanMaterialization={workflowEditDraft.approvedPlanMaterialization}
                overlapHistoryEntries={overlapHistoryEntries}
                onOpenPromptEditor={() => {
                  document
                    .getElementById("workflow-prompt-editor")
                    ?.scrollIntoView({ behavior: "smooth", block: "start" });
                }}
                onOpenModelRoutingEditor={() => {
                  document
                    .getElementById("workflow-model-selection")
                    ?.scrollIntoView({ behavior: "smooth", block: "start" });
                }}
                onOpenConnectorBindingsEditor={() => {
                  document
                    .getElementById("workflow-connector-bindings")
                    ?.scrollIntoView({ behavior: "smooth", block: "start" });
                }}
                onReplaceSharedContextPack={(fromPackId: string, toPackId: string) => {
                  if (!fromPackId || !toPackId) return;
                  setWorkflowEditDraft((current: any) =>
                    current
                      ? {
                          ...current,
                          sharedContextPackIdsText: String(current.sharedContextPackIdsText || "")
                            .split(/[\n,]/g)
                            .map((value: string) => String(value || "").trim())
                            .filter(Boolean)
                            .map((value: string) => (value === fromPackId ? toPackId : value))
                            .filter(
                              (value: string, index: number, values: string[]) =>
                                values.indexOf(value) === index
                            )
                            .join("\n"),
                        }
                      : current
                  );
                }}
              />
            </AccordionSection>

            <div id="workflow-prompt-editor">
              <AccordionSection
                title="Prompt Editor"
                description="Edit the actual prompts Tandem sends for each workflow step. These objectives control what every node does at runtime."
                defaultOpen={false}
              >
                {workflowEditDraft.nodes.length ? (
                  <div className="grid gap-3">
                    {workflowEditDraft.nodes.map((node: any, index: number) => {
                      const nodeToolMode = node.toolAccessMode || "inherit";
                      const nodeMcpTools =
                        nodeToolMode === "custom"
                          ? node.mcpAllowedTools === null
                            ? (node.mcpAllowedServers || []).map(
                                (server: string) => `mcp.${normalizeMcpNamespaceSegment(server)}.*`
                              )
                            : [
                                ...(node.mcpOtherAllowedTools || []),
                                ...(node.mcpAllowedTools || []),
                              ]
                          : workflowEditDraft.selectedMcpTools === null
                            ? (workflowEditDraft.selectedMcpServers || []).map(
                                (server: string) => `mcp.${normalizeMcpNamespaceSegment(server)}.*`
                              )
                            : [
                                ...(workflowEditDraft.mcpOtherAllowedTools || []),
                                ...(workflowEditDraft.selectedMcpTools || []),
                              ];
                      const explicitTaskMcpTools = uniqueStrings([
                        ...(node.mcpOtherAllowedTools || []),
                        ...(node.mcpAllowedTools || []),
                      ]);
                      const inferredTaskServers = inferMcpServersFromTools(
                        explicitTaskMcpTools,
                        mcpServers
                      );
                      const taskMcpServerNames = uniqueStrings([
                        ...(node.mcpAllowedServers || []),
                        ...inferredTaskServers,
                      ]);
                      const nodeSendCapable = nodeMcpTools.some(toolLooksSendCapable);
                      return (
                        <div
                          key={node.nodeId || index}
                          className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3"
                        >
                          <div className="mb-2 flex flex-wrap items-center gap-2">
                            <strong className="text-sm text-slate-100">
                              {node.nodeId || node.title || `Step ${index + 1}`}
                            </strong>
                            {node.agentId ? (
                              <span className="tcp-badge-info">agent: {node.agentId}</span>
                            ) : null}
                          </div>
                          <textarea
                            className="tcp-input min-h-[180px] text-sm leading-6"
                            value={node.objective}
                            onInput={(e) =>
                              setWorkflowEditDraft((current: any) =>
                                current
                                  ? {
                                      ...current,
                                      nodes: current.nodes.map((row: any) =>
                                        row.nodeId === node.nodeId
                                          ? {
                                              ...row,
                                              objective: (e.target as HTMLTextAreaElement).value,
                                            }
                                          : row
                                      ),
                                    }
                                  : current
                              )
                            }
                            placeholder="Describe exactly what this step should do."
                          />
                          <details className="mt-3 rounded-lg border border-slate-800/70 bg-slate-950/30 p-3">
                            <summary className="cursor-pointer list-none">
                              <div className="flex flex-wrap items-center justify-between gap-2">
                                <div className="inline-flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-300">
                                  <i data-lucide="cpu"></i>
                                  <span>Step model & provider</span>
                                </div>
                                <div className="flex flex-wrap items-center gap-2 text-[11px]">
                                  {node.modelProvider || node.modelId ? (
                                    <span className="tcp-badge-info">overrides workflow model</span>
                                  ) : (
                                    <span className="tcp-badge-info">inherits workflow model</span>
                                  )}
                                  <span className="text-slate-500">
                                    Expand to change this task's model.
                                  </span>
                                </div>
                              </div>
                            </summary>
                            <div className="mt-3 grid gap-2">
                              <ProviderModelSelector
                                providerLabel="Step model provider"
                                modelLabel="Step model"
                                draft={{
                                  provider: node.modelProvider,
                                  model: node.modelId,
                                }}
                                providers={providerOptions}
                                onChange={(draftModel) =>
                                  setWorkflowEditDraft((current: any) =>
                                    current
                                      ? {
                                          ...current,
                                          nodes: current.nodes.map((row: any) =>
                                            row.nodeId === node.nodeId
                                              ? {
                                                  ...row,
                                                  modelProvider: draftModel.provider,
                                                  modelId: draftModel.model,
                                                }
                                              : row
                                          ),
                                        }
                                      : current
                                  )
                                }
                                inheritLabel="Use workflow model"
                              />
                              {validateModelInput(node.modelProvider, node.modelId) ? (
                                <div className="text-xs text-red-300">
                                  {validateModelInput(node.modelProvider, node.modelId)}
                                </div>
                              ) : (
                                <div className="text-xs text-slate-500">
                                  Leave both fields blank to inherit the workflow model.
                                </div>
                              )}
                            </div>
                          </details>
                          <details className="mt-3 rounded-lg border border-slate-800/70 bg-slate-950/30 p-3">
                            <summary className="cursor-pointer list-none">
                              <div className="flex flex-wrap items-center justify-between gap-2">
                                <div className="inline-flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-300">
                                  <i data-lucide="wrench"></i>
                                  <span>Task tool access</span>
                                </div>
                                <div className="flex flex-wrap gap-2 text-[11px]">
                                  <span className="tcp-badge-info">
                                    {nodeToolMode === "custom"
                                      ? "custom task tools"
                                      : "inherits workflow tools"}
                                  </span>
                                  <span
                                    className={
                                      nodeSendCapable ? "tcp-badge-danger" : "tcp-badge-info"
                                    }
                                  >
                                    {nodeSendCapable ? "send-capable" : "no send tools selected"}
                                  </span>
                                  <span className="text-slate-500">
                                    Expand to change tools for this task only.
                                  </span>
                                </div>
                              </div>
                            </summary>
                            <div className="mt-3 grid gap-3">
                              <div className="flex flex-wrap gap-2">
                                <button
                                  className={`tcp-btn h-7 px-2 text-xs ${
                                    nodeToolMode === "inherit"
                                      ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                                      : ""
                                  }`}
                                  onClick={() =>
                                    setWorkflowEditDraft((current: any) =>
                                      current
                                        ? {
                                            ...current,
                                            nodes: current.nodes.map((row: any) =>
                                              row.nodeId === node.nodeId
                                                ? {
                                                    ...row,
                                                    toolAccessMode: "inherit",
                                                    toolAllowlist: [],
                                                    toolDenylist: [],
                                                    mcpAllowedServers: [],
                                                    mcpAllowedTools: null,
                                                    mcpOtherAllowedTools: [],
                                                  }
                                                : row
                                            ),
                                          }
                                        : current
                                    )
                                  }
                                >
                                  Inherit workflow tools
                                </button>
                                <button
                                  className={`tcp-btn h-7 px-2 text-xs ${
                                    nodeToolMode === "custom"
                                      ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                                      : ""
                                  }`}
                                  onClick={() =>
                                    setWorkflowEditDraft((current: any) =>
                                      current
                                        ? {
                                            ...current,
                                            nodes: current.nodes.map((row: any) =>
                                              row.nodeId === node.nodeId
                                                ? {
                                                    ...row,
                                                    toolAccessMode: "custom",
                                                    toolAllowlist: row.toolAllowlist?.length
                                                      ? row.toolAllowlist
                                                      : String(current.customToolsText || "")
                                                          .split(/[\n,]/g)
                                                          .map((value: string) =>
                                                            String(value || "").trim()
                                                          )
                                                          .filter(Boolean),
                                                    mcpAllowedServers: row.mcpAllowedServers?.length
                                                      ? row.mcpAllowedServers
                                                      : current.selectedMcpServers || [],
                                                    mcpAllowedTools:
                                                      row.mcpAllowedTools === undefined
                                                        ? current.selectedMcpTools
                                                        : row.mcpAllowedTools,
                                                    mcpOtherAllowedTools:
                                                      row.mcpOtherAllowedTools ||
                                                      current.mcpOtherAllowedTools ||
                                                      [],
                                                  }
                                                : row
                                            ),
                                          }
                                        : current
                                    )
                                  }
                                >
                                  Customize this task
                                </button>
                              </div>
                              {nodeToolMode === "custom" ? (
                                <>
                                  <div className="grid gap-1">
                                    <label className="text-xs text-slate-400">
                                      Task tool allowlist
                                    </label>
                                    <textarea
                                      className="tcp-input min-h-[96px] font-mono text-xs"
                                      value={(node.toolAllowlist || []).join("\n")}
                                      onInput={(e) =>
                                        setWorkflowEditDraft((current: any) =>
                                          current
                                            ? {
                                                ...current,
                                                nodes: current.nodes.map((row: any) =>
                                                  row.nodeId === node.nodeId
                                                    ? {
                                                        ...row,
                                                        toolAllowlist: (
                                                          e.target as HTMLTextAreaElement
                                                        ).value
                                                          .split(/[\n,]/g)
                                                          .map((value: string) =>
                                                            String(value || "").trim()
                                                          )
                                                          .filter(Boolean),
                                                      }
                                                    : row
                                                ),
                                              }
                                            : current
                                        )
                                      }
                                    />
                                  </div>
                                  <div className="grid gap-2">
                                    <div className="flex flex-wrap items-center justify-between gap-2">
                                      <label className="text-xs text-slate-400">
                                        Task MCP servers
                                      </label>
                                      <span className="text-[11px] text-slate-500">
                                        Select runtime servers to reveal all available task tools.
                                      </span>
                                    </div>
                                    {mcpServers.length ? (
                                      <div className="flex flex-wrap gap-2">
                                        {mcpServers.map((server: any) => {
                                          const selected = taskMcpServerNames.includes(server.name);
                                          return (
                                            <button
                                              key={server.name}
                                              type="button"
                                              className={`tcp-btn h-7 px-2 text-xs ${
                                                selected
                                                  ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                                                  : ""
                                              }`}
                                              onClick={() =>
                                                setWorkflowEditDraft((current: any) => {
                                                  if (!current) return current;
                                                  return {
                                                    ...current,
                                                    nodes: current.nodes.map((row: any) => {
                                                      if (row.nodeId !== node.nodeId) return row;
                                                      const currentServers = uniqueStrings([
                                                        ...(row.mcpAllowedServers || []),
                                                        ...inferMcpServersFromTools(
                                                          [
                                                            ...(row.mcpOtherAllowedTools || []),
                                                            ...(row.mcpAllowedTools || []),
                                                          ],
                                                          mcpServers
                                                        ),
                                                      ]);
                                                      const nextServers = selected
                                                        ? currentServers.filter(
                                                            (name) => name !== server.name
                                                          )
                                                        : uniqueStrings([
                                                            ...currentServers,
                                                            server.name,
                                                          ]);
                                                      const nextAllowedTools = Array.isArray(
                                                        row.mcpAllowedTools
                                                      )
                                                        ? row.mcpAllowedTools.filter(
                                                            (toolName: string) =>
                                                              !selected ||
                                                              !mcpToolBelongsToServer(
                                                                toolName,
                                                                server.name
                                                              )
                                                          )
                                                        : row.mcpAllowedTools;
                                                      return {
                                                        ...row,
                                                        mcpAllowedServers: nextServers,
                                                        mcpAllowedTools: nextAllowedTools,
                                                      };
                                                    }),
                                                  };
                                                })
                                              }
                                            >
                                              {server.name}{" "}
                                              {server.connected ? "• connected" : "• disconnected"}
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
                                  <div className="grid gap-1">
                                    <label className="text-xs text-slate-400">
                                      Task tool denylist
                                    </label>
                                    <input
                                      className="tcp-input font-mono text-xs"
                                      value={(node.toolDenylist || []).join(", ")}
                                      onInput={(e) =>
                                        setWorkflowEditDraft((current: any) =>
                                          current
                                            ? {
                                                ...current,
                                                nodes: current.nodes.map((row: any) =>
                                                  row.nodeId === node.nodeId
                                                    ? {
                                                        ...row,
                                                        toolDenylist: (
                                                          e.target as HTMLInputElement
                                                        ).value
                                                          .split(/[\n,]/g)
                                                          .map((value: string) =>
                                                            String(value || "").trim()
                                                          )
                                                          .filter(Boolean),
                                                      }
                                                    : row
                                                ),
                                              }
                                            : current
                                        )
                                      }
                                    />
                                  </div>
                                  <McpToolAllowlistEditor
                                    title="Task MCP tool access"
                                    subtitle="This task-level allowlist overrides the workflow MCP selection for only this step."
                                    discoveredTools={mcpToolsForServers(
                                      mcpServers,
                                      taskMcpServerNames
                                    )}
                                    value={node.mcpAllowedTools}
                                    onChange={(next) =>
                                      setWorkflowEditDraft((current: any) =>
                                        current
                                          ? {
                                              ...current,
                                              nodes: current.nodes.map((row: any) =>
                                                row.nodeId === node.nodeId
                                                  ? { ...row, mcpAllowedTools: next }
                                                  : row
                                              ),
                                            }
                                          : current
                                      )
                                    }
                                    collapsible
                                    defaultCollapsed
                                  />
                                </>
                              ) : null}
                            </div>
                          </details>
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  <div className="text-xs text-slate-400">
                    This workflow does not currently expose editable node objectives.
                  </div>
                )}
              </AccordionSection>
            </div>
          </div>
        </div>
        <div className="tcp-confirm-actions border-t border-slate-800/70 px-4 py-3">
          <button className="tcp-btn" onClick={() => setWorkflowEditDraft(null)}>
            <i data-lucide="x-circle"></i>
            Cancel
          </button>
          <button
            className="tcp-btn"
            onClick={() =>
              workflowEditDraft &&
              workflowEditDraft.automationId &&
              runNowV2Mutation.mutate({
                id: workflowEditDraft.automationId,
              })
            }
            disabled={!workflowEditDraft?.automationId || runNowV2Mutation.isPending}
          >
            <i data-lucide="play"></i>
            {runNowV2Mutation.isPending ? "Starting..." : "Run now"}
          </button>
          <button
            className="tcp-btn-primary"
            onClick={() =>
              workflowEditDraft && updateWorkflowAutomationMutation.mutate(workflowEditDraft)
            }
            disabled={updateWorkflowAutomationMutation.isPending}
          >
            <i data-lucide="check"></i>
            {updateWorkflowAutomationMutation.isPending ? "Saving..." : "Save"}
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}

export function DeleteAutomationDialog({
  deleteConfirm,
  setDeleteConfirm,
  automationActionMutation,
}: any) {
  const dialogRef = useDialogIconRender(!!deleteConfirm);
  if (!deleteConfirm) return null;

  return (
    <motion.div
      ref={dialogRef}
      className="tcp-confirm-overlay"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={() => setDeleteConfirm(null)}
    >
      <motion.div
        className="tcp-confirm-dialog w-[min(34rem,96vw)]"
        initial={{ opacity: 0, y: 8, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 6, scale: 0.98 }}
        onClick={(event) => event.stopPropagation()}
      >
        <h3 className="tcp-confirm-title">Delete automation</h3>
        <p className="tcp-confirm-message">
          This will permanently remove <strong>{deleteConfirm.title}</strong>.
        </p>
        <div className="tcp-confirm-actions mt-3">
          <button className="tcp-btn" onClick={() => setDeleteConfirm(null)}>
            <i data-lucide="x"></i>
            Cancel
          </button>
          <button
            className="tcp-btn-danger"
            disabled={automationActionMutation.isPending}
            onClick={() =>
              automationActionMutation.mutate(
                {
                  action: "delete",
                  automationId: deleteConfirm.automationId,
                  family: deleteConfirm.family,
                },
                {
                  onSettled: () => setDeleteConfirm(null),
                }
              )
            }
          >
            <i data-lucide="trash-2"></i>
            {automationActionMutation.isPending ? "Deleting..." : "Delete automation"}
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
