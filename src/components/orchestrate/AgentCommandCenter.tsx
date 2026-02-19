import { useCallback, useEffect, useMemo, useState } from "react";
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
  type AgentTeamApprovals,
  type AgentTeamInstance,
  type AgentTeamMissionSummary,
  type AgentTeamTemplate,
} from "@/lib/tauri";
import { Button } from "@/components/ui";
import { CheckCircle2, PauseCircle, PlayCircle, ShieldAlert, XCircle } from "lucide-react";

const ROLE_OPTIONS = ["orchestrator", "delegator", "worker", "watcher", "reviewer", "tester"];

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
  const [selectedTemplate, setSelectedTemplate] = useState("");
  const [selectedMissionId, setSelectedMissionId] = useState("");
  const [selectedMissionDetailId, setSelectedMissionDetailId] = useState<string | null>(null);
  const [selectedInstanceDetailId, setSelectedInstanceDetailId] = useState<string | null>(null);
  const [justification, setJustification] = useState("Delegate focused task execution.");
  const [actionReason, setActionReason] = useState("Reviewed in command center.");

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
      setApprovals(nextApprovals);
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
    () =>
      instances.find((instance) => instance.instance_id === selectedInstanceDetailId) || null,
    [instances, selectedInstanceDetailId]
  );

  const toolApprovals = useMemo(() => {
    const getString = (obj: Record<string, unknown>, keys: string[]): string | null => {
      for (const key of keys) {
        const value = obj[key];
        if (typeof value === "string" && value.trim()) {
          return value;
        }
      }
      return null;
    };

    return approvals.tool_approvals.map((raw, index) => {
      const obj = (raw ?? {}) as Record<string, unknown>;
      const sessionId =
        getString(obj, ["sessionID", "sessionId", "session_id"]) ??
        getString((obj.request as Record<string, unknown>) || {}, [
          "sessionID",
          "sessionId",
          "session_id",
        ]);
      const toolCallId =
        getString(obj, ["toolCallID", "toolCallId", "tool_call_id", "callID", "id"]) ??
        getString((obj.tool as Record<string, unknown>) || {}, [
          "callID",
          "toolCallID",
          "toolCallId",
          "id",
        ]);
      const toolName =
        getString(obj, ["tool", "toolName", "name"]) ??
        getString((obj.tool as Record<string, unknown>) || {}, ["name", "tool"]);

      return {
        id: getString(obj, ["id", "requestID", "requestId"]) || `tool-approval-${index}`,
        sessionId,
        toolCallId,
        toolName: toolName || "tool",
        raw: obj,
      };
    });
  }, [approvals.tool_approvals]);

  const handleSpawn = async () => {
    if (!justification.trim()) {
      setError("Spawn requires a short justification.");
      return;
    }
    setIsLoading(true);
    try {
      const result = await agentTeamSpawn({
        role: selectedRole,
        template_id: selectedTemplate || undefined,
        mission_id: selectedMissionId || undefined,
        justification: justification.trim(),
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
          <div className="text-lg font-semibold text-amber-300">{approvals.spawn_approvals.length}</div>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-2">
        <div className="text-xs uppercase tracking-wide text-text-subtle">Spawn Agent</div>
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
          <Button onClick={() => void handleSpawn()} disabled={isLoading || !justification.trim()}>
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

      <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-2">
        <div className="flex items-center justify-between">
          <div className="text-xs uppercase tracking-wide text-text-subtle">Pending Spawn Approvals</div>
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

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        <div className="rounded-lg border border-border bg-surface/50 p-3 space-y-2">
          <div className="text-xs uppercase tracking-wide text-text-subtle">Missions</div>
          <div className="space-y-2 max-h-56 overflow-y-auto">
            {missions.length === 0 ? (
              <div className="text-xs text-text-muted">No missions yet.</div>
            ) : (
              missions.map((mission) => (
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
                      onClick={() => void handleCancelMission(mission.mission_id)}
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
            {instances.length === 0 ? (
              <div className="text-xs text-text-muted">No instances yet.</div>
            ) : (
              instances.map((instance) => (
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
                        <span className="font-mono text-xs text-text-muted">{instance.instance_id}</span>
                      </div>
                      <div className="text-xs text-text-muted">
                        mission {instance.mission_id} | {instance.status}
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => void handleCancelInstance(instance.instance_id)}
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
                  completed {selectedMission.completed_count} | failed {selectedMission.failed_count}{" "}
                  | cancelled {selectedMission.cancelled_count}
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

      {toolApprovals.length > 0 && (
        <div className="rounded-lg border border-rose-500/30 bg-rose-500/10 p-3 space-y-2">
          <div className="flex items-center gap-2 text-rose-200 text-sm">
            <ShieldAlert className="h-4 w-4" />
            Tool approvals pending: {toolApprovals.length}
          </div>
          <div className="space-y-2">
            {toolApprovals.map((approval) => (
              <div key={approval.id} className="rounded border border-rose-500/30 bg-black/10 p-2">
                <div className="text-xs text-rose-100">
                  {approval.toolName}{" "}
                  {approval.sessionId ? (
                    <>
                      in <span className="font-mono">{approval.sessionId}</span>
                    </>
                  ) : null}
                </div>
                {approval.sessionId && approval.toolCallId ? (
                  <div className="mt-2 flex gap-2">
                    <Button
                      size="sm"
                      onClick={() => void handleApproveTool(approval.sessionId!, approval.toolCallId!)}
                    >
                      <CheckCircle2 className="mr-1 h-4 w-4" />
                      Approve Tool
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      onClick={() => void handleDenyTool(approval.sessionId!, approval.toolCallId!)}
                    >
                      <XCircle className="mr-1 h-4 w-4" />
                      Deny Tool
                    </Button>
                  </div>
                ) : (
                  <div className="mt-1 text-xs text-rose-100/80">
                    Missing `sessionID`/`toolCallID` in approval payload; use request center fallback.
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

