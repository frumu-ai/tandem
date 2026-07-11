import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  AnimatedPage,
  Badge,
  LoadingState,
  PanelCard,
  SearchInput,
  StatusPulse,
  Toolbar,
} from "../ui/index.tsx";
import { RunTimeline, useRunTimeline } from "../features/runs/RunTimeline";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";
import { Icon, type IconName } from "../ui/Icon";

type EvidenceTone = "ok" | "warn" | "info" | "ghost" | "err";

type EvidenceStep = {
  id: string;
  label: string;
  icon: IconName;
  tone: EvidenceTone;
  headline: string;
  detail: string;
  count?: number;
};

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function safeString(value: any, fallback = "") {
  const text = String(value ?? "").trim();
  return text || fallback;
}

function normalizeStatus(value: any) {
  return safeString(value, "unknown")
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[\s-]+/g, "_")
    .toLowerCase();
}

function titleCase(value: any) {
  return safeString(value, "unknown")
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (match) => match.toUpperCase());
}

function runIdOf(row: any) {
  return safeString(row?.run_id || row?.runId || row?.id || row?.run?.run_id || row?.run?.id);
}

function optionalError(data: unknown): string {
  return data && typeof data === "object" && "error" in data
    ? safeString((data as { error?: unknown }).error)
    : "";
}

function timestampMs(row: any) {
  const raw =
    row?.updated_at_ms ||
    row?.created_at_ms ||
    row?.timestamp_ms ||
    row?.ts_ms ||
    row?.requested_at_ms ||
    row?.decided_at_ms ||
    row?.finished_at_ms ||
    row?.started_at_ms ||
    0;
  const value = Number(raw);
  if (Number.isFinite(value) && value > 0) return value;
  const parsed = Date.parse(String(raw || ""));
  return Number.isFinite(parsed) ? parsed : 0;
}

function formatTime(ms: number) {
  if (!ms) return "n/a";
  return new Date(ms).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function relativeTime(ms: number) {
  if (!ms) return "n/a";
  const seconds = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

function sortedByRecent(rows: any[]) {
  return [...rows].sort((a, b) => timestampMs(b) - timestampMs(a));
}

function evidenceToneForStatus(status: any): EvidenceTone {
  const normalized = normalizeStatus(status);
  if (["succeeded", "completed", "executed", "approved", "allow", "allowed"].includes(normalized)) {
    return "ok";
  }
  if (["pending", "queued", "running", "awaiting_approval", "approval_required"].includes(normalized)) {
    return "warn";
  }
  if (["failed", "blocked", "denied", "deny", "rejected", "cancelled", "canceled"].includes(normalized)) {
    return "err";
  }
  return "ghost";
}

function badgeTone(tone: EvidenceTone): "ok" | "warn" | "err" | "info" | "ghost" {
  return tone === "err" ? "err" : tone;
}

function toneLabel(tone: EvidenceTone) {
  if (tone === "ok") return "Ready";
  if (tone === "warn") return "Review";
  if (tone === "err") return "Blocked";
  if (tone === "info") return "Live";
  return "Waiting";
}

function sourceLabel(source: string) {
  if (source === "aca") return "Action firewall";
  if (source === "automation_v2") return "Automation v2";
  if (source === "context") return "Context run";
  return "Run";
}

function matchesRunId(row: any, ids: Set<string>) {
  const candidates = [
    runIdOf(row),
    row?.aca_run_id,
    row?.tandem_run_id,
    row?.coder_run_id,
    row?.context_run_id,
    row?.source_run_id,
    row?.source_context_run_id,
    row?.run?.run_id,
    row?.payload?.run_id,
    row?.payload?.runID,
    row?.record?.run_id,
    row?.record?.runId,
    row?.metadata?.run_id,
    row?.linkage?.run_id,
  ]
    .map((value) => safeString(value))
    .filter(Boolean);
  return candidates.some((candidate) => ids.has(candidate));
}

function buildRunIdSet(runId: string, contextRunId: string) {
  const ids = new Set<string>();
  const trimmed = safeString(runId);
  const context = safeString(contextRunId);
  if (trimmed) {
    ids.add(trimmed);
    ids.add(`automation-v2-${trimmed}`);
    if (trimmed.startsWith("automation-v2-")) ids.add(trimmed.replace(/^automation-v2-/, ""));
  }
  if (context) {
    ids.add(context);
    if (context.startsWith("automation-v2-")) ids.add(context.replace(/^automation-v2-/, ""));
  }
  return ids;
}

function candidateContextRunIds(runId: string, selectedRun: any) {
  const ids = [
    selectedRun?.context_run_id,
    selectedRun?.contextRunId,
    selectedRun?.source_context_run_id,
    selectedRun?.sourceContextRunId,
    runId,
    runId ? `automation-v2-${runId}` : "",
  ];
  if (runId.startsWith("automation-v2-")) ids.push(runId.replace(/^automation-v2-/, ""));
  return ids.map((id) => safeString(id)).filter(Boolean);
}

function runObjective(run: any, contextRun: any, acaRun: any) {
  const task = acaRun?.blackboard?.task || acaRun?.snapshot?.blackboard?.task || {};
  const candidates = [
    task?.title,
    task?.summary,
    run?.mission_snapshot?.objective,
    run?.automation_snapshot?.goal,
    run?.automation_snapshot?.name,
    run?.trigger_reason,
    contextRun?.goal,
    contextRun?.objective,
    contextRun?.title,
  ];
  return safeString(candidates.find((candidate) => safeString(candidate)), "No goal captured");
}

function runStatus(run: any, contextRun: any, acaRun: any) {
  return safeString(
    acaRun?.status?.run?.status ||
      acaRun?.status ||
      run?.status ||
      contextRun?.status ||
      acaRun?.phase?.status,
    "unknown"
  );
}

function actionPreview(approval: any) {
  const payload = approval?.payload || approval?.surface_payload || {};
  const target = approval?.target || {};
  const body = safeString(payload?.body || payload?.summary || approval?.action_preview_markdown);
  const targetLabel = safeString(
    target?.base_repo || target?.identifier || payload?.repo || approval?.target_resource?.id
  );
  return body || targetLabel || safeString(approval?.action_type || approval?.request_type);
}

function memoryRunId(item: any) {
  return safeString(
    item?.run_id ||
      item?.runId ||
      item?.linkage?.run_id ||
      item?.linkage?.runId ||
      item?.metadata?.run_id ||
      item?.metadata?.runId ||
      item?.provenance?.run_id ||
      item?.provenance?.runId
  );
}

function memoryText(item: any) {
  return safeString(item?.text || item?.content || item?.value || item?.summary);
}

function toolRecord(row: any) {
  return row?.record || row?.payload?.record || row;
}

function policyDecisionRunId(row: any) {
  return safeString(row?.run_id || row?.runId || row?.context?.run_id || row?.context?.runId);
}

function policyDecisionLabel(row: any, fallback = "unknown") {
  return safeString(row?.decision || row?.effect || row?.policy_effect || row?.status, fallback);
}

function governanceReason(row: any) {
  return safeString(row?.reason || row?.reason_code || row?.message || row?.explanation);
}

function first<T>(rows: T[], fallback: T): T {
  return rows.length ? rows[0] : fallback;
}

function downloadJsonFile(filename: string, payload: unknown) {
  const blob = new Blob([JSON.stringify(payload, null, 2)], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

function safeDownloadName(value: string) {
  const sanitized = safeString(value)
    .replace(/[^a-zA-Z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 96);
  return sanitized || "run";
}

export function ControlLoopPage({ api, client, navigate, toast }: AppPageProps) {
  const [selectedRunId, setSelectedRunId] = useState("");
  const [query, setQuery] = useState("");
  const [showRawEvidence, setShowRawEvidence] = useState(false);
  const [exportingEvidence, setExportingEvidence] = useState(false);

  const automationRunsQuery = useQuery({
    queryKey: ["control-loop", "automation-runs"],
    queryFn: () =>
      api("/api/engine/automations/v2/runs?limit=120").catch((error: any) => ({
        runs: [],
        error: String(error?.message || error),
      })),
    refetchInterval: 10000,
  });
  const contextRunsQuery = useQuery({
    queryKey: ["control-loop", "context-runs"],
    queryFn: () =>
      api("/api/engine/context/runs?limit=120").catch((error: any) => ({
        runs: [],
        error: String(error?.message || error),
      })),
    refetchInterval: 10000,
  });
  const acaRunsQuery = useQuery({
    queryKey: ["control-loop", "aca-runs"],
    queryFn: () =>
      api("/api/aca/runs").catch((error: any) => ({
        runs: [],
        error: String(error?.message || error),
      })),
    refetchInterval: 10000,
  });
  const acaCoderRunsQuery = useQuery({
    queryKey: ["control-loop", "aca-coder-runs"],
    queryFn: () => api("/api/aca/operator/coder-runs").catch(() => ({ coder_runs: [] })),
    refetchInterval: 15000,
  });
  const pendingApprovalsQuery = useQuery({
    queryKey: ["control-loop", "approvals", "pending"],
    queryFn: () =>
      api("/api/engine/approvals/pending").catch((error: any) => ({
        approvals: [],
        error: String(error?.message || error),
      })),
    refetchInterval: 5000,
  });
  const acaPendingApprovalsQuery = useQuery({
    queryKey: ["control-loop", "aca-approvals", "pending"],
    queryFn: () =>
      api("/api/aca/approvals/pending").catch((error: any) => ({
        approvals: [],
        error: String(error?.message || error),
      })),
    refetchInterval: 5000,
  });
  const memoryQuery = useQuery({
    queryKey: ["control-loop", "memory-list"],
    queryFn: () =>
      client.memory.list({ q: "", limit: 120 }).catch((error: any) => ({
        items: [],
        error: String(error?.message || error),
      })),
    refetchInterval: 15000,
  });

  const automationRuns = sortedByRecent(toArray(automationRunsQuery.data, "runs"));
  const contextRuns = sortedByRecent(toArray(contextRunsQuery.data, "runs"));
  const acaRuns = sortedByRecent(toArray(acaRunsQuery.data, "runs"));
  const acaCoderRuns = sortedByRecent(toArray(acaCoderRunsQuery.data, "coder_runs"));
  const pendingApprovals = toArray(pendingApprovalsQuery.data, "approvals");
  const acaPendingApprovals = toArray(acaPendingApprovalsQuery.data, "approvals");
  const memoryRows = sortedByRecent(toArray(memoryQuery.data, "items").concat(toArray(memoryQuery.data, "results")));

  const recommendedRunId = useMemo(() => {
    const acaPending = acaPendingApprovals.find((approval: any) => safeString(approval?.run_id));
    if (acaPending) return safeString(acaPending.run_id);
    const blockedAca = acaRuns.find((run: any) =>
      ["blocked", "awaiting_approval", "approval_required", "failed"].includes(
        normalizeStatus(run?.status?.run?.status || run?.status)
      )
    );
    if (blockedAca) return runIdOf(blockedAca);
    const blockedAutomation = automationRuns.find((run: any) =>
      ["blocked", "awaiting_approval", "approval_required", "running"].includes(
        normalizeStatus(run?.status)
      )
    );
    if (blockedAutomation) return runIdOf(blockedAutomation);
    return (
      runIdOf(first(acaRuns, null)) ||
      runIdOf(first(automationRuns, null)) ||
      runIdOf(first(contextRuns, null))
    );
  }, [acaPendingApprovals, acaRuns, automationRuns, contextRuns]);

  const effectiveRunId = safeString(selectedRunId || recommendedRunId);
  const selectedAutomationRun = automationRuns.find((run: any) => runIdOf(run) === effectiveRunId);
  const selectedAcaRun = acaRuns.find((run: any) => runIdOf(run) === effectiveRunId);
  const selectedCoderRun = acaCoderRuns.find((run: any) => matchesRunId(run, new Set([effectiveRunId])));
  const contextCandidates = candidateContextRunIds(effectiveRunId, selectedAutomationRun || selectedAcaRun || selectedCoderRun);
  const selectedContextRun =
    contextRuns.find((run: any) => contextCandidates.includes(runIdOf(run))) ||
    contextRuns.find((run: any) => matchesRunId(run, new Set(contextCandidates)));
  const contextRunId = runIdOf(selectedContextRun) || contextCandidates.find((candidate) => candidate.startsWith("automation-v2-")) || "";
  const runIds = buildRunIdSet(effectiveRunId, contextRunId);
  const runTimeline = useRunTimeline({
    runId: effectiveRunId,
    enabled: !!effectiveRunId,
    limit: 120,
  });

  const policyDecisionsQuery = useQuery({
    queryKey: ["control-loop", "policy-decisions", effectiveRunId],
    enabled: !!effectiveRunId,
    queryFn: () =>
      api(
        `/api/engine/governance/policy-decisions?run_id=${encodeURIComponent(
          effectiveRunId
        )}&limit=200`
      ).catch((error: any) => ({
        policy_decisions: [],
        error: String(error?.message || error),
      })),
    refetchInterval: 10000,
  });

  const contextRunDetailQuery = useQuery({
    queryKey: ["control-loop", "context-run", contextRunId],
    enabled: !!contextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(contextRunId)}`).catch(() => ({
        run: null,
      })),
  });
  const contextRunEventsQuery = useQuery({
    queryKey: ["control-loop", "context-events", contextRunId],
    enabled: !!contextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(contextRunId)}/events?tail=120`).catch(
        () => ({ events: [] })
      ),
    refetchInterval: 10000,
  });
  const contextLedgerQuery = useQuery({
    queryKey: ["control-loop", "context-ledger", contextRunId],
    enabled: !!contextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(contextRunId)}/ledger?tail=160`).catch(
        () => ({ records: [], summary: {} })
      ),
    refetchInterval: 10000,
  });
  const acaRunDetailQuery = useQuery({
    queryKey: ["control-loop", "aca-run-detail", effectiveRunId],
    enabled: !!effectiveRunId,
    queryFn: () => api(`/api/aca/runs/${encodeURIComponent(effectiveRunId)}`).catch(() => ({})),
    refetchInterval: selectedAcaRun ? 8000 : false,
  });
  const acaRunApprovalsQuery = useQuery({
    queryKey: ["control-loop", "aca-run-approvals", effectiveRunId],
    enabled: !!effectiveRunId,
    queryFn: () =>
      api(`/api/aca/runs/${encodeURIComponent(effectiveRunId)}/approvals?limit=60`).catch(() => ({ approvals: [] })),
    refetchInterval: selectedAcaRun ? 5000 : false,
  });
  const memoryAuditQuery = useQuery({
    queryKey: ["control-loop", "memory-audit", effectiveRunId],
    enabled: !!effectiveRunId,
    queryFn: () =>
      client.memory.audit({ run_id: effectiveRunId, limit: 80 }).catch(() => ({
        entries: [],
        count: 0,
      })),
    refetchInterval: 15000,
  });

  const contextRunDetail = contextRunDetailQuery.data?.run || contextRunDetailQuery.data || selectedContextRun;
  const acaRunDetail = acaRunDetailQuery.data || selectedAcaRun || {};
  const contextEvents = sortedByRecent(toArray(contextRunEventsQuery.data, "events"));
  const ledgerRecords = sortedByRecent(toArray(contextLedgerQuery.data, "records"));
  const policyDecisions = sortedByRecent(toArray(policyDecisionsQuery.data, "policy_decisions"));
  const selectedPolicyDecisions = policyDecisions.filter((row: any) => {
    const id = policyDecisionRunId(row);
    return id ? runIds.has(id) : matchesRunId(row, runIds);
  });
  const selectedPendingApprovals = pendingApprovals
    .concat(acaPendingApprovals)
    .filter((approval: any) => matchesRunId(approval, runIds));
  const selectedAcaApprovals = sortedByRecent(toArray(acaRunApprovalsQuery.data, "approvals"));
  const gateHistory = toArray(selectedAutomationRun?.checkpoint, "gate_history").concat(
    toArray(selectedAutomationRun?.checkpoint, "gateHistory")
  );
  const selectedApprovals = sortedByRecent(
    selectedAcaApprovals.concat(selectedPendingApprovals, gateHistory)
  );
  const selectedMemoryRows = memoryRows.filter((row: any) => runIds.has(memoryRunId(row)));
  const memoryAuditRows = sortedByRecent(
    toArray(memoryAuditQuery.data, "entries").concat(toArray(memoryAuditQuery.data, "audit"))
  );
  const toolRows = ledgerRecords.map(toolRecord).filter((record: any) => safeString(record?.tool));
  const blockedToolRows = toolRows.filter((record: any) => normalizeStatus(record?.status) === "blocked");
  const succeededToolRows = toolRows.filter((record: any) =>
    ["succeeded", "completed", "executed"].includes(normalizeStatus(record?.status))
  );
  const eventRows = contextEvents.filter((event: any) => safeString(event?.event_type));
  const memoryPromotions = memoryAuditRows.filter((entry: any) =>
    normalizeStatus(entry?.event_type || entry?.kind || entry?.action).includes("promot")
  );
  const memoryReuseRows = memoryAuditRows.filter((entry: any) => {
    const eventType = normalizeStatus(entry?.event_type || entry?.kind || entry?.action);
    return eventType.includes("search") || eventType.includes("inject") || eventType.includes("context");
  });

  const selectedSource =
    selectedAcaRun || selectedCoderRun
      ? "aca"
      : selectedAutomationRun
        ? "automation_v2"
        : selectedContextRun
          ? "context"
          : "unknown";
  const selectedStatus = runStatus(selectedAutomationRun, contextRunDetail, acaRunDetail);
  const objective = runObjective(selectedAutomationRun, contextRunDetail, acaRunDetail);
  const actionFirewallReady =
    !!selectedAcaRun ||
    selectedAcaApprovals.length > 0 ||
    selectedPendingApprovals.some(
      (approval: any) => safeString(approval?.source) === "aca_external_action"
    );

  const steps: EvidenceStep[] = [
    {
      id: "goal",
      label: "Goal",
      icon: "target",
      tone: objective === "No goal captured" ? "ghost" : "info",
      headline: objective,
      detail: `${sourceLabel(selectedSource)} · ${effectiveRunId || "no run selected"}`,
    },
    {
      id: "run",
      label: "Agent / run",
      icon: "bot",
      tone: evidenceToneForStatus(selectedStatus),
      headline: titleCase(selectedStatus),
      detail: contextRunId ? `Context ledger ${contextRunId}` : "No context ledger linked",
    },
    {
      id: "tools",
      label: "Tool calls",
      icon: "wrench",
      tone: toolRows.length ? "info" : eventRows.length ? "ghost" : "warn",
      headline: toolRows.length ? `${toolRows.length} tool-effect records` : `${eventRows.length} run events`,
      detail: safeString(toolRows[0]?.tool || eventRows[0]?.event_type, "Waiting for execution evidence"),
      count: toolRows.length || eventRows.length,
    },
    {
      id: "policy",
      label: "Policy",
      icon: "shield",
      tone: selectedPolicyDecisions.length || blockedToolRows.length ? "warn" : "ghost",
      headline: selectedPolicyDecisions.length
        ? `${selectedPolicyDecisions.length} policy decisions`
        : blockedToolRows.length
          ? `${blockedToolRows.length} blocked tool records`
          : "No policy decision linked",
      detail: safeString(
        policyDecisionLabel(selectedPolicyDecisions[0], "") ||
          blockedToolRows[0]?.policy_decision_id ||
          blockedToolRows[0]?.tool,
        "Governance feed has no matching rows"
      ),
      count: selectedPolicyDecisions.length || blockedToolRows.length,
    },
    {
      id: "approval",
      label: "Approval gate",
      icon: "shield-check",
      tone: selectedApprovals.length
        ? evidenceToneForStatus(selectedApprovals[0]?.status || selectedApprovals[0]?.decision)
        : "ghost",
      headline: selectedApprovals.length ? `${selectedApprovals.length} approval records` : "No approval gate recorded",
      detail: safeString(actionPreview(selectedApprovals[0]), "No human checkpoint linked"),
      count: selectedApprovals.length,
    },
    {
      id: "decision",
      label: "Human decision",
      icon: "user-check",
      tone: selectedApprovals.some((approval: any) =>
        ["approved", "executed", "rejected", "denied"].includes(
          normalizeStatus(approval?.status || approval?.decision)
        )
      )
        ? "ok"
        : selectedApprovals.length
          ? "warn"
          : "ghost",
      headline: selectedApprovals.length
        ? titleCase(safeString(selectedApprovals[0]?.status || selectedApprovals[0]?.decision, "pending"))
        : "No decision yet",
      detail: safeString(
        selectedApprovals[0]?.reviewed_by ||
          selectedApprovals[0]?.decided_by ||
          selectedApprovals[0]?.requested_by,
        "Operator decision pending"
      ),
    },
    {
      id: "audit",
      label: "Action / audit",
      icon: "file-check-2",
      tone:
        succeededToolRows.length ||
        selectedAcaApprovals.some(
          (approval: any) => normalizeStatus(approval?.status) === "executed"
        )
          ? "ok"
          : "ghost",
      headline: succeededToolRows.length
        ? `${succeededToolRows.length} executed tool records`
        : selectedAcaApprovals.length
          ? `${selectedAcaApprovals.length} action approvals`
          : "No executed action found",
      detail: safeString(succeededToolRows[0]?.tool || selectedAcaApprovals[0]?.action_type, "Audit evidence waiting"),
      count: succeededToolRows.length || selectedAcaApprovals.length,
    },
    {
      id: "memory",
      label: "Memory",
      icon: "database-zap",
      tone: selectedMemoryRows.length || memoryPromotions.length ? "ok" : memoryReuseRows.length ? "info" : "ghost",
      headline: selectedMemoryRows.length
        ? `${selectedMemoryRows.length} memory records`
        : memoryPromotions.length
          ? `${memoryPromotions.length} promotion events`
          : memoryReuseRows.length
            ? `${memoryReuseRows.length} reuse events`
            : "No run memory found",
      detail: safeString(
        memoryText(selectedMemoryRows[0]) ||
          memoryPromotions[0]?.event_type ||
          memoryReuseRows[0]?.event_type,
        "No promotion or reuse evidence"
      ),
      count: selectedMemoryRows.length || memoryPromotions.length || memoryReuseRows.length,
    },
  ];

  const filteredCandidates = useMemo(() => {
    const term = query.trim().toLowerCase();
    const rows = [
      ...acaRuns.map((run: any) => ({ source: "aca", run })),
      ...automationRuns.map((run: any) => ({ source: "automation_v2", run })),
      ...contextRuns.map((run: any) => ({ source: "context", run })),
    ];
    const seen = new Set<string>();
    return rows
      .filter(({ run }) => {
        const id = runIdOf(run);
        if (!id || seen.has(id)) return false;
        seen.add(id);
        if (!term) return true;
        return JSON.stringify(run).toLowerCase().includes(term);
      })
      .slice(0, 18);
  }, [acaRuns, automationRuns, contextRuns, query]);

  const loadingInitial =
    automationRunsQuery.isLoading && contextRunsQuery.isLoading && acaRunsQuery.isLoading;
  const sourceErrors = [
    ["ACA", optionalError(acaRunsQuery.data)],
    ["Governance", optionalError(policyDecisionsQuery.data)],
    ["Memory", optionalError(memoryQuery.data)],
    ["Approvals", optionalError(pendingApprovalsQuery.data) || optionalError(acaPendingApprovalsQuery.data)],
  ].filter(([, error]) => safeString(error));

  useEffect(() => {
    if (!effectiveRunId && recommendedRunId) setSelectedRunId(recommendedRunId);
  }, [effectiveRunId, recommendedRunId]);

  async function exportGovernanceEvidence() {
    if (!contextRunId) {
      toast("warn", "Select a run with a context ledger before exporting evidence.");
      return;
    }
    setExportingEvidence(true);
    try {
      const response = await api(
        `/api/engine/context/runs/${encodeURIComponent(contextRunId)}/governance-evidence`
      );
      const evidencePackage = response?.evidence_package || response;
      const filename =
        safeString(response?.filename) ||
        `tandem-governance-evidence-${safeDownloadName(effectiveRunId || contextRunId)}.json`;
      downloadJsonFile(filename, evidencePackage);
      toast("ok", "Governance evidence package downloaded.");
    } catch (error: any) {
      toast("err", error instanceof Error ? error.message : String(error));
    } finally {
      setExportingEvidence(false);
    }
  }

  return (
    <AnimatedPage className="grid h-full min-h-0 gap-4">
      <section className="grid gap-3">
        <div className="flex flex-col justify-between gap-3 md:flex-row md:items-start">
          <div className="min-w-0">
            <h2 className="text-base font-semibold text-tcp-text-primary">Control-loop evidence</h2>
            <p className="tcp-subtle mt-1 text-sm">
              Goal, run, tool, policy, approval, action, audit, and memory in one trace.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Badge tone={actionFirewallReady ? "ok" : "ghost"}>
              {actionFirewallReady ? "action-firewall evidence" : "general trace"}
            </Badge>
            <StatusPulse
              tone={automationRunsQuery.isFetching || acaRunsQuery.isFetching ? "live" : "info"}
              text="live evidence"
            />
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => {
                automationRunsQuery.refetch();
                contextRunsQuery.refetch();
                acaRunsQuery.refetch();
                pendingApprovalsQuery.refetch();
                acaPendingApprovalsQuery.refetch();
                policyDecisionsQuery.refetch();
                memoryQuery.refetch();
                if (effectiveRunId) {
                  contextRunEventsQuery.refetch();
                  contextLedgerQuery.refetch();
                  acaRunApprovalsQuery.refetch();
                  memoryAuditQuery.refetch();
                  runTimeline.refresh();
                }
              }}
            >
              <Icon name="refresh-cw" />
              Refresh
            </button>
          </div>
        </div>
        <Toolbar className="mb-3">
          <input
            className="tcp-input min-w-0 flex-1"
            value={selectedRunId || recommendedRunId}
            onInput={(event) => setSelectedRunId((event.target as HTMLInputElement).value)}
            placeholder="Run id"
          />
          <SearchInput
            className="tcp-input min-w-0 flex-1"
            aria-label="Filter recent runs"
            value={query}
            onInput={(event) => setQuery((event.target as HTMLInputElement).value)}
            placeholder="Filter recent runs"
          />
          <button type="button" className="tcp-btn" onClick={() => navigate("approvals")}>
            <Icon name="shield-check" />
            Approvals
          </button>
          <button type="button" className="tcp-btn" onClick={() => navigate("memory")}>
            <Icon name="database" />
            Memory
          </button>
          <button
            type="button"
            className="tcp-btn"
            disabled={!contextRunId || exportingEvidence}
            onClick={exportGovernanceEvidence}
          >
            <Icon name={exportingEvidence ? "loader-circle" : "download"} />
            {exportingEvidence ? "Exporting" : "Export"}
          </button>
        </Toolbar>
      </section>

      {sourceErrors.length ? (
        <div className="flex flex-wrap gap-2">
          {sourceErrors.map(([label, error]) => (
            <Badge key={label} tone="warn">
              {label}: {safeString(error).slice(0, 96)}
            </Badge>
          ))}
        </div>
      ) : null}

      {loadingInitial ? (
        <LoadingState
          title="Loading control-loop evidence"
          text="Checking runs, approvals, governance, and memory"
        />
      ) : !effectiveRunId ? (
        <EmptyState
          title="No runs found"
          text="Action-firewall and automation runs will appear here once execution evidence exists."
        />
      ) : (
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_22rem]">
            <div className="grid gap-4">
              <section className="grid gap-2 md:grid-cols-4">
                <Metric label="Run source" value={sourceLabel(selectedSource)} tone="info" />
                <Metric
                  label="Status"
                  value={titleCase(selectedStatus)}
                  tone={evidenceToneForStatus(selectedStatus)}
                />
                <Metric
                  label="Policy"
                  value={String(selectedPolicyDecisions.length || blockedToolRows.length)}
                  tone={selectedPolicyDecisions.length || blockedToolRows.length ? "warn" : "ghost"}
                />
                <Metric
                  label="Memory"
                  value={String(selectedMemoryRows.length || memoryPromotions.length)}
                  tone={selectedMemoryRows.length || memoryPromotions.length ? "ok" : "ghost"}
                />
              </section>

              <section className="grid gap-3">
                {steps.map((step, index) => (
                  <LoopStepCard key={step.id} step={step} index={index} />
                ))}
              </section>

              <RunTimeline
                entries={runTimeline.entries}
                loading={runTimeline.loading}
                loadingMore={runTimeline.loadingMore}
                error={runTimeline.error}
                hasMore={runTimeline.hasMore}
                onRefresh={runTimeline.refresh}
                onLoadMore={runTimeline.loadMore}
                title="Runtime Event Timeline"
                subtitle={effectiveRunId}
              />

              <section className="grid gap-4 lg:grid-cols-2">
                <EvidencePanel
                  title="Policy Decisions"
                  rows={selectedPolicyDecisions}
                  empty="No policy decision rows matched this run."
                  render={(row, index) => (
                    <EvidenceRow
                      key={safeString(row?.decision_id || index)}
                      icon="shield"
                      title={titleCase(policyDecisionLabel(row))}
                      subtitle={safeString(governanceReason(row), "No reason recorded")}
                      meta={safeString(row?.decision_id || row?.tool || row?.capability_key)}
                      tone={evidenceToneForStatus(policyDecisionLabel(row))}
                    />
                  )}
                />
                <EvidencePanel
                  title="Approvals"
                  rows={selectedApprovals}
                  empty="No approval records matched this run."
                  render={(row, index) => (
                    <EvidenceRow
                      key={safeString(row?.approval_id || row?.request_id || index)}
                      icon="user-check"
                      title={titleCase(
                        row?.status || row?.decision || row?.action_type || row?.request_type
                      )}
                      subtitle={actionPreview(row) || "No action preview"}
                      meta={safeString(row?.approval_id || row?.request_id || row?.node_id)}
                      tone={evidenceToneForStatus(row?.status || row?.decision)}
                    />
                  )}
                />
              </section>

              <section className="grid gap-4 lg:grid-cols-2">
                <EvidencePanel
                  title="Tool And Audit Records"
                  rows={toolRows.length ? toolRows : eventRows}
                  empty="No tool-effect or context event records matched this run."
                  render={(row, index) => (
                    <EvidenceRow
                      key={safeString(row?.event_id || row?.tool || row?.seq || index)}
                      icon={safeString(row?.tool) ? "wrench" : "activity"}
                      title={safeString(row?.tool || row?.event_type, "event")}
                      subtitle={safeString(
                        row?.error || row?.phase || row?.status || row?.payload?.detail,
                        "Recorded"
                      )}
                      meta={formatTime(timestampMs(row))}
                      tone={evidenceToneForStatus(row?.status)}
                    />
                  )}
                />
                <EvidencePanel
                  title="Memory"
                  rows={selectedMemoryRows.length ? selectedMemoryRows : memoryAuditRows}
                  empty="No memory promotion, audit, or reuse rows matched this run."
                  render={(row, index) => (
                    <EvidenceRow
                      key={safeString(row?.id || row?.audit_id || row?.event_id || index)}
                      icon="database-zap"
                      title={safeString(row?.id || row?.event_type || row?.kind, "memory")}
                      subtitle={
                        memoryText(row) ||
                        safeString(row?.detail || row?.reason || row?.action, "Memory audit row")
                      }
                      meta={safeString(memoryRunId(row) || relativeTime(timestampMs(row)))}
                      tone={memoryText(row) ? "ok" : "info"}
                    />
                  )}
                />
              </section>

              <button
                type="button"
                className="tcp-btn w-fit h-8 px-3 text-xs"
                onClick={() => setShowRawEvidence((value) => !value)}
              >
                <Icon name={showRawEvidence ? "chevron-up" : "chevron-down"} />
                Raw evidence
              </button>
              {showRawEvidence ? (
                <pre className="max-h-96 overflow-auto rounded-lg border border-white/10 bg-black/30 p-3 text-xs text-tcp-text-secondary">
                  {JSON.stringify(
                    {
                      run_id: effectiveRunId,
                      context_run_id: contextRunId,
                      selected_source: selectedSource,
                      run: selectedAutomationRun || selectedAcaRun || selectedContextRun,
                      policy_decisions: selectedPolicyDecisions,
                      approvals: selectedApprovals,
                      runtime_events: runTimeline.entries,
                      ledger_records: ledgerRecords,
                      memory: selectedMemoryRows,
                      memory_audit: memoryAuditRows,
                    },
                    null,
                    2
                  )}
                </pre>
              ) : null}
            </div>

            <aside className="grid content-start gap-4">
              <PanelCard title="Recent Runs" subtitle="Action-firewall candidates first.">
                <div className="grid gap-2">
                  {filteredCandidates.length ? (
                    filteredCandidates.map(({ source, run }) => {
                      const runId = runIdOf(run);
                      const active = runId === effectiveRunId;
                      const status = runStatus(
                        source === "automation_v2" ? run : null,
                        source === "context" ? run : null,
                        source === "aca" ? run : null
                      );
                      return (
                        <button
                          key={`${source}:${runId}`}
                          type="button"
                          className={`tcp-list-item text-left ${
                            active ? "border-sky-400/60 bg-sky-950/20" : ""
                          }`.trim()}
                          onClick={() => setSelectedRunId(runId)}
                        >
                          <div className="mb-1 flex items-center justify-between gap-2">
                            <strong className="truncate text-sm">{runId}</strong>
                            <Badge tone={badgeTone(evidenceToneForStatus(status))}>
                              {titleCase(status)}
                            </Badge>
                          </div>
                          <div className="flex items-center justify-between gap-2 text-xs text-tcp-text-tertiary">
                            <span>{sourceLabel(source)}</span>
                            <span>{relativeTime(timestampMs(run))}</span>
                          </div>
                        </button>
                      );
                    })
                  ) : (
                    <EmptyState text="No recent runs match this filter." />
                  )}
                </div>
              </PanelCard>

              <PanelCard title="Trace Links" subtitle={effectiveRunId || "No run selected"}>
                <div className="grid gap-2 text-xs">
                  <LinkRow label="Run id" value={effectiveRunId} />
                  <LinkRow label="Context run" value={contextRunId || "n/a"} />
                  <LinkRow label="Policy rows" value={String(selectedPolicyDecisions.length)} />
                  <LinkRow label="Approval rows" value={String(selectedApprovals.length)} />
                  <LinkRow label="Tool records" value={String(toolRows.length)} />
                  <LinkRow label="Memory rows" value={String(selectedMemoryRows.length)} />
                </div>
              </PanelCard>
            </aside>
        </div>
      )}
    </AnimatedPage>
  );
}

function Metric({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone: EvidenceTone;
}) {
  return (
    <div className="rounded-lg border border-white/10 bg-black/20 p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-xs text-tcp-text-tertiary">{label}</span>
        <Badge tone={badgeTone(tone)}>{toneLabel(tone)}</Badge>
      </div>
      <div className="truncate text-lg font-semibold text-tcp-text-primary">{value}</div>
    </div>
  );
}

function LoopStepCard({ step, index }: { step: EvidenceStep; index: number }) {
  return (
    <article className="grid gap-3 rounded-lg border border-white/10 bg-black/20 p-3 md:grid-cols-[2.5rem_10rem_minmax(0,1fr)_auto] md:items-center">
      <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-white/10 bg-white/5 text-tcp-text-secondary">
        <Icon name={step.icon} />
      </div>
      <div className="min-w-0">
        <div className="tcp-text-caption uppercase text-tcp-text-tertiary">Step {index + 1}</div>
        <div className="truncate text-sm font-semibold text-tcp-text-primary">{step.label}</div>
      </div>
      <div className="min-w-0">
        <div className="truncate text-sm font-medium text-tcp-text-primary">{step.headline}</div>
        <div className="mt-1 line-clamp-2 text-xs text-tcp-text-secondary">{step.detail}</div>
      </div>
      <Badge tone={badgeTone(step.tone)}>
        {step.count !== undefined ? step.count : toneLabel(step.tone)}
      </Badge>
    </article>
  );
}

function EvidencePanel({
  title,
  rows,
  empty,
  render,
}: {
  title: string;
  rows: any[];
  empty: string;
  render: (row: any, index: number) => any;
}) {
  return (
    <section className="rounded-lg border border-white/10 bg-black/15 p-3">
      <div className="mb-3 flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold text-tcp-text-primary">{title}</h3>
        <Badge tone={rows.length ? "info" : "ghost"}>{rows.length}</Badge>
      </div>
      <div className="grid max-h-80 gap-2 overflow-auto pr-1">
        {rows.length ? (
          rows.slice(0, 12).map(render)
        ) : (
          <div className="tcp-subtle text-xs">{empty}</div>
        )}
      </div>
    </section>
  );
}

function EvidenceRow({
  icon,
  title,
  subtitle,
  meta,
  tone,
}: {
  icon: IconName;
  title: string;
  subtitle: string;
  meta: string;
  tone: EvidenceTone;
}) {
  return (
    <article className="grid grid-cols-[2rem_minmax(0,1fr)] gap-2 rounded-md border border-white/10 bg-black/20 p-2">
      <div className="mt-0.5 flex h-8 w-8 items-center justify-center rounded-md border border-white/10 bg-white/5">
        <Icon name={icon} />
      </div>
      <div className="min-w-0">
        <div className="mb-1 flex items-center justify-between gap-2">
          <strong className="truncate text-xs text-tcp-text-primary">{title}</strong>
          <Badge tone={badgeTone(tone)}>{toneLabel(tone)}</Badge>
        </div>
        <div className="line-clamp-2 text-xs text-tcp-text-secondary">{subtitle}</div>
        {meta ? (
          <div className="mt-1 truncate tcp-text-caption text-tcp-text-tertiary">{meta}</div>
        ) : null}
      </div>
    </article>
  );
}

function LinkRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border border-white/10 bg-black/20 px-3 py-2">
      <span className="text-tcp-text-tertiary">{label}</span>
      <span className="min-w-0 truncate font-mono text-tcp-text-secondary">{value}</span>
    </div>
  );
}
