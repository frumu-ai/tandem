import { useCallback, useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui";
import { ProjectSwitcher } from "@/components/sidebar";
import { AgentCommandCenter } from "@/components/orchestrate/AgentCommandCenter";
import {
  mcpConnect,
  mcpDisconnect,
  mcpListServers,
  mcpListTools,
  mcpRefresh,
  mcpSetEnabled,
  onSidecarEventV2,
  routinesCreate,
  routinesList,
  routinesPatch,
  routinesRunApprove,
  routinesRunDeny,
  routinesRunPause,
  routinesRunResume,
  routinesRunsAll,
  toolIds,
  type McpRemoteTool,
  type McpServerRecord,
  type RoutineRunRecord,
  type RoutineSpec,
  type StreamEventEnvelopeV2,
  type UserProject,
} from "@/lib/tauri";

type AgentAutomationTab = "automated-bots" | "agent-ops";
type BotTemplateId = "daily-research" | "issue-triage" | "release-reporter";

interface BotTemplate {
  id: BotTemplateId;
  label: string;
  description: string;
  name: string;
  intervalSeconds: number;
  entrypoint: "mission.default" | "mcp_first_tool";
  allowedTools: string[];
  requiresApproval: boolean;
  externalAllowed: boolean;
  outputTargets: string[];
  missionObjective: string;
  successCriteria: string[];
}

interface WorkshopMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
}

interface MissionDraft {
  objective: string;
  successCriteria: string[];
  suggestedMode: "standalone" | "orchestrated";
}

function buildMissionDraft(brief: string, tools: string[]): MissionDraft {
  const normalized = brief.trim();
  const lower = normalized.toLowerCase();
  const suggestedMode =
    lower.includes("verify") || lower.includes("multi") || lower.includes("workflow")
      ? "orchestrated"
      : "standalone";
  const objective =
    normalized.length > 0
      ? normalized
      : "Run a scheduled automation that gathers context, performs the task, and produces a clear artifact.";
  const successCriteria = [
    "Produces at least one output artifact at configured output targets.",
    "Uses only allowed tools and records run events.",
    suggestedMode === "orchestrated"
      ? "Verifier confirms output quality before completion."
      : "Completes without blocked policy or approval timeout.",
  ];
  if (tools.some((tool) => tool === "webfetch_document")) {
    successCriteria.push("Uses webfetch_document for web content extraction when relevant.");
  }
  return { objective, successCriteria, suggestedMode };
}

function modeFromArgs(args: Record<string, unknown> | undefined): string {
  const value = typeof args?.["mode"] === "string" ? args["mode"].trim() : "";
  return value || "standalone";
}

interface AgentAutomationPageProps {
  userProjects: UserProject[];
  activeProject: UserProject | null;
  onSwitchProject: (projectId: string) => void;
  onAddProject: () => void;
  onManageProjects: () => void;
  projectSwitcherLoading?: boolean;
  onOpenMcpExtensions?: () => void;
}

export function AgentAutomationPage({
  userProjects,
  activeProject,
  onSwitchProject,
  onAddProject,
  onManageProjects,
  projectSwitcherLoading = false,
  onOpenMcpExtensions,
}: AgentAutomationPageProps) {
  const [tab, setTab] = useState<AgentAutomationTab>("automated-bots");
  const [error, setError] = useState<string | null>(null);

  const [mcpServers, setMcpServers] = useState<McpServerRecord[]>([]);
  const [mcpTools, setMcpTools] = useState<McpRemoteTool[]>([]);
  const [mcpLoading, setMcpLoading] = useState(false);
  const [busyConnector, setBusyConnector] = useState<string | null>(null);
  const [availableToolIds, setAvailableToolIds] = useState<string[]>([]);

  const [routines, setRoutines] = useState<RoutineSpec[]>([]);
  const [routinesLoading, setRoutinesLoading] = useState(false);
  const [createRoutineLoading, setCreateRoutineLoading] = useState(false);
  const [routineNameDraft, setRoutineNameDraft] = useState("MCP Automation");
  const [routineEntrypointDraft, setRoutineEntrypointDraft] = useState("mission.default");
  const [routineIntervalSecondsDraft, setRoutineIntervalSecondsDraft] = useState(300);
  const [routineAllowedToolsDraft, setRoutineAllowedToolsDraft] = useState<string[]>([]);
  const [routineMissionObjectiveDraft, setRoutineMissionObjectiveDraft] = useState("");
  const [routineSuccessCriteriaDraft, setRoutineSuccessCriteriaDraft] = useState("");
  const [routineModeDraft, setRoutineModeDraft] = useState<"standalone" | "orchestrated">(
    "standalone"
  );
  const [routineOrchestratorOnlyToolCallsDraft, setRoutineOrchestratorOnlyToolCallsDraft] =
    useState(false);
  const [routineOutputTargetsDraft, setRoutineOutputTargetsDraft] = useState("");
  const [routineRequiresApprovalDraft, setRoutineRequiresApprovalDraft] = useState(true);
  const [routineExternalAllowedDraft, setRoutineExternalAllowedDraft] = useState(true);
  const [workshopInputDraft, setWorkshopInputDraft] = useState("");
  const [workshopMessages, setWorkshopMessages] = useState<WorkshopMessage[]>([
    {
      id: "workshop-welcome",
      role: "assistant",
      text: "Mission Workshop: describe the bot mission in plain language. I will suggest an objective, success criteria, and default mode.",
    },
  ]);

  const [routineRuns, setRoutineRuns] = useState<RoutineRunRecord[]>([]);
  const [routineRunsLoading, setRoutineRunsLoading] = useState(false);
  const [routineActionBusyRunId, setRoutineActionBusyRunId] = useState<string | null>(null);

  const templates: BotTemplate[] = useMemo(
    () => [
      {
        id: "daily-research",
        label: "Daily Research",
        description: "Web + MCP research with markdown extraction output.",
        name: "Daily MCP Research",
        intervalSeconds: 86400,
        entrypoint: "mission.default",
        allowedTools: ["websearch", "webfetch_document", "read", "write"],
        requiresApproval: true,
        externalAllowed: true,
        outputTargets: ["file://reports/daily-mcp-research.md"],
        missionObjective:
          "Research daily topic signals and produce a concise markdown digest with citations.",
        successCriteria: [
          "Includes top findings with source URLs.",
          "Writes an artifact to the configured report path.",
          "Highlights uncertain claims and verification notes.",
        ],
      },
      {
        id: "issue-triage",
        label: "Issue Triage",
        description: "Classify inbound issues and post suggested actions.",
        name: "Issue Triage Bot",
        intervalSeconds: 900,
        entrypoint: "mission.default",
        allowedTools: ["read", "write", "websearch", "webfetch_document"],
        requiresApproval: true,
        externalAllowed: true,
        outputTargets: ["file://reports/issue-triage.json"],
        missionObjective:
          "Review incoming issues, classify severity, and draft recommended next actions.",
        successCriteria: [
          "Classifies each issue into priority buckets.",
          "Includes suggested owner/team when inferable.",
          "Produces a machine-readable triage artifact.",
        ],
      },
      {
        id: "release-reporter",
        label: "Release Reporter",
        description: "Compile status updates into a periodic artifact report.",
        name: "Release Reporter",
        intervalSeconds: 3600,
        entrypoint: "mission.default",
        allowedTools: ["read", "websearch", "webfetch_document", "write"],
        requiresApproval: false,
        externalAllowed: true,
        outputTargets: ["file://reports/release-status.md"],
        missionObjective:
          "Compile release readiness status and generate an hourly summary report for operators.",
        successCriteria: [
          "Summarizes current release blockers and risks.",
          "Writes a markdown status report artifact.",
          "Runs unattended without policy violations.",
        ],
      },
    ],
    []
  );

  const loadMcpStatus = useCallback(async () => {
    setMcpLoading(true);
    try {
      const [servers, tools] = await Promise.all([mcpListServers(), mcpListTools()]);
      setMcpServers(servers);
      setMcpTools(tools);
    } catch {
      setMcpServers([]);
      setMcpTools([]);
    } finally {
      setMcpLoading(false);
    }
  }, []);

  const loadToolCatalog = useCallback(async () => {
    try {
      const ids = await toolIds();
      setAvailableToolIds(
        [...new Set(ids.map((id) => id.trim()).filter((id) => id.length > 0))].sort()
      );
    } catch {
      setAvailableToolIds([]);
    }
  }, []);

  const loadRoutines = useCallback(async () => {
    setRoutinesLoading(true);
    try {
      const rows = await routinesList();
      rows.sort((a, b) => a.routine_id.localeCompare(b.routine_id));
      setRoutines(rows);
    } catch {
      setRoutines([]);
    } finally {
      setRoutinesLoading(false);
    }
  }, []);

  const loadRoutineRuns = useCallback(async () => {
    setRoutineRunsLoading(true);
    try {
      const rows = await routinesRunsAll(undefined, 30);
      rows.sort((a, b) => b.created_at_ms - a.created_at_ms);
      setRoutineRuns(rows);
    } catch {
      setRoutineRuns([]);
    } finally {
      setRoutineRunsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadMcpStatus();
    const timer = setInterval(() => void loadMcpStatus(), 10000);
    return () => clearInterval(timer);
  }, [loadMcpStatus]);

  useEffect(() => {
    void loadToolCatalog();
    const timer = setInterval(() => void loadToolCatalog(), 15000);
    return () => clearInterval(timer);
  }, [loadToolCatalog]);

  useEffect(() => {
    void loadRoutines();
    const timer = setInterval(() => void loadRoutines(), 15000);
    return () => clearInterval(timer);
  }, [loadRoutines]);

  useEffect(() => {
    void loadRoutineRuns();
    const timer = setInterval(() => void loadRoutineRuns(), 10000);
    return () => clearInterval(timer);
  }, [loadRoutineRuns]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    const setup = async () => {
      unlisten = await onSidecarEventV2((envelope: StreamEventEnvelopeV2) => {
        if (envelope?.payload?.type !== "raw") {
          return;
        }
        const eventType = envelope.payload.event_type;
        if (eventType.startsWith("mcp.")) {
          void loadMcpStatus();
          void loadToolCatalog();
          return;
        }
        if (eventType.startsWith("routine.")) {
          void loadRoutines();
          void loadRoutineRuns();
          return;
        }
        if (eventType.startsWith("agent_team.")) {
          // Agent Ops tab handles its own refresh; this keeps page-level state simple.
        }
      });
    };
    void setup();
    return () => {
      if (unlisten) unlisten();
    };
  }, [loadMcpStatus, loadRoutineRuns, loadRoutines, loadToolCatalog]);

  const mcpToolIds = useMemo(
    () =>
      [...new Set(mcpTools.map((tool) => tool.namespaced_name))]
        .filter((tool) => tool.trim().length > 0)
        .sort(),
    [mcpTools]
  );

  const allowlistChoices = useMemo(
    () =>
      [
        ...new Set([
          ...availableToolIds,
          "read",
          "write",
          "bash",
          "websearch",
          "webfetch_document",
          ...mcpToolIds,
        ]),
      ]
        .filter((tool) => tool.trim().length > 0)
        .sort(),
    [availableToolIds, mcpToolIds]
  );

  useEffect(() => {
    if (routineAllowedToolsDraft.length > 0) return;
    const defaults = ["read", "websearch", "webfetch_document"];
    if (mcpToolIds.length > 0) {
      defaults.push(mcpToolIds[0]);
    }
    setRoutineAllowedToolsDraft(defaults);
  }, [mcpToolIds, routineAllowedToolsDraft.length]);

  const applyTemplate = (template: BotTemplate) => {
    const firstMcpTool = mcpToolIds[0];
    const nextEntrypoint =
      template.entrypoint === "mcp_first_tool" && firstMcpTool ? firstMcpTool : "mission.default";
    const mergedTools = [...template.allowedTools];
    if (firstMcpTool && !mergedTools.includes(firstMcpTool)) {
      mergedTools.push(firstMcpTool);
    }
    setRoutineNameDraft(template.name);
    setRoutineIntervalSecondsDraft(template.intervalSeconds);
    setRoutineEntrypointDraft(nextEntrypoint);
    setRoutineAllowedToolsDraft(mergedTools);
    setRoutineRequiresApprovalDraft(template.requiresApproval);
    setRoutineExternalAllowedDraft(template.externalAllowed);
    setRoutineOutputTargetsDraft(template.outputTargets.join(", "));
    setRoutineMissionObjectiveDraft(template.missionObjective);
    setRoutineSuccessCriteriaDraft(template.successCriteria.join("\n"));
    setRoutineModeDraft("standalone");
    setRoutineOrchestratorOnlyToolCallsDraft(false);
  };

  useEffect(() => {
    if (routineMissionObjectiveDraft.trim().length > 0) return;
    setRoutineMissionObjectiveDraft(
      "Run a scheduled automation with a clear mission objective and artifact output."
    );
    setRoutineSuccessCriteriaDraft(
      "Produces one artifact per run.\nUses only allowed tools.\nLogs run events for observability."
    );
  }, [routineMissionObjectiveDraft]);

  const formatIntervalHint = (seconds: number): string => {
    if (!Number.isFinite(seconds) || seconds <= 0) return "";
    if (seconds % 86400 === 0) {
      const days = seconds / 86400;
      return `every ${days} day${days === 1 ? "" : "s"}`;
    }
    if (seconds % 3600 === 0) {
      const hours = seconds / 3600;
      return `every ${hours} hour${hours === 1 ? "" : "s"}`;
    }
    if (seconds % 60 === 0) {
      const mins = seconds / 60;
      return `every ${mins} minute${mins === 1 ? "" : "s"}`;
    }
    return `every ${seconds} second${seconds === 1 ? "" : "s"}`;
  };

  const toggleRoutineAllowedTool = (toolId: string) => {
    setRoutineAllowedToolsDraft((prev) => {
      if (prev.includes(toolId)) {
        return prev.filter((row) => row !== toolId);
      }
      return [...prev, toolId];
    });
  };

  const applyMissionDraft = (draft: MissionDraft) => {
    setRoutineMissionObjectiveDraft(draft.objective);
    setRoutineSuccessCriteriaDraft(draft.successCriteria.join("\n"));
    setRoutineModeDraft(draft.suggestedMode);
  };

  const handleWorkshopSubmit = () => {
    const text = workshopInputDraft.trim();
    if (!text) return;
    const userMessage: WorkshopMessage = {
      id: `workshop-user-${Date.now()}`,
      role: "user",
      text,
    };
    const draft = buildMissionDraft(text, routineAllowedToolsDraft);
    const assistantText = [
      `Suggested objective: ${draft.objective}`,
      `Suggested mode: ${draft.suggestedMode}`,
      "Suggested success criteria:",
      ...draft.successCriteria.map((row, index) => `${index + 1}. ${row}`),
    ].join("\n");
    const assistantMessage: WorkshopMessage = {
      id: `workshop-assistant-${Date.now()}`,
      role: "assistant",
      text: assistantText,
    };
    setWorkshopMessages((prev) => [...prev, userMessage, assistantMessage]);
    setWorkshopInputDraft("");
    applyMissionDraft(draft);
  };

  const handleCreateRoutine = async () => {
    const trimmedName = routineNameDraft.trim();
    if (!trimmedName) {
      setError("Routine name is required.");
      return;
    }
    const missionObjective = routineMissionObjectiveDraft.trim();
    if (!missionObjective) {
      setError("Mission objective is required.");
      return;
    }
    const successCriteria = routineSuccessCriteriaDraft
      .split("\n")
      .map((row) => row.trim())
      .filter((row) => row.length > 0);
    const intervalSeconds = Math.max(1, Math.floor(routineIntervalSecondsDraft));
    const outputTargets = routineOutputTargetsDraft
      .split(",")
      .map((value) => value.trim())
      .filter((value) => value.length > 0);

    setCreateRoutineLoading(true);
    setError(null);
    try {
      await routinesCreate({
        name: trimmedName,
        schedule: { interval_seconds: { seconds: intervalSeconds } },
        entrypoint: routineEntrypointDraft.trim() || "mission.default",
        args: {
          prompt: missionObjective,
          success_criteria: successCriteria,
          mode: routineModeDraft,
          orchestrator_only_tool_calls: routineOrchestratorOnlyToolCallsDraft,
        },
        allowed_tools: routineAllowedToolsDraft,
        output_targets: outputTargets,
        requires_approval: routineRequiresApprovalDraft,
        external_integrations_allowed: routineExternalAllowedDraft,
      });
      await Promise.all([loadRoutines(), loadRoutineRuns()]);
      setRoutineNameDraft("MCP Automation");
      setRoutineIntervalSecondsDraft(300);
      setRoutineOutputTargetsDraft("");
      setRoutineMissionObjectiveDraft("");
      setRoutineSuccessCriteriaDraft("");
      setRoutineModeDraft("standalone");
      setRoutineOrchestratorOnlyToolCallsDraft(false);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Create routine failed: ${message}`);
    } finally {
      setCreateRoutineLoading(false);
    }
  };

  const handleToggleRoutineStatus = async (routine: RoutineSpec) => {
    try {
      await routinesPatch(routine.routine_id, {
        status: routine.status === "active" ? "paused" : "active",
      });
      await loadRoutines();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Update routine failed: ${message}`);
    }
  };

  const handleRoutineRunAction = async (
    run: RoutineRunRecord,
    action: "approve" | "deny" | "pause" | "resume"
  ) => {
    setRoutineActionBusyRunId(run.run_id);
    try {
      if (action === "approve") {
        await routinesRunApprove(run.run_id);
      } else if (action === "deny") {
        await routinesRunDeny(run.run_id);
      } else if (action === "pause") {
        await routinesRunPause(run.run_id);
      } else {
        await routinesRunResume(run.run_id);
      }
      await loadRoutineRuns();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Routine run action failed: ${message}`);
    } finally {
      setRoutineActionBusyRunId(null);
    }
  };

  const handleConnectorAction = async (
    serverName: string,
    action: "set-enabled" | "connect" | "disconnect" | "refresh",
    nextEnabled?: boolean
  ) => {
    setBusyConnector(`${serverName}:${action}`);
    setError(null);
    try {
      if (action === "set-enabled") {
        await mcpSetEnabled(serverName, !!nextEnabled);
      } else if (action === "connect") {
        await mcpConnect(serverName);
      } else if (action === "disconnect") {
        await mcpDisconnect(serverName);
      } else {
        await mcpRefresh(serverName);
      }
      await loadMcpStatus();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Connector action failed: ${message}`);
    } finally {
      setBusyConnector(null);
    }
  };

  const connectedConnectors = mcpServers.filter((row) => row.connected).length;
  const activeRoutines = routines.filter((routine) => routine.status === "active").length;
  const pendingApprovals = routineRuns.filter((run) => run.status === "pending_approval").length;
  const blockedRuns = routineRuns.filter((run) => run.status === "blocked_policy").length;
  const artifactCount = routineRuns.reduce((sum, run) => sum + run.artifacts.length, 0);

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mx-auto max-w-[1600px] space-y-4">
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h2 className="text-lg font-semibold text-text">Agent Automation</h2>
              <p className="text-xs text-text-muted">
                Scheduled bots, MCP connector operations, approvals, and runtime visibility.
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant={tab === "automated-bots" ? "primary" : "secondary"}
                size="sm"
                onClick={() => setTab("automated-bots")}
              >
                Automated Bots
              </Button>
              <Button
                variant={tab === "agent-ops" ? "primary" : "secondary"}
                size="sm"
                onClick={() => setTab("agent-ops")}
              >
                Agent Ops
              </Button>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-border bg-surface p-4">
          <ProjectSwitcher
            projects={userProjects}
            activeProject={activeProject}
            onSwitchProject={onSwitchProject}
            onAddProject={onAddProject}
            onManageProjects={onManageProjects}
            isLoading={projectSwitcherLoading}
          />
        </div>

        {error ? (
          <div className="rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
            {error}
          </div>
        ) : null}

        {tab === "automated-bots" ? (
          <>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-5">
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Active Routines
                </div>
                <div className="text-lg font-semibold text-text">{activeRoutines}</div>
              </div>
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Needs Approval
                </div>
                <div className="text-lg font-semibold text-text">{pendingApprovals}</div>
              </div>
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">Blocked</div>
                <div className="text-lg font-semibold text-text">{blockedRuns}</div>
              </div>
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Connected MCP
                </div>
                <div className="text-lg font-semibold text-text">
                  {connectedConnectors}/{mcpServers.length}
                </div>
              </div>
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Artifacts
                </div>
                <div className="text-lg font-semibold text-text">{artifactCount}</div>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="text-xs uppercase tracking-wide text-text-subtle">
                  Automation Wiring
                </div>
                <div className="text-xs text-text-muted">
                  {routinesLoading ? "Refreshing..." : `${routines.length} configured`}
                </div>
              </div>
              <div className="mt-2 grid grid-cols-1 gap-3 lg:grid-cols-2">
                <div className="rounded-md border border-border bg-surface-elevated/40 p-3">
                  <div className="text-xs font-semibold text-text">Create Scheduled Bot</div>
                  <div className="mt-2">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Ready Templates
                    </div>
                    <div className="mt-1 grid grid-cols-1 gap-1 sm:grid-cols-3">
                      {templates.map((template) => (
                        <button
                          key={template.id}
                          type="button"
                          className="rounded border border-border bg-surface px-2 py-1 text-left hover:border-primary/40 hover:bg-surface-elevated"
                          onClick={() => applyTemplate(template)}
                          title={template.description}
                        >
                          <div className="text-[11px] font-semibold text-text">
                            {template.label}
                          </div>
                          <div className="truncate text-[10px] text-text-muted">
                            {template.description}
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="mt-3 rounded border border-border bg-surface p-2">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Mission Workshop
                    </div>
                    <div className="mt-1 max-h-28 space-y-1 overflow-y-auto rounded border border-border/60 bg-surface-elevated/30 p-2">
                      {workshopMessages.slice(-6).map((message) => (
                        <div key={message.id} className="text-[11px] text-text">
                          <span className="font-semibold text-text-subtle">
                            {message.role === "assistant" ? "Workshop" : "You"}:
                          </span>{" "}
                          <span className="whitespace-pre-wrap">{message.text}</span>
                        </div>
                      ))}
                    </div>
                    <div className="mt-2 flex gap-2">
                      <input
                        value={workshopInputDraft}
                        onChange={(event) => setWorkshopInputDraft(event.target.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            event.preventDefault();
                            handleWorkshopSubmit();
                          }
                        }}
                        className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                        placeholder="Describe the mission in plain language..."
                      />
                      <Button size="sm" variant="secondary" onClick={handleWorkshopSubmit}>
                        Suggest
                      </Button>
                    </div>
                  </div>
                  <div className="mt-2 space-y-2">
                    <input
                      value={routineNameDraft}
                      onChange={(event) => setRoutineNameDraft(event.target.value)}
                      className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      placeholder="Routine name"
                    />
                    <div className="grid grid-cols-2 gap-2">
                      <div className="space-y-1">
                        <input
                          type="number"
                          min={1}
                          value={routineIntervalSecondsDraft}
                          onChange={(event) =>
                            setRoutineIntervalSecondsDraft(
                              Number.parseInt(event.target.value || "300", 10)
                            )
                          }
                          className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                          placeholder="Interval (seconds)"
                        />
                        <div className="text-[10px] text-text-subtle">
                          Unit: seconds ({formatIntervalHint(routineIntervalSecondsDraft)})
                        </div>
                      </div>
                      <select
                        value={routineEntrypointDraft}
                        onChange={(event) => setRoutineEntrypointDraft(event.target.value)}
                        className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      >
                        <option value="mission.default">mission.default</option>
                        {mcpToolIds.map((toolId) => (
                          <option key={toolId} value={toolId}>
                            {toolId}
                          </option>
                        ))}
                      </select>
                    </div>
                    <textarea
                      value={routineMissionObjectiveDraft}
                      onChange={(event) => setRoutineMissionObjectiveDraft(event.target.value)}
                      className="min-h-[72px] w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      placeholder="Mission objective (required)"
                    />
                    <textarea
                      value={routineSuccessCriteriaDraft}
                      onChange={(event) => setRoutineSuccessCriteriaDraft(event.target.value)}
                      className="min-h-[64px] w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      placeholder="Success criteria (one per line)"
                    />
                    <div className="grid grid-cols-2 gap-2">
                      <label className="text-[11px] text-text-subtle">
                        Mode
                        <select
                          value={routineModeDraft}
                          onChange={(event) =>
                            setRoutineModeDraft(
                              event.target.value === "orchestrated" ? "orchestrated" : "standalone"
                            )
                          }
                          className="mt-1 w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                        >
                          <option value="standalone">standalone</option>
                          <option value="orchestrated">orchestrated</option>
                        </select>
                      </label>
                      <label className="inline-flex items-center gap-2 self-end text-[11px] text-text-subtle">
                        <input
                          type="checkbox"
                          checked={routineOrchestratorOnlyToolCallsDraft}
                          onChange={(event) =>
                            setRoutineOrchestratorOnlyToolCallsDraft(event.target.checked)
                          }
                        />
                        Orchestrator-only tool calls
                      </label>
                    </div>
                    <div className="grid grid-cols-2 gap-2 text-[11px] text-text-subtle">
                      <label className="inline-flex items-center gap-1">
                        <input
                          type="checkbox"
                          checked={routineRequiresApprovalDraft}
                          onChange={(event) =>
                            setRoutineRequiresApprovalDraft(event.target.checked)
                          }
                        />
                        Requires approval
                      </label>
                      <label className="inline-flex items-center gap-1">
                        <input
                          type="checkbox"
                          checked={routineExternalAllowedDraft}
                          onChange={(event) => setRoutineExternalAllowedDraft(event.target.checked)}
                        />
                        External allowed
                      </label>
                    </div>
                    <div className="rounded border border-border bg-surface p-2">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Allowed Tools
                      </div>
                      <div className="mt-1 max-h-32 space-y-1 overflow-y-auto pr-1">
                        {allowlistChoices.map((toolId) => (
                          <label
                            key={`allowlist-${toolId}`}
                            className="flex items-center gap-2 text-[11px] text-text"
                          >
                            <input
                              type="checkbox"
                              checked={routineAllowedToolsDraft.includes(toolId)}
                              onChange={() => toggleRoutineAllowedTool(toolId)}
                            />
                            <span className="truncate font-mono text-[10px]">{toolId}</span>
                          </label>
                        ))}
                        {allowlistChoices.length === 0 ? (
                          <div className="text-[11px] text-text-muted">
                            No tools available yet. Connect MCP servers to populate options.
                          </div>
                        ) : null}
                      </div>
                    </div>
                    <input
                      value={routineOutputTargetsDraft}
                      onChange={(event) => setRoutineOutputTargetsDraft(event.target.value)}
                      className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      placeholder="Output targets (comma-separated URIs)"
                    />
                    <Button
                      size="sm"
                      variant="primary"
                      disabled={createRoutineLoading}
                      onClick={() => void handleCreateRoutine()}
                    >
                      {createRoutineLoading ? "Creating..." : "Create routine"}
                    </Button>
                  </div>
                </div>
                <div className="rounded-md border border-border bg-surface-elevated/40 p-3">
                  <div className="text-xs font-semibold text-text">Configured Routines</div>
                  <div className="mt-2 space-y-2">
                    {routines.slice(0, 8).map((routine) => (
                      <div
                        key={routine.routine_id}
                        className="rounded border border-border bg-surface px-2 py-2"
                      >
                        <div className="flex items-center justify-between gap-2">
                          <div className="min-w-0">
                            <div className="truncate text-xs font-semibold text-text">
                              {routine.name}
                            </div>
                            <div className="truncate text-[11px] text-text-muted">
                              {routine.routine_id}
                            </div>
                            <div className="truncate text-[11px] text-text-subtle">
                              {routine.entrypoint} | {routine.status} | mode:{" "}
                              {modeFromArgs(routine.args)}
                            </div>
                            {routine.args?.["orchestrator_only_tool_calls"] ? (
                              <div className="truncate text-[11px] text-text-subtle">
                                policy: orchestrator-only tool calls
                              </div>
                            ) : null}
                            {typeof routine.args?.["prompt"] === "string" &&
                            routine.args["prompt"].trim().length > 0 ? (
                              <div className="line-clamp-2 text-[11px] text-text-subtle">
                                mission: {routine.args["prompt"]}
                              </div>
                            ) : null}
                            {routine.output_targets.length > 0 ? (
                              <div className="truncate text-[11px] text-text-subtle">
                                outputs: {routine.output_targets.length}
                              </div>
                            ) : null}
                          </div>
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() => void handleToggleRoutineStatus(routine)}
                          >
                            {routine.status === "active" ? "Pause" : "Resume"}
                          </Button>
                        </div>
                      </div>
                    ))}
                    {!routinesLoading && routines.length === 0 ? (
                      <div className="rounded border border-border bg-surface px-2 py-2 text-xs text-text-muted">
                        No routines configured.
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="text-xs uppercase tracking-wide text-text-subtle">
                  Scheduled Bots
                </div>
                <div className="text-xs text-text-muted">
                  {routineRunsLoading ? "Refreshing..." : `${routineRuns.length} recent runs`}
                </div>
              </div>
              <div className="mt-2 space-y-2">
                {routineRuns.slice(0, 8).map((run) => {
                  const busy = routineActionBusyRunId === run.run_id;
                  return (
                    <div
                      key={run.run_id}
                      className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2"
                    >
                      <div className="flex items-center justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate text-xs font-semibold text-text">
                            {run.routine_id} | {run.status}
                          </div>
                          <div className="mt-0.5 truncate text-[11px] text-text-muted">
                            run {run.run_id} | {run.trigger_type} | mode: {modeFromArgs(run.args)}
                          </div>
                          {run.args?.["orchestrator_only_tool_calls"] ? (
                            <div className="mt-0.5 text-[11px] text-text-subtle">
                              policy: orchestrator-only tool calls
                            </div>
                          ) : null}
                          {run.allowed_tools.length > 0 ? (
                            <div className="mt-1 flex flex-wrap gap-1">
                              {run.allowed_tools.slice(0, 3).map((toolId) => (
                                <span
                                  key={`${run.run_id}-${toolId}`}
                                  className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] text-text-subtle"
                                >
                                  {toolId}
                                </span>
                              ))}
                              {run.allowed_tools.length > 3 ? (
                                <span className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] text-text-subtle">
                                  +{run.allowed_tools.length - 3} more
                                </span>
                              ) : null}
                            </div>
                          ) : (
                            <div className="mt-0.5 text-[11px] text-text-subtle">
                              tool scope: all
                            </div>
                          )}
                          {run.output_targets.length > 0 ? (
                            <div className="mt-0.5 text-[11px] text-text-subtle">
                              outputs: {run.output_targets.length}
                            </div>
                          ) : null}
                          {run.artifacts.length > 0 ? (
                            <div className="mt-0.5 text-[11px] text-text-subtle">
                              {run.artifacts.length} artifact{run.artifacts.length === 1 ? "" : "s"}
                            </div>
                          ) : null}
                        </div>
                        <div className="flex items-center gap-1">
                          {run.status === "pending_approval" ? (
                            <>
                              <Button
                                size="sm"
                                variant="secondary"
                                disabled={busy}
                                onClick={() => void handleRoutineRunAction(run, "approve")}
                              >
                                Approve
                              </Button>
                              <Button
                                size="sm"
                                variant="ghost"
                                disabled={busy}
                                onClick={() => void handleRoutineRunAction(run, "deny")}
                              >
                                Deny
                              </Button>
                            </>
                          ) : null}
                          {(run.status === "queued" || run.status === "running") && (
                            <Button
                              size="sm"
                              variant="ghost"
                              disabled={busy}
                              onClick={() => void handleRoutineRunAction(run, "pause")}
                            >
                              Pause
                            </Button>
                          )}
                          {run.status === "paused" && (
                            <Button
                              size="sm"
                              variant="ghost"
                              disabled={busy}
                              onClick={() => void handleRoutineRunAction(run, "resume")}
                            >
                              Resume
                            </Button>
                          )}
                        </div>
                      </div>
                    </div>
                  );
                })}
                {!routineRunsLoading && routineRuns.length === 0 ? (
                  <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2 text-xs text-text-muted">
                    No recent routine runs.
                  </div>
                ) : null}
              </div>
            </div>

            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="text-xs uppercase tracking-wide text-text-subtle">Connectors</div>
                <div className="text-xs text-text-muted">
                  {mcpLoading
                    ? "Refreshing..."
                    : `${connectedConnectors}/${mcpServers.length} connected`}
                </div>
              </div>
              <div className="mt-2 flex items-center justify-between gap-2">
                <div className="text-xs text-text-muted">
                  Add or edit server config in Extensions, then operate connectors here.
                </div>
                {onOpenMcpExtensions ? (
                  <Button size="sm" variant="secondary" onClick={onOpenMcpExtensions}>
                    Open Extensions MCP
                  </Button>
                ) : null}
              </div>
              <div className="mt-2 grid grid-cols-1 gap-2 md:grid-cols-2">
                {mcpServers.slice(0, 8).map((server) => {
                  const count = mcpTools.filter((tool) => tool.server_name === server.name).length;
                  const busy = busyConnector?.startsWith(`${server.name}:`) ?? false;
                  return (
                    <div
                      key={server.name}
                      className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2"
                    >
                      <div className="text-xs font-semibold text-text">{server.name}</div>
                      <div className="mt-0.5 text-[11px] text-text-muted">
                        {server.enabled ? "enabled" : "disabled"} ·{" "}
                        {server.connected ? "connected" : "disconnected"} · {count} tools
                      </div>
                      {server.last_error ? (
                        <div className="mt-1 text-[11px] text-red-300">{server.last_error}</div>
                      ) : null}
                      <div className="mt-2 flex flex-wrap gap-1">
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={busy}
                          onClick={() =>
                            void handleConnectorAction(server.name, "set-enabled", !server.enabled)
                          }
                        >
                          {server.enabled ? "Disable" : "Enable"}
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy || !server.enabled}
                          onClick={() =>
                            void handleConnectorAction(
                              server.name,
                              server.connected ? "disconnect" : "connect"
                            )
                          }
                        >
                          {server.connected ? "Disconnect" : "Connect"}
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy || !server.enabled}
                          onClick={() => void handleConnectorAction(server.name, "refresh")}
                        >
                          Refresh
                        </Button>
                      </div>
                    </div>
                  );
                })}
                {!mcpLoading && mcpServers.length === 0 ? (
                  <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2 text-xs text-text-muted">
                    No MCP connectors configured.
                  </div>
                ) : null}
              </div>
            </div>
          </>
        ) : (
          <AgentCommandCenter />
        )}
      </div>
    </div>
  );
}
