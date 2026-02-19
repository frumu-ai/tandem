import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  approveTool,
  agentTeamApproveSpawn,
  agentTeamCancelInstance,
  agentTeamCancelMission,
  agentTeamDenySpawn,
  agentTeamListApprovals,
  agentTeamListInstances,
  agentTeamListMissions,
  agentTeamListTemplates,
  agentTeamSpawn,
  denyTool,
  onSidecarEventV2,
  type AgentTeamApprovals,
  type AgentTeamInstance,
  type AgentTeamMissionSummary,
  type AgentTeamTemplate,
  type StreamEventEnvelopeV2,
} from "@/lib/tauri";
import { Button } from "@/components/ui";
import { CheckCircle2, PauseCircle, PlayCircle, ShieldAlert, XCircle } from "lucide-react";

const ROLE_OPTIONS = ["orchestrator", "delegator", "worker", "watcher", "reviewer", "tester"];
const STATUS_FILTER_OPTIONS = ["all", "queued", "running", "completed", "failed", "cancelled"];

type RailEventTone = "neutral" | "running" | "success" | "danger" | "warning";

interface MissionRailEvent {
  id: string;
  missionId: string;
  instanceId?: string;
  parentInstanceId?: string | null;
  role?: string;
  status?: string;
  label: string;
  detail?: string;
  tone: RailEventTone;
  depth: number;
  tsMs?: number;
}

type InboxItem =
  | {
      id: string;
      kind: "spawn";
      createdAtMs: number;
      title: string;
      detail: string;
      approvalId: string;
    }
  | {
      id: string;
      kind: "tool";
      createdAtMs: number;
      title: string;
      detail: string;
      sessionId?: string | null;
      toolCallId?: string | null;
    };

function getStatusTone(status: string): RailEventTone {
  if (status === "running") return "running";
  if (status === "completed") return "success";
  if (status === "failed") return "danger";
  if (status === "cancelled") return "warning";
  return "neutral";
}

function toneClassName(tone: RailEventTone): string {
  if (tone === "running") return "border-cyan-400/40 bg-cyan-500/10";
  if (tone === "success") return "border-emerald-400/40 bg-emerald-500/10";
  if (tone === "danger") return "border-red-400/40 bg-red-500/10";
  if (tone === "warning") return "border-amber-400/40 bg-amber-500/10";
  return "border-border bg-surface/40";
}

function toneDotClassName(tone: RailEventTone): string {
  if (tone === "running") return "bg-cyan-300";
  if (tone === "success") return "bg-emerald-300";
  if (tone === "danger") return "bg-red-300";
  if (tone === "warning") return "bg-amber-300";
  return "bg-zinc-400";
}

function downloadTextFile(filename: string, content: string, mime: string): void {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

function normalizeApprovals(input: unknown): AgentTeamApprovals {
  const value = (input || {}) as Record<string, unknown>;
  const spawn = (value.spawn_approvals as unknown[]) || (value.spawnApprovals as unknown[]) || [];
  const tools = (value.tool_approvals as unknown[]) || (value.toolApprovals as unknown[]) || [];
  return {
    spawn_approvals: Array.isArray(spawn) ? (spawn as AgentTeamApprovals["spawn_approvals"]) : [],
    tool_approvals: Array.isArray(tools) ? (tools as AgentTeamApprovals["tool_approvals"]) : [],
  };
}

export function AgentCommandCenter() {
  const [templates, setTemplates] = useState<AgentTeamTemplate[]>([]);
  const [missions, setMissions] = useState<AgentTeamMissionSummary[]>([]);
  const [instances, setInstances] = useState<AgentTeamInstance[]>([]);
  const [approvals, setApprovals] = useState<AgentTeamApprovals>({
    spawn_approvals: [],
    tool_approvals: [],
  });
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [selectedRole, setSelectedRole] = useState("worker");
  const [spawnMode, setSpawnMode] = useState<"simple" | "advanced">("simple");
  const [simpleGoal, setSimpleGoal] = useState("");
  const [simpleRole, setSimpleRole] = useState("worker");
  const [selectedTemplate, setSelectedTemplate] = useState("");
  const [selectedMissionId, setSelectedMissionId] = useState("");
  const [selectedMissionDetailId, setSelectedMissionDetailId] = useState<string | null>(null);
  const [selectedInstanceDetailId, setSelectedInstanceDetailId] = useState<string | null>(null);
  const [justification, setJustification] = useState("Delegate focused task execution.");
  const [actionReason, setActionReason] = useState("Reviewed in command center.");
  const [searchQuery, setSearchQuery] = useState("");
  const [filterRole, setFilterRole] = useState("all");
  const [filterStatus, setFilterStatus] = useState("all");
  const [filterMission, setFilterMission] = useState("all");
  const [filterParent, setFilterParent] = useState("all");
  const [lastAgentEventAtMs, setLastAgentEventAtMs] = useState<number | null>(null);
  const refreshTimerRef = useRef<number | null>(null);

  const load = useCallback(async () => {
    try {
      const [nextTemplates, nextMissions, nextInstances, nextApprovals] = await Promise.all([
        agentTeamListTemplates(),
        agentTeamListMissions(),
        agentTeamListInstances(),
        agentTeamListApprovals(),
      ]);
      setTemplates(nextTemplates);
      setMissions(nextMissions);
      setInstances(nextInstances);
      setApprovals(normalizeApprovals(nextApprovals));
      setError(null);
      if (!selectedTemplate && nextTemplates.length > 0) {
        setSelectedTemplate(nextTemplates[0].template_id);
      }
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    }
  }, [selectedTemplate]);

  useEffect(() => {
    void load();
    const id = setInterval(() => {
      void load();
    }, 2500);
    return () => clearInterval(id);
  }, [load]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    const setup = async () => {
      unlisten = await onSidecarEventV2((envelope: StreamEventEnvelopeV2) => {
        const payload = envelope?.payload;
        if (!payload || payload.type !== "raw") {
          return;
        }
        if (!payload.event_type.startsWith("agent_team.")) {
          return;
        }
        setLastAgentEventAtMs(Date.now());
        if (refreshTimerRef.current !== null) {
          return;
        }
        refreshTimerRef.current = window.setTimeout(() => {
          refreshTimerRef.current = null;
          void load();
        }, 250);
      });
    };
    void setup();
    return () => {
      if (refreshTimerRef.current !== null) {
        window.clearTimeout(refreshTimerRef.current);
        refreshTimerRef.current = null;
      }
      if (unlisten) {
        unlisten();
      }
    };
  }, [load]);

  const byStatus = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const instance of instances) {
      counts[instance.status] = (counts[instance.status] || 0) + 1;
    }
    return counts;
  }, [instances]);

  const selectedMission = useMemo(
    () =>
      missions.find(
        (mission) => mission.mission_id === (selectedMissionDetailId || selectedMissionId)
      ) || null,
    [missions, selectedMissionDetailId, selectedMissionId]
  );

  const selectedInstance = useMemo(
    () => instances.find((instance) => instance.instance_id === selectedInstanceDetailId) || null,
    [instances, selectedInstanceDetailId]
  );

  const availableRoles = useMemo(
    () => ["all", ...Array.from(new Set(instances.map((instance) => instance.role))).sort()],
    [instances]
  );

  const availableMissionIds = useMemo(
    () => ["all", ...Array.from(new Set(instances.map((instance) => instance.mission_id))).sort()],
    [instances]
  );

  const availableParents = useMemo(
    () => [
      "all",
      ...Array.from(
        new Set(
          instances
            .map((instance) => instance.parent_instance_id)
            .filter((parent): parent is string => Boolean(parent))
        )
      ).sort(),
    ],
    [instances]
  );

  const filteredInstances = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    return instances.filter((instance) => {
      if (filterRole !== "all" && instance.role !== filterRole) {
        return false;
      }
      if (filterStatus !== "all" && instance.status !== filterStatus) {
        return false;
      }
      if (filterMission !== "all" && instance.mission_id !== filterMission) {
        return false;
      }
      if (filterParent !== "all" && (instance.parent_instance_id || "") !== filterParent) {
        return false;
      }
      if (!query) {
        return true;
      }
      const haystack = [
        instance.instance_id,
        instance.mission_id,
        instance.session_id,
        instance.role,
        instance.status,
        instance.parent_instance_id || "",
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(query);
    });
  }, [filterMission, filterParent, filterRole, filterStatus, instances, searchQuery]);

  const filteredMissions = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    return missions.filter((mission) => {
      if (filterMission !== "all" && mission.mission_id !== filterMission) {
        return false;
      }
      if (!query) {
        return true;
      }
      return mission.mission_id.toLowerCase().includes(query);
    });
  }, [filterMission, missions, searchQuery]);

  const missionRailEvents = useMemo<MissionRailEvent[]>(() => {
    const missionId = selectedMission?.mission_id || selectedMissionDetailId || selectedMissionId;
    if (!missionId) {
      return [];
    }

    const missionInstances = instances.filter((instance) => instance.mission_id === missionId);
    const depthById = new Map<string, number>();
    const instanceById = new Map(
      missionInstances.map((instance) => [instance.instance_id, instance])
    );

    const getDepth = (instance: AgentTeamInstance): number => {
      const cached = depthById.get(instance.instance_id);
      if (cached !== undefined) {
        return cached;
      }
      let depth = 0;
      let cursor = instance.parent_instance_id || null;
      let guard = 0;
      while (cursor && instanceById.has(cursor) && guard < 32) {
        depth += 1;
        cursor = instanceById.get(cursor)?.parent_instance_id || null;
        guard += 1;
      }
      depthById.set(instance.instance_id, depth);
      return depth;
    };

    const events: MissionRailEvent[] = [];

    for (const instance of missionInstances) {
      const depth = getDepth(instance);
      const roleLabel = `${instance.role} ${instance.instance_id}`;
      events.push({
        id: `${instance.instance_id}:spawned`,
        missionId,
        instanceId: instance.instance_id,
        parentInstanceId: instance.parent_instance_id,
        role: instance.role,
        status: instance.status,
        label: `Spawned ${roleLabel}`,
        detail: instance.parent_instance_id
          ? `child of ${instance.parent_instance_id}`
          : "root instance",
        tone: "neutral",
        depth,
      });
      events.push({
        id: `${instance.instance_id}:status:${instance.status}`,
        missionId,
        instanceId: instance.instance_id,
        parentInstanceId: instance.parent_instance_id,
        role: instance.role,
        status: instance.status,
        label: `${roleLabel} is ${instance.status}`,
        tone: getStatusTone(instance.status),
        depth,
      });
    }

    const missionApprovals = approvals.spawn_approvals.filter((approval) => {
      const request = approval.request || {};
      const requestMission = String((request as Record<string, unknown>).missionID || "");
      return requestMission === missionId;
    });

    for (const approval of missionApprovals) {
      const request = approval.request || {};
      const role = String((request as Record<string, unknown>).role || "agent");
      events.push({
        id: `${approval.approval_id}:approval`,
        missionId,
        label: `Spawn approval pending for ${role}`,
        detail: approval.approval_id,
        tone: "warning",
        depth: 0,
        tsMs: approval.created_at_ms,
      });
    }

    const toolApprovalCount = approvals.tool_approvals.filter((approval) => {
      const sessionId = approval.session_id || "";
      return missionInstances.some((instance) => instance.session_id === sessionId);
    }).length;
    if (toolApprovalCount > 0) {
      events.push({
        id: `${missionId}:tool-approvals`,
        missionId,
        label: `${toolApprovalCount} tool approval(s) pending`,
        tone: "warning",
        depth: 0,
      });
    }

    return events.sort((a, b) => {
      if (a.tsMs && b.tsMs) return a.tsMs - b.tsMs;
      if (a.tsMs) return -1;
      if (b.tsMs) return 1;
      if (a.depth !== b.depth) return a.depth - b.depth;
      return a.label.localeCompare(b.label);
    });
  }, [
    approvals.spawn_approvals,
    approvals.tool_approvals,
    instances,
    selectedMission,
    selectedMissionDetailId,
    selectedMissionId,
  ]);

  const toolApprovals = approvals.tool_approvals;
  const inboxItems = useMemo<InboxItem[]>(() => {
    const spawnItems: InboxItem[] = approvals.spawn_approvals.map((approval) => {
      const request = approval.request || {};
      const role = String((request as Record<string, unknown>).role || "agent");
      const missionId = String((request as Record<string, unknown>).missionID || "new mission");
      return {
        id: `spawn:${approval.approval_id}`,
        kind: "spawn",
        createdAtMs: approval.created_at_ms,
        title: `Spawn ${role}`,
        detail: `mission ${missionId}`,
        approvalId: approval.approval_id,
      };
    });
    const toolItems: InboxItem[] = approvals.tool_approvals.map((approval) => ({
      id: `tool:${approval.approval_id}`,
      kind: "tool",
      createdAtMs: Date.now(),
      title: `Tool ${approval.tool || "request"}`,
      detail: approval.session_id ? `session ${approval.session_id}` : "missing session binding",
      sessionId: approval.session_id,
      toolCallId: approval.tool_call_id,
    }));
    return [...spawnItems, ...toolItems].sort((a, b) => b.createdAtMs - a.createdAtMs);
  }, [approvals.spawn_approvals, approvals.tool_approvals]);
  const sseFresh = lastAgentEventAtMs ? Date.now() - lastAgentEventAtMs < 15000 : false;
  const refreshMode = sseFresh ? "SSE + polling fallback" : "Polling fallback";

  const handleSpawn = async () => {
    const effectiveRole = spawnMode === "simple" ? simpleRole : selectedRole;
    const effectiveJustification =
      spawnMode === "simple"
        ? simpleGoal.trim()
          ? `User goal: ${simpleGoal.trim()}`
          : ""
        : justification.trim();

    if (!effectiveJustification) {
      setError("Spawn requires a short justification.");
      return;
    }
    setIsLoading(true);
    try {
      const result = await agentTeamSpawn({
        role: effectiveRole,
        template_id: selectedTemplate || undefined,
        mission_id: selectedMissionId || undefined,
        justification: effectiveJustification,
        source: "desktop_ui",
      });
      if (!result.ok) {
        throw new Error(result.error || result.code || "Spawn denied");
      }
      await load();
      setError(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleApprove = async (approvalId: string) => {
    setIsLoading(true);
    try {
      await agentTeamApproveSpawn(approvalId, actionReason);
      await load();
      setError(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleDeny = async (approvalId: string) => {
    setIsLoading(true);
    try {
      await agentTeamDenySpawn(approvalId, actionReason);
      await load();
      setError(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleCancelInstance = async (instanceId: string) => {
    setIsLoading(true);
    try {
      await agentTeamCancelInstance(instanceId, actionReason);
      await load();
      setError(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleCancelMission = async (missionId: string) => {
    setIsLoading(true);
    try {
      await agentTeamCancelMission(missionId, actionReason);
      await load();
      setError(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleApproveTool = async (sessionId: string, toolCallId: string) => {
    setIsLoading(true);
    try {
      await approveTool(sessionId, toolCallId);
      await load();
      setError(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleDenyTool = async (sessionId: string, toolCallId: string) => {
    setIsLoading(true);
    try {
      await denyTool(sessionId, toolCallId);
      await load();
      setError(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleExportMissionReportJson = () => {
    const targetMissionId =
      selectedMission?.mission_id || selectedMissionDetailId || selectedMissionId;
    if (!targetMissionId) {
      setError("Select a mission before exporting a report.");
      return;
    }
    const mission = missions.find((item) => item.mission_id === targetMissionId);
    if (!mission) {
      setError("Mission data not loaded.");
      return;
    }
    const missionInstances = instances.filter(
      (instance) => instance.mission_id === targetMissionId
    );
    const missionSpawnApprovals = approvals.spawn_approvals.filter((approval) => {
      const request = approval.request || {};
      return String((request as Record<string, unknown>).missionID || "") === targetMissionId;
    });
    const missionToolApprovals = approvals.tool_approvals.filter((approval) => {
      const sessionId = approval.session_id || "";
      return missionInstances.some((instance) => instance.session_id === sessionId);
    });
    const payload = {
      generatedAt: new Date().toISOString(),
      mission,
      instances: missionInstances,
      spawnApprovals: missionSpawnApprovals,
      toolApprovals: missionToolApprovals,
      activityRail: missionRailEvents,
    };
    downloadTextFile(
      `agent-mission-${targetMissionId}-report.json`,
      JSON.stringify(payload, null, 2),
      "application/json"
    );
  };

  const handleExportMissionReportMarkdown = () => {
    const targetMissionId =
      selectedMission?.mission_id || selectedMissionDetailId || selectedMissionId;
    if (!targetMissionId) {
      setError("Select a mission before exporting a report.");
      return;
    }
    const mission = missions.find((item) => item.mission_id === targetMissionId);
    if (!mission) {
      setError("Mission data not loaded.");
      return;
    }
    const missionInstances = instances.filter(
      (instance) => instance.mission_id === targetMissionId
    );
    const lines: string[] = [];
    lines.push(`# Agent Mission Report: ${targetMissionId}`);
    lines.push("");
    lines.push(`Generated: ${new Date().toISOString()}`);
    lines.push("");
    lines.push("## Summary");
    lines.push("");
    lines.push(`- Instances: ${mission.instance_count}`);
    lines.push(`- Running: ${mission.running_count}`);
    lines.push(`- Completed: ${mission.completed_count}`);
    lines.push(`- Failed: ${mission.failed_count}`);
    lines.push(`- Cancelled: ${mission.cancelled_count}`);
    lines.push(`- Tokens used: ${mission.token_used_total}`);
    lines.push(`- Tool calls used: ${mission.tool_calls_used_total}`);
    lines.push(`- Steps used: ${mission.steps_used_total}`);
    lines.push(`- Cost used (USD): ${mission.cost_used_usd_total.toFixed(4)}`);
    lines.push("");
    lines.push("## Instances");
    lines.push("");
    if (missionInstances.length === 0) {
      lines.push("- (none)");
    } else {
      for (const instance of missionInstances) {
        lines.push(
          `- \`${instance.instance_id}\` role=${instance.role} status=${instance.status} session=\`${instance.session_id}\` parent=\`${instance.parent_instance_id || "-"}\``
        );
      }
    }
    lines.push("");
    lines.push("## Activity Rail");
    lines.push("");
    if (missionRailEvents.length === 0) {
      lines.push("- (no activity events captured)");
    } else {
      for (const event of missionRailEvents) {
        lines.push(`- ${event.label}${event.detail ? ` - ${event.detail}` : ""}`);
      }
    }
    lines.push("");
    downloadTextFile(
      `agent-mission-${targetMissionId}-report.md`,
      lines.join("\n"),
      "text/markdown"
    );
  };

  return (
    <div className="rounded-xl border border-cyan-500/30 bg-gradient-to-br from-cyan-950/30 via-surface-elevated to-indigo-950/30 p-4 space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <div className="text-xs uppercase tracking-wide text-cyan-300">Agent Command Center</div>
          <div className="text-sm text-text-muted">
            Live mission graph, approvals, and controlled spawn operations.
          </div>
        </div>
        <Button variant="secondary" size="sm" onClick={() => void load()} disabled={isLoading}>
          Refresh
        </Button>
      </div>

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <div className="rounded-lg border border-border bg-surface/60 p-2">
          <div className="text-[10px] uppercase text-text-subtle">Missions</div>
          <div className="text-lg font-semibold text-text">{missions.length}</div>
        </div>
        <div className="rounded-lg border border-border bg-surface/60 p-2">
          <div className="text-[10px] uppercase text-text-subtle">Instances</div>
          <div className="text-lg font-semibold text-text">{instances.length}</div>
        </div>
        <div className="rounded-lg border border-border bg-surface/60 p-2">
          <div className="text-[10px] uppercase text-text-subtle">Running</div>
          <div className="text-lg font-semibold text-emerald-300">{byStatus.running || 0}</div>
        </div>
        <div className="rounded-lg border border-border bg-surface/60 p-2">
          <div className="text-[10px] uppercase text-text-subtle">Spawn Approvals</div>
          <div className="text-lg font-semibold text-amber-300">
            {approvals.spawn_approvals.length}
          </div>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="text-xs uppercase tracking-wide text-text-subtle">Health Strip</div>
          <div className="text-[11px] text-text-muted">{refreshMode}</div>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-2 text-xs">
          <span
            className={`rounded-full border px-2 py-1 ${
              sseFresh
                ? "border-emerald-400/50 bg-emerald-500/10 text-emerald-200"
                : "border-amber-400/50 bg-amber-500/10 text-amber-200"
            }`}
          >
            SSE {sseFresh ? "connected" : "idle"}
          </span>
          <span className="rounded-full border border-border px-2 py-1 text-text-muted">
            last event{" "}
            {lastAgentEventAtMs ? new Date(lastAgentEventAtMs).toLocaleTimeString() : "none yet"}
          </span>
          <span className="rounded-full border border-border px-2 py-1 text-text-muted">
            auto refresh: 2.5s
          </span>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="text-xs uppercase tracking-wide text-text-subtle">Spawn Agent</div>
          <div className="flex gap-2">
            <button
              className={`rounded-full border px-2 py-1 text-[11px] ${
                spawnMode === "simple"
                  ? "border-cyan-400/60 bg-cyan-500/10 text-cyan-100"
                  : "border-border text-text-muted"
              }`}
              onClick={() => setSpawnMode("simple")}
            >
              Simple
            </button>
            <button
              className={`rounded-full border px-2 py-1 text-[11px] ${
                spawnMode === "advanced"
                  ? "border-cyan-400/60 bg-cyan-500/10 text-cyan-100"
                  : "border-border text-text-muted"
              }`}
              onClick={() => setSpawnMode("advanced")}
            >
              Advanced
            </button>
          </div>
        </div>
        {spawnMode === "simple" ? (
          <div className="space-y-2">
            <input
              className="w-full rounded border border-border bg-surface p-2 text-sm text-text"
              value={simpleGoal}
              onChange={(e) => setSimpleGoal(e.target.value)}
              placeholder="What should this agent do?"
            />
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
              <button
                className={`rounded border p-2 text-left text-xs ${
                  simpleRole === "worker"
                    ? "border-cyan-400/60 bg-cyan-500/10 text-cyan-100"
                    : "border-border text-text-muted"
                }`}
                onClick={() => setSimpleRole("worker")}
              >
                Worker
              </button>
              <button
                className={`rounded border p-2 text-left text-xs ${
                  simpleRole === "reviewer"
                    ? "border-cyan-400/60 bg-cyan-500/10 text-cyan-100"
                    : "border-border text-text-muted"
                }`}
                onClick={() => setSimpleRole("reviewer")}
              >
                Reviewer
              </button>
              <button
                className={`rounded border p-2 text-left text-xs ${
                  simpleRole === "tester"
                    ? "border-cyan-400/60 bg-cyan-500/10 text-cyan-100"
                    : "border-border text-text-muted"
                }`}
                onClick={() => setSimpleRole("tester")}
              >
                Tester
              </button>
            </div>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              <select
                className="rounded border border-border bg-surface p-2 text-sm text-text"
                value={selectedMissionId}
                onChange={(e) => setSelectedMissionId(e.target.value)}
              >
                <option value="">new mission</option>
                {missions.map((mission) => (
                  <option key={mission.mission_id} value={mission.mission_id}>
                    {mission.mission_id}
                  </option>
                ))}
              </select>
              <Button onClick={() => void handleSpawn()} disabled={isLoading || !simpleGoal.trim()}>
                <PlayCircle className="mr-1 h-4 w-4" />
                Launch Agent
              </Button>
            </div>
          </div>
        ) : (
          <div className="space-y-2">
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              <select
                className="rounded border border-border bg-surface p-2 text-sm text-text"
                value={selectedRole}
                onChange={(e) => setSelectedRole(e.target.value)}
              >
                {ROLE_OPTIONS.map((role) => (
                  <option key={role} value={role}>
                    {role}
                  </option>
                ))}
              </select>
              <select
                className="rounded border border-border bg-surface p-2 text-sm text-text"
                value={selectedTemplate}
                onChange={(e) => setSelectedTemplate(e.target.value)}
              >
                <option value="">auto-template</option>
                {templates.map((template) => (
                  <option key={template.template_id} value={template.template_id}>
                    {template.template_id} ({template.role})
                  </option>
                ))}
              </select>
              <select
                className="rounded border border-border bg-surface p-2 text-sm text-text"
                value={selectedMissionId}
                onChange={(e) => setSelectedMissionId(e.target.value)}
              >
                <option value="">new mission</option>
                {missions.map((mission) => (
                  <option key={mission.mission_id} value={mission.mission_id}>
                    {mission.mission_id}
                  </option>
                ))}
              </select>
              <Button
                onClick={() => void handleSpawn()}
                disabled={isLoading || !justification.trim()}
              >
                <PlayCircle className="mr-1 h-4 w-4" />
                Spawn
              </Button>
            </div>
            <input
              className="w-full rounded border border-border bg-surface p-2 text-sm text-text"
              value={justification}
              onChange={(e) => setJustification(e.target.value)}
              placeholder="Why this spawn is needed"
            />
          </div>
        )}
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-2">
        <div className="flex items-center justify-between">
          <div className="text-xs uppercase tracking-wide text-text-subtle">
            Pending Spawn Approvals
          </div>
          <input
            className="w-56 rounded border border-border bg-surface p-1.5 text-xs text-text"
            value={actionReason}
            onChange={(e) => setActionReason(e.target.value)}
            placeholder="approval note"
          />
        </div>
        {approvals.spawn_approvals.length === 0 ? (
          <div className="text-xs text-text-muted">No pending spawn approvals.</div>
        ) : (
          <div className="space-y-2">
            {approvals.spawn_approvals.map((approval) => {
              const request = approval.request || {};
              const role = String((request as Record<string, unknown>).role || "unknown");
              const missionId = String((request as Record<string, unknown>).missionID || "new");
              return (
                <div
                  key={approval.approval_id}
                  className="rounded border border-amber-500/30 bg-amber-500/10 p-2"
                >
                  <div className="flex items-center justify-between text-xs text-amber-100">
                    <span>{approval.approval_id}</span>
                    <span>{new Date(approval.created_at_ms).toLocaleTimeString()}</span>
                  </div>
                  <div className="mt-1 text-sm text-text">
                    {role} on mission <span className="font-mono text-xs">{missionId}</span>
                  </div>
                  <div className="mt-2 flex gap-2">
                    <Button size="sm" onClick={() => void handleApprove(approval.approval_id)}>
                      <CheckCircle2 className="mr-1 h-4 w-4" />
                      Approve
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      onClick={() => void handleDeny(approval.approval_id)}
                    >
                      <XCircle className="mr-1 h-4 w-4" />
                      Deny
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-2">
        <div className="flex items-center justify-between">
          <div className="text-xs uppercase tracking-wide text-text-subtle">
            Operator Approvals Inbox
          </div>
          <div className="text-[11px] text-text-muted">{inboxItems.length} pending</div>
        </div>
        {inboxItems.length === 0 ? (
          <div className="text-xs text-text-muted">No pending approvals.</div>
        ) : (
          <div className="space-y-2 max-h-48 overflow-y-auto">
            {inboxItems.map((item) => (
              <div key={item.id} className="rounded border border-border bg-surface/60 p-2">
                <div className="flex items-center justify-between">
                  <div className="text-xs text-text">
                    <span
                      className={`mr-2 rounded-full border px-1.5 py-0.5 text-[10px] uppercase ${
                        item.kind === "spawn"
                          ? "border-amber-400/50 text-amber-200"
                          : "border-rose-400/50 text-rose-200"
                      }`}
                    >
                      {item.kind}
                    </span>
                    {item.title}
                  </div>
                </div>
                <div className="mt-1 text-[11px] text-text-muted">{item.detail}</div>
                <div className="mt-2 flex gap-2">
                  {item.kind === "spawn" ? (
                    <>
                      <Button size="sm" onClick={() => void handleApprove(item.approvalId)}>
                        <CheckCircle2 className="mr-1 h-4 w-4" />
                        Approve
                      </Button>
                      <Button
                        size="sm"
                        variant="danger"
                        onClick={() => void handleDeny(item.approvalId)}
                      >
                        <XCircle className="mr-1 h-4 w-4" />
                        Deny
                      </Button>
                    </>
                  ) : item.sessionId && item.toolCallId ? (
                    <>
                      <Button
                        size="sm"
                        onClick={() => void handleApproveTool(item.sessionId!, item.toolCallId!)}
                      >
                        <CheckCircle2 className="mr-1 h-4 w-4" />
                        Approve Tool
                      </Button>
                      <Button
                        size="sm"
                        variant="danger"
                        onClick={() => void handleDenyTool(item.sessionId!, item.toolCallId!)}
                      >
                        <XCircle className="mr-1 h-4 w-4" />
                        Deny Tool
                      </Button>
                    </>
                  ) : (
                    <div className="text-[11px] text-text-muted">
                      Missing session/tool IDs, use request center fallback.
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-3">
        <div className="text-xs uppercase tracking-wide text-text-subtle">Search And Filters</div>
        <input
          className="w-full rounded border border-border bg-surface p-2 text-sm text-text"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder="Search mission / instance / session / role / status"
        />
        <div className="space-y-2">
          <div className="text-[10px] uppercase text-text-subtle">Role</div>
          <div className="flex flex-wrap gap-2">
            {availableRoles.map((role) => (
              <button
                key={role}
                className={`rounded-full border px-2 py-1 text-[11px] ${
                  filterRole === role
                    ? "border-cyan-400/60 bg-cyan-500/10 text-cyan-100"
                    : "border-border text-text-muted"
                }`}
                onClick={() => setFilterRole(role)}
              >
                {role}
              </button>
            ))}
          </div>
          <div className="text-[10px] uppercase text-text-subtle">Status</div>
          <div className="flex flex-wrap gap-2">
            {STATUS_FILTER_OPTIONS.map((status) => (
              <button
                key={status}
                className={`rounded-full border px-2 py-1 text-[11px] ${
                  filterStatus === status
                    ? "border-cyan-400/60 bg-cyan-500/10 text-cyan-100"
                    : "border-border text-text-muted"
                }`}
                onClick={() => setFilterStatus(status)}
              >
                {status}
              </button>
            ))}
          </div>
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <select
              className="rounded border border-border bg-surface p-2 text-xs text-text"
              value={filterMission}
              onChange={(e) => setFilterMission(e.target.value)}
            >
              {availableMissionIds.map((missionId) => (
                <option key={missionId} value={missionId}>
                  {missionId === "all" ? "all missions" : missionId}
                </option>
              ))}
            </select>
            <select
              className="rounded border border-border bg-surface p-2 text-xs text-text"
              value={filterParent}
              onChange={(e) => setFilterParent(e.target.value)}
            >
              {availableParents.map((parentId) => (
                <option key={parentId} value={parentId}>
                  {parentId === "all" ? "all parents" : parentId}
                </option>
              ))}
            </select>
          </div>
          <div className="text-[11px] text-text-muted">
            showing {filteredMissions.length}/{missions.length} missions and{" "}
            {filteredInstances.length}/{instances.length} instances
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-2">
          <div className="text-xs uppercase tracking-wide text-text-subtle">Missions</div>
          <div className="space-y-2 max-h-56 overflow-y-auto">
            {filteredMissions.length === 0 ? (
              <div className="text-xs text-text-muted">No missions yet.</div>
            ) : (
              filteredMissions.map((mission) => (
                <div
                  key={mission.mission_id}
                  className={`rounded border p-2 cursor-pointer ${
                    selectedMission?.mission_id === mission.mission_id
                      ? "border-cyan-400/60 bg-cyan-500/10"
                      : "border-border"
                  }`}
                  onClick={() => setSelectedMissionDetailId(mission.mission_id)}
                >
                  <div className="flex items-center justify-between">
                    <div className="font-mono text-xs text-text">{mission.mission_id}</div>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={(e) => {
                        e.stopPropagation();
                        void handleCancelMission(mission.mission_id);
                      }}
                    >
                      <PauseCircle className="mr-1 h-4 w-4" />
                      Cancel
                    </Button>
                  </div>
                  <div className="mt-1 text-xs text-text-muted">
                    total {mission.instance_count} | running {mission.running_count} | done{" "}
                    {mission.completed_count} | failed {mission.failed_count}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>

        <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-2">
          <div className="text-xs uppercase tracking-wide text-text-subtle">Instances</div>
          <div className="space-y-2 max-h-56 overflow-y-auto">
            {filteredInstances.length === 0 ? (
              <div className="text-xs text-text-muted">No instances yet.</div>
            ) : (
              filteredInstances.map((instance) => (
                <div
                  key={instance.instance_id}
                  className={`rounded border p-2 cursor-pointer ${
                    selectedInstance?.instance_id === instance.instance_id
                      ? "border-cyan-400/60 bg-cyan-500/10"
                      : "border-border"
                  }`}
                  onClick={() => setSelectedInstanceDetailId(instance.instance_id)}
                >
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-sm text-text">
                        {instance.role}{" "}
                        <span className="font-mono text-xs text-text-muted">
                          {instance.instance_id}
                        </span>
                      </div>
                      <div className="text-xs text-text-muted">
                        mission {instance.mission_id} | {instance.status}
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={(e) => {
                        e.stopPropagation();
                        void handleCancelInstance(instance.instance_id);
                      }}
                    >
                      <PauseCircle className="mr-1 h-4 w-4" />
                      Cancel
                    </Button>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </div>

      {(selectedMission || selectedInstance) && (
        <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
          <div className="rounded-lg border border-border bg-surface/50 p-3">
            <div className="text-xs uppercase tracking-wide text-text-subtle mb-2">
              Mission Drill-Down
            </div>
            {selectedMission ? (
              <div className="space-y-1 text-xs text-text">
                <div>
                  <span className="text-text-muted">mission:</span>{" "}
                  <span className="font-mono">{selectedMission.mission_id}</span>
                </div>
                <div>
                  running {selectedMission.running_count} / total {selectedMission.instance_count}
                </div>
                <div>
                  completed {selectedMission.completed_count} | failed{" "}
                  {selectedMission.failed_count} | cancelled {selectedMission.cancelled_count}
                </div>
                <div className="pt-1">
                  tokens {selectedMission.token_used_total} | toolCalls{" "}
                  {selectedMission.tool_calls_used_total} | steps {selectedMission.steps_used_total}
                </div>
                <div>cost ${selectedMission.cost_used_usd_total.toFixed(4)}</div>
              </div>
            ) : (
              <div className="text-xs text-text-muted">Select a mission to inspect details.</div>
            )}
          </div>

          <div className="rounded-lg border border-border bg-surface/50 p-3">
            <div className="text-xs uppercase tracking-wide text-text-subtle mb-2">
              Instance Drill-Down
            </div>
            {selectedInstance ? (
              <div className="space-y-2 text-xs text-text">
                <div>
                  <span className="text-text-muted">instance:</span>{" "}
                  <span className="font-mono">{selectedInstance.instance_id}</span>
                </div>
                <div>
                  role {selectedInstance.role} | status {selectedInstance.status}
                </div>
                <div>
                  mission <span className="font-mono">{selectedInstance.mission_id}</span> | session{" "}
                  <span className="font-mono">{selectedInstance.session_id}</span>
                </div>
                <div className="text-text-muted">budget</div>
                <pre className="overflow-x-auto rounded border border-border bg-surface p-2 text-[11px]">
                  {JSON.stringify(selectedInstance.budget, null, 2)}
                </pre>
                <div className="text-text-muted">capabilities</div>
                <pre className="overflow-x-auto rounded border border-border bg-surface p-2 text-[11px]">
                  {JSON.stringify(selectedInstance.capabilities || {}, null, 2)}
                </pre>
              </div>
            ) : (
              <div className="text-xs text-text-muted">Select an instance to inspect details.</div>
            )}
          </div>
        </div>
      )}

      <div className="rounded-lg border border-border bg-surface/50 p-3">
        <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
          <div className="text-xs uppercase tracking-wide text-text-subtle">
            Mission Activity Rail
          </div>
          <div className="flex gap-2">
            <Button size="sm" variant="secondary" onClick={handleExportMissionReportJson}>
              Export JSON
            </Button>
            <Button size="sm" variant="secondary" onClick={handleExportMissionReportMarkdown}>
              Export MD
            </Button>
          </div>
        </div>
        {missionRailEvents.length === 0 ? (
          <div className="text-xs text-text-muted">
            Select a mission to see spawn chain, status transitions, failures, and cancellations.
          </div>
        ) : (
          <div className="space-y-2 max-h-64 overflow-y-auto pr-1">
            {missionRailEvents.map((event) => (
              <div
                key={event.id}
                className={`rounded border p-2 ${toneClassName(event.tone)}`}
                style={{ marginLeft: `${Math.min(event.depth, 6) * 14}px` }}
              >
                <div className="flex items-center gap-2 text-xs text-text">
                  <span
                    className={`inline-block h-2 w-2 rounded-full ${toneDotClassName(event.tone)}`}
                  />
                  <span>{event.label}</span>
                  {event.status ? (
                    <span className="text-[10px] uppercase text-text-muted">{event.status}</span>
                  ) : null}
                </div>
                {event.detail ? (
                  <div className="mt-1 text-[11px] text-text-muted">{event.detail}</div>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </div>

      {toolApprovals.length > 0 && (
        <div className="rounded-lg border border-rose-500/30 bg-rose-500/10 p-3 space-y-2">
          <div className="flex items-center gap-2 text-rose-200 text-sm">
            <ShieldAlert className="h-4 w-4" />
            Tool approvals pending: {toolApprovals.length}
          </div>
          <div className="space-y-2">
            {toolApprovals.map((approval) => (
              <div
                key={approval.approval_id}
                className="rounded border border-rose-500/30 bg-black/10 p-2"
              >
                <div className="text-xs text-rose-100">
                  {approval.tool || "tool"}{" "}
                  {approval.session_id ? (
                    <>
                      in <span className="font-mono">{approval.session_id}</span>
                    </>
                  ) : null}
                </div>
                {approval.session_id && approval.tool_call_id ? (
                  <div className="mt-2 flex gap-2">
                    <Button
                      size="sm"
                      onClick={() =>
                        void handleApproveTool(approval.session_id!, approval.tool_call_id)
                      }
                    >
                      <CheckCircle2 className="mr-1 h-4 w-4" />
                      Approve Tool
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      onClick={() =>
                        void handleDenyTool(approval.session_id!, approval.tool_call_id)
                      }
                    >
                      <XCircle className="mr-1 h-4 w-4" />
                      Deny Tool
                    </Button>
                  </div>
                ) : (
                  <div className="mt-1 text-xs text-rose-100/80">
                    Missing `sessionID`/`toolCallID` in approval payload; use request center
                    fallback.
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {error && (
        <div className="rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
          {error}
        </div>
      )}
    </div>
  );
}
