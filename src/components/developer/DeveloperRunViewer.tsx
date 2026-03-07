import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  Brain,
  Database,
  ExternalLink,
  GitBranch,
  PanelsTopLeft,
  RefreshCw,
  Search,
  SquareCheckBig,
  SquareX,
  Workflow,
} from "lucide-react";
import {
  approveCoderRun,
  cancelCoderRun,
  getCoderMemoryHits,
  getCoderRun,
  listCoderArtifacts,
  listCoderMemoryCandidates,
  listCoderRuns,
  readFileText,
  type CoderArtifactRecord,
  type CoderMemoryCandidateRecord,
  type CoderRunRecord,
} from "@/lib/tauri";
import { DiffViewer } from "@/components/plan/DiffViewer";
import { Button } from "@/components/ui/Button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import { cn } from "@/lib/utils";

type DeveloperRunViewerProps = {
  repoSlug?: string | null;
  onOpenMcpSettings?: () => void;
};

type RunTaskRecord = Record<string, unknown>;
type BlackboardRow = Record<string, unknown>;
type BlackboardTimelineItem = {
  id: string;
  kind: "decision" | "question";
  text: string;
  tsMs: number | null;
  stepId: string | null;
  sourceEventId: string | null;
};
type RunEventRow = Record<string, unknown>;

type ArtifactCategory = "duplicate" | "triage" | "memory" | "validation" | "other";

type ArtifactGroup = {
  key: ArtifactCategory;
  label: string;
  description: string;
  artifacts: CoderArtifactRecord[];
};

const TASK_COLUMNS = [
  { key: "runnable", label: "Runnable" },
  { key: "in_progress", label: "In Progress" },
  { key: "blocked", label: "Blocked" },
  { key: "done", label: "Done" },
  { key: "failed", label: "Failed" },
] as const;

function formatTimestamp(value?: number | null): string {
  if (!value) return "Unknown";
  try {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(value);
  } catch {
    return String(value);
  }
}

function asArray<T>(value: T[] | undefined | null): T[] {
  return Array.isArray(value) ? value : [];
}

function statusTone(status?: string | null): string {
  switch ((status ?? "").toLowerCase()) {
    case "completed":
      return "border-emerald-500/40 bg-emerald-500/10 text-emerald-200";
    case "failed":
    case "blocked":
      return "border-rose-500/40 bg-rose-500/10 text-rose-200";
    case "awaiting_approval":
      return "border-amber-500/40 bg-amber-500/10 text-amber-100";
    case "running":
    case "planning":
      return "border-sky-500/40 bg-sky-500/10 text-sky-200";
    case "cancelled":
      return "border-zinc-500/40 bg-zinc-500/10 text-zinc-300";
    default:
      return "border-border bg-surface-elevated text-text";
  }
}

function renderValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return JSON.stringify(value, null, 2) ?? "";
}

function pickText(value: unknown): string {
  if (typeof value === "string" && value.trim().length > 0) return value.trim();
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function artifactCategory(artifact: CoderArtifactRecord): ArtifactCategory {
  const type = artifact.artifact_type.toLowerCase();
  const path = artifact.path.toLowerCase();
  if (type.includes("duplicate") || path.includes("duplicate")) return "duplicate";
  if (type.includes("triage") || path.includes("triage")) return "triage";
  if (type.includes("memory") || path.includes("memory")) return "memory";
  if (type.includes("validation") || path.includes("validation")) return "validation";
  return "other";
}

function artifactCategoryTone(category: ArtifactCategory): string {
  switch (category) {
    case "duplicate":
      return "border-amber-500/30 bg-amber-500/10 text-amber-100";
    case "triage":
      return "border-sky-500/30 bg-sky-500/10 text-sky-200";
    case "memory":
      return "border-emerald-500/30 bg-emerald-500/10 text-emerald-200";
    case "validation":
      return "border-violet-500/30 bg-violet-500/10 text-violet-200";
    default:
      return "border-border bg-surface text-text-muted";
  }
}

function duplicateMatchLabel(match: Record<string, unknown>): string {
  return (
    pickText(match.summary) ||
    pickText(match.title) ||
    pickText(match.issue_title) ||
    pickText(match.fingerprint) ||
    pickText(match.candidate_id) ||
    "Historical match"
  );
}

function duplicateMatchBadges(match: Record<string, unknown>): string[] {
  const badges: string[] = [];
  const issueNumber = pickText(match.issue_number);
  const prNumber = pickText(match.pr_number);
  const score = match.score;
  const confidence = match.confidence;
  const component = pickText(match.component ?? match.affected_component);
  if (issueNumber) badges.push(`Issue #${issueNumber}`);
  if (prNumber) badges.push(`PR #${prNumber}`);
  if (typeof score === "number") badges.push(`score ${score.toFixed(2)}`);
  if (typeof confidence === "number") badges.push(`confidence ${confidence.toFixed(2)}`);
  if (component) badges.push(component);
  return badges;
}

function blackboardRowText(row: BlackboardRow): string {
  return (
    pickText(row.text) ||
    pickText(row.summary) ||
    pickText(row.label) ||
    pickText(row.title) ||
    pickText(row.reason) ||
    renderValue(row)
  );
}

function blackboardRowTimestamp(row: BlackboardRow): number | null {
  const value = row.ts_ms;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function blackboardRowStepId(row: BlackboardRow): string | null {
  const value = row.step_id;
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function blackboardRowSourceEventId(row: BlackboardRow): string | null {
  const value = row.source_event_id;
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function runEventText(event: RunEventRow): string {
  const payload = asRecord(event.payload);
  return (
    pickText(payload?.why_next_step) ||
    pickText(payload?.detail) ||
    pickText(payload?.summary) ||
    pickText(payload?.stage) ||
    pickText(payload?.message) ||
    ""
  );
}

function runEventTimestamp(event: RunEventRow): number | null {
  const value = event.ts_ms;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function runEventType(event: RunEventRow): string {
  return pickText(event.type) || "event";
}

function runEventId(event: RunEventRow, index: number): string {
  return pickText(event.event_id) || `${runEventType(event)}-${index}`;
}

function isValidationTask(task: RunTaskRecord): boolean {
  const typeText = [
    pickText(task.task_type),
    pickText(task.workflow_node_id),
    pickText(task.title),
    pickText(asRecord(task.payload)?.task_kind),
  ]
    .join(" ")
    .toLowerCase();
  return typeText.includes("validation");
}

function taskLabel(task: RunTaskRecord): string {
  return String(task.title ?? task.workflow_node_id ?? task.task_type ?? task.id ?? "task");
}

function memoryKindLabel(value: unknown): string {
  return pickText(value) || "memory";
}

function runNeedsAttention(run: CoderRunRecord): boolean {
  const status = (run.status ?? "").toLowerCase();
  return status === "failed" || status === "blocked" || status === "awaiting_approval";
}

function runRecencyLabel(updatedAtMs: number): "fresh" | "recent" | "stale" {
  const ageMs = Math.max(0, Date.now() - updatedAtMs);
  if (ageMs <= 15 * 60 * 1000) return "fresh";
  if (ageMs <= 2 * 60 * 60 * 1000) return "recent";
  return "stale";
}

function runRecencyTone(label: "fresh" | "recent" | "stale"): string {
  switch (label) {
    case "fresh":
      return "border-emerald-500/20 bg-emerald-500/10 text-emerald-200";
    case "recent":
      return "border-sky-500/20 bg-sky-500/10 text-sky-200";
    default:
      return "border-zinc-500/20 bg-zinc-500/10 text-zinc-300";
  }
}

function relatedArtifactsForEvent(
  artifacts: CoderArtifactRecord[],
  stepId: string,
  sourceEventId: string
): CoderArtifactRecord[] {
  return artifacts.filter((artifact) => {
    if (sourceEventId && artifact.source_event_id === sourceEventId) return true;
    if (stepId && artifact.step_id === stepId) return true;
    return false;
  });
}

function relatedArtifactsForTask(
  artifacts: CoderArtifactRecord[],
  task: RunTaskRecord
): CoderArtifactRecord[] {
  const taskIds = [
    pickText(task.id),
    pickText(task.workflow_node_id),
    pickText(task.task_type),
  ].filter((value) => value.length > 0);
  return artifacts.filter((artifact) => !!artifact.step_id && taskIds.includes(artifact.step_id));
}

export function DeveloperRunViewer({ repoSlug, onOpenMcpSettings }: DeveloperRunViewerProps) {
  const [runs, setRuns] = useState<CoderRunRecord[]>([]);
  const [runQuery, setRunQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [workflowFilter, setWorkflowFilter] = useState<string>("all");
  const [runSortMode, setRunSortMode] = useState<"updated" | "attention" | "approval">("updated");
  const [artifactQuery, setArtifactQuery] = useState("");
  const [eventQuery, setEventQuery] = useState("");
  const [eventTypeFilter, setEventTypeFilter] = useState<string>("all");
  const [detailTab, setDetailTab] = useState<"overview" | "artifacts" | "memory" | "validation">(
    "overview"
  );
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [selectedRun, setSelectedRun] = useState<CoderRunRecord | null>(null);
  const [runState, setRunState] = useState<Record<string, unknown> | null>(null);
  const [artifacts, setArtifacts] = useState<CoderArtifactRecord[]>([]);
  const [memoryHits, setMemoryHits] = useState<Record<string, unknown>[]>([]);
  const [memoryCandidates, setMemoryCandidates] = useState<CoderMemoryCandidateRecord[]>([]);
  const [selectedArtifactPath, setSelectedArtifactPath] = useState<string | null>(null);
  const [selectedArtifactContent, setSelectedArtifactContent] = useState<string>("");
  const [loadingArtifact, setLoadingArtifact] = useState(false);
  const [loadingRuns, setLoadingRuns] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [acting, setActing] = useState<"approve" | "cancel" | null>(null);
  const [copiedContextRun, setCopiedContextRun] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const kanbanRef = useRef<HTMLDivElement | null>(null);
  const blackboardRef = useRef<HTMLDivElement | null>(null);
  const timelineRef = useRef<HTMLDivElement | null>(null);
  const memorySnapshotRef = useRef<HTMLDivElement | null>(null);

  const loadRuns = useCallback(async () => {
    setLoadingRuns(true);
    try {
      const payload = await listCoderRuns({ limit: 40, repoSlug: repoSlug ?? undefined });
      setRuns(payload.runs);
      setSelectedRunId((current) => current ?? payload.runs[0]?.coder_run_id ?? null);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoadingRuns(false);
    }
  }, [repoSlug]);

  const loadRunDetail = useCallback(async (runId: string) => {
    setLoadingDetail(true);
    try {
      const [runPayload, artifactsPayload, memoryHitsPayload, memoryCandidatesPayload] =
        await Promise.all([
          getCoderRun(runId),
          listCoderArtifacts(runId),
          getCoderMemoryHits(runId, { limit: 8 }),
          listCoderMemoryCandidates(runId),
        ]);
      setSelectedRun(runPayload.coder_run);
      setRunState(runPayload.run);
      setArtifacts(asArray(artifactsPayload.artifacts));
      setMemoryHits(asArray(memoryHitsPayload.hits));
      setMemoryCandidates(asArray(memoryCandidatesPayload.candidates));
      setSelectedArtifactPath((current) => current ?? artifactsPayload.artifacts[0]?.path ?? null);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoadingDetail(false);
    }
  }, []);

  useEffect(() => {
    void loadRuns();
    const interval = globalThis.setInterval(() => {
      void loadRuns();
    }, 8000);
    return () => globalThis.clearInterval(interval);
  }, [loadRuns]);

  useEffect(() => {
    if (!selectedRunId) {
      setSelectedRun(null);
      setRunState(null);
      setArtifacts([]);
      setMemoryHits([]);
      setMemoryCandidates([]);
      setSelectedArtifactPath(null);
      setSelectedArtifactContent("");
      return;
    }
    void loadRunDetail(selectedRunId);
  }, [loadRunDetail, selectedRunId]);

  useEffect(() => {
    if (!selectedArtifactPath) {
      setSelectedArtifactContent("");
      return;
    }
    let cancelled = false;
    const loadArtifact = async () => {
      setLoadingArtifact(true);
      try {
        const content = await readFileText(selectedArtifactPath, 512 * 1024, 120_000);
        if (!cancelled) {
          setSelectedArtifactContent(content);
        }
      } catch (err) {
        if (!cancelled) {
          setSelectedArtifactContent(err instanceof Error ? err.message : String(err));
        }
      } finally {
        if (!cancelled) {
          setLoadingArtifact(false);
        }
      }
    };
    void loadArtifact();
    return () => {
      cancelled = true;
    };
  }, [selectedArtifactPath]);

  const selectedTaskRows = useMemo(() => {
    const tasks = runState?.tasks;
    return (Array.isArray(tasks) ? tasks : []) as RunTaskRecord[];
  }, [runState]);

  const selectedBlackboard = useMemo(() => {
    const blackboard = runState?.blackboard;
    return blackboard && typeof blackboard === "object"
      ? (blackboard as Record<string, unknown>)
      : null;
  }, [runState]);

  const selectedRunEvents = useMemo(() => {
    const events = runState?.events;
    return (Array.isArray(events) ? events : []) as RunEventRow[];
  }, [runState]);

  const decisions = useMemo(() => {
    const rows = selectedBlackboard?.decisions;
    return (Array.isArray(rows) ? rows : []) as BlackboardRow[];
  }, [selectedBlackboard]);

  const openQuestions = useMemo(() => {
    const rows = selectedBlackboard?.open_questions;
    return (Array.isArray(rows) ? rows : []) as BlackboardRow[];
  }, [selectedBlackboard]);

  const latestDecision = useMemo(() => {
    if (decisions.length === 0) return null;
    return (
      [...decisions].sort((left, right) => {
        return (blackboardRowTimestamp(right) ?? 0) - (blackboardRowTimestamp(left) ?? 0);
      })[0] ?? null
    );
  }, [decisions]);

  const blackboardTimeline = useMemo<BlackboardTimelineItem[]>(() => {
    const items = [
      ...decisions.map((row, index) => ({
        id: String(row.id ?? row.source_event_id ?? `decision-${index}`),
        kind: "decision" as const,
        text: blackboardRowText(row),
        tsMs: blackboardRowTimestamp(row),
        stepId: blackboardRowStepId(row),
        sourceEventId: blackboardRowSourceEventId(row),
      })),
      ...openQuestions.map((row, index) => ({
        id: String(row.id ?? row.source_event_id ?? `question-${index}`),
        kind: "question" as const,
        text: blackboardRowText(row),
        tsMs: blackboardRowTimestamp(row),
        stepId: blackboardRowStepId(row),
        sourceEventId: blackboardRowSourceEventId(row),
      })),
    ];
    return items
      .sort((left, right) => {
        const tsDelta = (right.tsMs ?? 0) - (left.tsMs ?? 0);
        if (tsDelta !== 0) return tsDelta;
        if (left.kind !== right.kind) return left.kind === "decision" ? -1 : 1;
        return left.id.localeCompare(right.id);
      })
      .slice(0, 10);
  }, [decisions, openQuestions]);

  const eventTimeline = useMemo(() => {
    return [...selectedRunEvents]
      .sort((left, right) => {
        const tsDelta = (runEventTimestamp(right) ?? 0) - (runEventTimestamp(left) ?? 0);
        if (tsDelta !== 0) return tsDelta;
        return runEventId(right, 0).localeCompare(runEventId(left, 0));
      })
      .slice(0, 12);
  }, [selectedRunEvents]);

  const eventTypes = useMemo(() => {
    return [
      "all",
      ...new Set(selectedRunEvents.map((event) => runEventType(event)).filter(Boolean)),
    ];
  }, [selectedRunEvents]);

  const filteredEventTimeline = useMemo(() => {
    const needle = eventQuery.trim().toLowerCase();
    return eventTimeline.filter((event) => {
      const type = runEventType(event);
      if (eventTypeFilter !== "all" && type !== eventTypeFilter) return false;
      if (!needle) return true;
      const text = runEventText(event).toLowerCase();
      const stepId = pickText(event.step_id).toLowerCase();
      const eventId = pickText(event.event_id).toLowerCase();
      return (
        type.toLowerCase().includes(needle) ||
        text.includes(needle) ||
        stepId.includes(needle) ||
        eventId.includes(needle)
      );
    });
  }, [eventQuery, eventTimeline, eventTypeFilter]);

  const readinessHint = useMemo(() => {
    if (!error) return null;
    const normalized = error.toLowerCase();
    if (normalized.includes("readiness") || normalized.includes("mcp")) {
      return "Coder readiness is blocked. Open MCP settings to inspect server connectivity.";
    }
    return null;
  }, [error]);

  const filteredRuns = useMemo(() => {
    const query = runQuery.trim().toLowerCase();
    return runs.filter((run) => {
      if (statusFilter !== "all" && (run.status ?? "unknown") !== statusFilter) return false;
      if (workflowFilter !== "all" && run.workflow_mode !== workflowFilter) return false;
      if (!query) return true;
      return [
        run.coder_run_id,
        run.repo_binding?.repo_slug,
        run.workflow_mode,
        run.phase,
        run.status,
        run.github_ref ? `${run.github_ref.kind} ${run.github_ref.number}` : "",
      ]
        .join(" ")
        .toLowerCase()
        .includes(query);
    });
  }, [runQuery, runs, statusFilter, workflowFilter]);

  const displayedRuns = useMemo(() => {
    const ordered = [...filteredRuns];
    ordered.sort((left, right) => {
      if (runSortMode === "approval") {
        const leftApproval = (left.status ?? "") === "awaiting_approval" ? 1 : 0;
        const rightApproval = (right.status ?? "") === "awaiting_approval" ? 1 : 0;
        if (leftApproval !== rightApproval) return rightApproval - leftApproval;
      } else if (runSortMode === "attention") {
        const leftAttention = runNeedsAttention(left) ? 1 : 0;
        const rightAttention = runNeedsAttention(right) ? 1 : 0;
        if (leftAttention !== rightAttention) return rightAttention - leftAttention;
      }
      return (right.updated_at_ms ?? 0) - (left.updated_at_ms ?? 0);
    });
    return ordered;
  }, [filteredRuns, runSortMode]);

  const runSummary = useMemo(() => {
    return filteredRuns.reduce(
      (summary, run) => {
        const normalized = (run.status ?? "unknown").toLowerCase();
        summary.total += 1;
        if (normalized === "running" || normalized === "planning") summary.active += 1;
        if (normalized === "awaiting_approval") summary.awaitingApproval += 1;
        if (normalized === "failed" || normalized === "blocked") summary.needsAttention += 1;
        return summary;
      },
      { total: 0, active: 0, awaitingApproval: 0, needsAttention: 0 }
    );
  }, [filteredRuns]);

  const runStatuses = useMemo(() => {
    return ["all", ...new Set(runs.map((run) => run.status ?? "unknown"))];
  }, [runs]);

  const workflowModes = useMemo(() => {
    return ["all", ...new Set(runs.map((run) => run.workflow_mode))];
  }, [runs]);

  const taskColumns = useMemo(() => {
    const grouped = new Map<string, RunTaskRecord[]>();
    for (const column of TASK_COLUMNS) grouped.set(column.key, []);
    for (const task of selectedTaskRows) {
      const status = String(task.status ?? "pending").toLowerCase();
      const key =
        status === "pending"
          ? "runnable"
          : TASK_COLUMNS.some((column) => column.key === status)
            ? status
            : "runnable";
      grouped.get(key)?.push(task);
    }
    return TASK_COLUMNS.map((column) => ({
      ...column,
      tasks: grouped.get(column.key) ?? [],
    }));
  }, [selectedTaskRows]);

  const validationTasks = useMemo(() => {
    return selectedTaskRows.filter((task) => isValidationTask(task));
  }, [selectedTaskRows]);

  const artifactGroups = useMemo<ArtifactGroup[]>(() => {
    const needle = artifactQuery.trim().toLowerCase();
    const buckets = new Map<ArtifactCategory, CoderArtifactRecord[]>();
    for (const key of ["duplicate", "triage", "memory", "validation", "other"] as const) {
      buckets.set(key, []);
    }
    for (const artifact of artifacts) {
      if (needle) {
        const haystack = [
          artifact.artifact_type,
          artifact.path,
          artifact.step_id ?? "",
          artifact.source_event_id ?? "",
        ]
          .join(" ")
          .toLowerCase();
        if (!haystack.includes(needle)) continue;
      }
      buckets.get(artifactCategory(artifact))?.push(artifact);
    }
    const groups: ArtifactGroup[] = [
      {
        key: "duplicate",
        label: "Duplicate History",
        description: "Historical matches and duplicate candidate artifacts.",
        artifacts: buckets.get("duplicate") ?? [],
      },
      {
        key: "triage",
        label: "Triage",
        description: "Run summaries and diagnosis artifacts.",
        artifacts: buckets.get("triage") ?? [],
      },
      {
        key: "memory",
        label: "Memory",
        description: "Retrieved memory evidence and memory-backed outputs.",
        artifacts: buckets.get("memory") ?? [],
      },
      {
        key: "validation",
        label: "Validation",
        description: "Validation outcomes and follow-up checks.",
        artifacts: buckets.get("validation") ?? [],
      },
      {
        key: "other",
        label: "Other",
        description: "Remaining artifacts emitted by the run.",
        artifacts: buckets.get("other") ?? [],
      },
    ];
    return groups.filter((group) => group.artifacts.length > 0);
  }, [artifactQuery, artifacts]);

  const selectedArtifactRecord = useMemo(() => {
    if (!selectedArtifactPath) return null;
    return artifacts.find((artifact) => artifact.path === selectedArtifactPath) ?? null;
  }, [artifacts, selectedArtifactPath]);

  const artifactPreview = useMemo(() => {
    if (!selectedArtifactContent) return null;
    try {
      const parsed = JSON.parse(selectedArtifactContent) as Record<string, unknown>;
      const oldValue =
        typeof parsed.old === "string"
          ? parsed.old
          : typeof parsed.before === "string"
            ? parsed.before
            : null;
      const newValue =
        typeof parsed.new === "string"
          ? parsed.new
          : typeof parsed.after === "string"
            ? parsed.after
            : null;
      if (oldValue !== null && newValue !== null) {
        return { kind: "diff" as const, oldValue, newValue };
      }
      return { kind: "raw" as const, value: selectedArtifactContent };
    } catch {
      return { kind: "raw" as const, value: selectedArtifactContent };
    }
  }, [selectedArtifactContent]);

  const selectedArtifactJson = useMemo(() => {
    if (!selectedArtifactContent) return null;
    try {
      return asRecord(JSON.parse(selectedArtifactContent));
    } catch {
      return null;
    }
  }, [selectedArtifactContent]);

  const selectedDuplicateMatches = useMemo(() => {
    const matches = selectedArtifactJson?.matches ?? selectedArtifactJson?.duplicate_candidates;
    return Array.isArray(matches)
      ? matches
          .map((item) => asRecord(item))
          .filter((item): item is Record<string, unknown> => !!item)
      : [];
  }, [selectedArtifactJson]);

  const selectedValidationSummary = useMemo(() => {
    if (!selectedArtifactJson) return null;
    const validation = asRecord(selectedArtifactJson.validation);
    const validationsAttempted = Array.isArray(selectedArtifactJson.validations_attempted)
      ? selectedArtifactJson.validations_attempted.length
      : null;
    const outcome = pickText(
      selectedArtifactJson.outcome ??
        selectedArtifactJson.result ??
        validation?.outcome ??
        validation?.result
    );
    const passed =
      typeof selectedArtifactJson.passed === "boolean"
        ? selectedArtifactJson.passed
        : typeof validation?.passed === "boolean"
          ? validation.passed
          : null;
    if (!outcome && passed === null && validationsAttempted === null) return null;
    return { outcome, passed, validationsAttempted };
  }, [selectedArtifactJson]);

  const selectedRunOverview = useMemo(() => {
    return {
      tasks: selectedTaskRows.length,
      decisions: decisions.length,
      artifacts: artifacts.length,
      validationTasks: validationTasks.length,
      duplicateArtifacts:
        artifactGroups.find((group) => group.key === "duplicate")?.artifacts.length ?? 0,
    };
  }, [
    artifactGroups,
    artifacts.length,
    decisions.length,
    selectedTaskRows.length,
    validationTasks.length,
  ]);

  const validationArtifacts = useMemo(() => {
    return artifactGroups.find((group) => group.key === "validation")?.artifacts ?? [];
  }, [artifactGroups]);

  const latestValidationArtifact = useMemo(() => {
    return [...validationArtifacts].sort((left, right) => right.ts_ms - left.ts_ms)[0] ?? null;
  }, [validationArtifacts]);

  const latestDuplicateArtifact = useMemo(() => {
    const duplicateArtifacts =
      artifactGroups.find((group) => group.key === "duplicate")?.artifacts ?? [];
    return [...duplicateArtifacts].sort((left, right) => right.ts_ms - left.ts_ms)[0] ?? null;
  }, [artifactGroups]);

  const latestMemoryArtifact = useMemo(() => {
    const memoryArtifacts = artifactGroups.find((group) => group.key === "memory")?.artifacts ?? [];
    return [...memoryArtifacts].sort((left, right) => right.ts_ms - left.ts_ms)[0] ?? null;
  }, [artifactGroups]);

  const latestTriageArtifact = useMemo(() => {
    const triageArtifacts = artifactGroups.find((group) => group.key === "triage")?.artifacts ?? [];
    return [...triageArtifacts].sort((left, right) => right.ts_ms - left.ts_ms)[0] ?? null;
  }, [artifactGroups]);

  const detailTabMeta = useMemo(() => {
    return [
      { key: "overview" as const, label: "Overview", count: selectedTaskRows.length },
      { key: "artifacts" as const, label: "Artifacts", count: artifacts.length },
      { key: "validation" as const, label: "Validation", count: validationTasks.length },
      {
        key: "memory" as const,
        label: "Memory",
        count: memoryHits.length + memoryCandidates.length,
      },
    ];
  }, [
    artifacts.length,
    memoryCandidates.length,
    memoryHits.length,
    selectedTaskRows.length,
    validationTasks.length,
  ]);

  const handleAction = useCallback(
    async (action: "approve" | "cancel") => {
      if (!selectedRunId) return;
      setActing(action);
      try {
        if (action === "approve") {
          await approveCoderRun(selectedRunId, "approved from desktop developer mode");
        } else {
          await cancelCoderRun(selectedRunId, "cancelled from desktop developer mode");
        }
        await Promise.all([loadRuns(), loadRunDetail(selectedRunId)]);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setActing(null);
      }
    },
    [loadRunDetail, loadRuns, selectedRunId]
  );

  const copyContextRunId = useCallback(async () => {
    const value = selectedRun?.linked_context_run_id?.trim();
    const clipboard = globalThis.navigator?.clipboard;
    if (!value || !clipboard?.writeText) return;
    try {
      await clipboard.writeText(value);
      setCopiedContextRun(true);
      globalThis.setTimeout(() => setCopiedContextRun(false), 1500);
    } catch {
      // Ignore clipboard failures; the id is still visible in the UI.
    }
  }, [selectedRun?.linked_context_run_id]);

  const focusOverviewSection = useCallback(
    (section: "kanban" | "blackboard" | "timeline" | "memory") => {
      const target =
        section === "kanban"
          ? kanbanRef
          : section === "blackboard"
            ? blackboardRef
            : section === "timeline"
              ? timelineRef
              : memorySnapshotRef;
      setDetailTab("overview");
      globalThis.setTimeout(() => {
        target.current?.scrollIntoView({ behavior: "smooth", block: "start" });
      }, 0);
    },
    []
  );

  return (
    <div className="h-full w-full overflow-hidden app-background">
      <div className="grid h-full grid-cols-[340px_minmax(0,1fr)] gap-4 p-4">
        <Card className="flex min-h-0 flex-col p-0">
          <CardHeader className="border-b border-border px-5 py-4">
            <div className="flex items-start justify-between gap-3">
              <div>
                <CardTitle className="text-base">Developer</CardTitle>
                <CardDescription>
                  Coder runs, artifacts, memory hits, and operator controls.
                </CardDescription>
              </div>
              <Button
                variant="ghost"
                size="sm"
                loading={loadingRuns}
                onClick={() => void loadRuns()}
                aria-label="Refresh coder runs"
              >
                <RefreshCw className="h-4 w-4" />
              </Button>
            </div>
          </CardHeader>
          <CardContent className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
            <div className="mb-3 space-y-2">
              <div className="grid grid-cols-2 gap-2">
                {[
                  ["Visible runs", runSummary.total],
                  ["Active", runSummary.active],
                  ["Awaiting approval", runSummary.awaitingApproval],
                  ["Needs attention", runSummary.needsAttention],
                ].map(([label, value]) => (
                  <button
                    key={label}
                    type="button"
                    onClick={() => {
                      if (label === "Awaiting approval") {
                        setStatusFilter("awaiting_approval");
                        setRunSortMode("approval");
                      } else if (label === "Needs attention") {
                        setStatusFilter("all");
                        setRunSortMode("attention");
                      } else if (label === "Visible runs") {
                        setStatusFilter("all");
                        setWorkflowFilter("all");
                        setRunSortMode("updated");
                      } else if (label === "Active") {
                        setStatusFilter("running");
                        setRunSortMode("updated");
                      }
                    }}
                    className="rounded-2xl border border-border bg-surface-elevated/40 px-3 py-2 text-left transition-colors hover:bg-surface-elevated"
                  >
                    <p className="text-[10px] uppercase tracking-[0.2em] text-text-muted">
                      {label}
                    </p>
                    <p className="mt-1 text-sm font-semibold text-text">{String(value)}</p>
                  </button>
                ))}
              </div>
              <div className="flex items-center gap-2 rounded-2xl border border-border bg-surface-elevated/40 px-3 py-2">
                <Search className="h-4 w-4 text-text-muted" />
                <input
                  value={runQuery}
                  onChange={(event) => setRunQuery(event.target.value)}
                  placeholder="Filter runs by id, repo, mode, or ref"
                  className="w-full bg-transparent text-sm text-text outline-none placeholder:text-text-subtle"
                />
              </div>
              <div className="grid grid-cols-2 gap-2">
                <select
                  value={statusFilter}
                  onChange={(event) => setStatusFilter(event.target.value)}
                  className="rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text outline-none"
                >
                  {runStatuses.map((status) => (
                    <option key={status} value={status}>
                      {status === "all" ? "All statuses" : status}
                    </option>
                  ))}
                </select>
                <select
                  value={runSortMode}
                  onChange={(event) =>
                    setRunSortMode(event.target.value as "updated" | "attention" | "approval")
                  }
                  className="rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text outline-none"
                >
                  <option value="updated">Recently updated</option>
                  <option value="attention">Needs attention first</option>
                  <option value="approval">Awaiting approval first</option>
                </select>
              </div>
              <div className="grid grid-cols-2 gap-2">
                <select
                  value={workflowFilter}
                  onChange={(event) => setWorkflowFilter(event.target.value)}
                  className="rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text outline-none"
                >
                  {workflowModes.map((mode) => (
                    <option key={mode} value={mode}>
                      {mode === "all" ? "All workflows" : mode}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            {displayedRuns.length === 0 ? (
              <div className="flex h-full flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-border bg-surface-elevated/40 p-6 text-center">
                <Workflow className="h-6 w-6 text-text-muted" />
                <div>
                  <p className="text-sm font-medium text-text">No matching coder runs.</p>
                  <p className="text-xs text-text-muted">
                    Adjust the filters or wait for the engine to create a run.
                  </p>
                </div>
              </div>
            ) : (
              <div className="space-y-2">
                {displayedRuns.map((run) =>
                  (() => {
                    const recency = runRecencyLabel(run.updated_at_ms);
                    const isSelected = selectedRunId === run.coder_run_id;
                    return (
                      <button
                        key={run.coder_run_id}
                        type="button"
                        onClick={() => setSelectedRunId(run.coder_run_id)}
                        className={cn(
                          "w-full rounded-2xl border px-4 py-3 text-left transition-colors",
                          isSelected
                            ? "border-primary/40 bg-primary/10"
                            : "border-border bg-surface hover:bg-surface-elevated"
                        )}
                      >
                        <div className="flex items-center justify-between gap-3">
                          <div className="min-w-0">
                            <p className="truncate text-sm font-semibold text-text">
                              {run.coder_run_id}
                            </p>
                            <p className="truncate text-xs text-text-muted">
                              {run.repo_binding?.repo_slug} • {run.workflow_mode}
                            </p>
                          </div>
                          <span
                            className={cn(
                              "rounded-full border px-2 py-1 text-[10px] font-medium uppercase tracking-[0.2em]",
                              statusTone(run.status)
                            )}
                          >
                            {run.status ?? "unknown"}
                          </span>
                        </div>
                        <div className="mt-3 flex flex-wrap gap-2">
                          <span className="rounded-full border border-border bg-surface-elevated/40 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                            {run.phase ?? "analysis"}
                          </span>
                          <span
                            className={cn(
                              "rounded-full border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em]",
                              runRecencyTone(recency)
                            )}
                          >
                            {recency}
                          </span>
                          {run.github_ref ? (
                            <span className="rounded-full border border-border bg-surface-elevated/40 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                              {run.github_ref.kind === "pull_request" ? "PR" : "Issue"} #
                              {run.github_ref.number}
                            </span>
                          ) : null}
                          {run.source_client ? (
                            <span className="rounded-full border border-border bg-surface-elevated/40 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                              {run.source_client}
                            </span>
                          ) : null}
                          {run.status === "awaiting_approval" ? (
                            <span className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-amber-100">
                              Approval queue
                            </span>
                          ) : null}
                          {runNeedsAttention(run) ? (
                            <span className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-amber-100">
                              Needs attention
                            </span>
                          ) : null}
                        </div>
                        {isSelected ? (
                          <div className="mt-3 grid grid-cols-3 gap-2">
                            {[
                              ["tasks", selectedRunOverview.tasks],
                              ["artifacts", selectedRunOverview.artifacts],
                              ["validation", selectedRunOverview.validationTasks],
                            ].map(([label, value]) => (
                              <div
                                key={label}
                                className="rounded-xl border border-primary/20 bg-surface px-2 py-1.5"
                              >
                                <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                  {label}
                                </p>
                                <p className="mt-1 text-xs font-semibold text-text">
                                  {String(value)}
                                </p>
                              </div>
                            ))}
                          </div>
                        ) : null}
                        <div className="mt-3 flex items-center justify-between gap-3 text-[11px] text-text-muted">
                          <span>{run.workflow_mode}</span>
                          <span>{formatTimestamp(run.updated_at_ms)}</span>
                        </div>
                      </button>
                    );
                  })()
                )}
              </div>
            )}
          </CardContent>
        </Card>

        <div className="min-h-0 overflow-y-auto">
          {!selectedRun ? (
            <Card>
              <CardContent className="flex min-h-[280px] flex-col items-center justify-center gap-3 text-center">
                <Workflow className="h-8 w-8 text-text-muted" />
                <div>
                  <p className="text-sm font-medium text-text">Select a coder run.</p>
                  <p className="text-xs text-text-muted">
                    Run detail, artifacts, memory hits, and candidates appear here.
                  </p>
                </div>
              </CardContent>
            </Card>
          ) : (
            <div className="space-y-4">
              <Card>
                <CardHeader>
                  <div className="flex flex-wrap items-start justify-between gap-4">
                    <div>
                      <CardTitle className="text-xl">{selectedRun.coder_run_id}</CardTitle>
                      <CardDescription>
                        {selectedRun.repo_binding.repo_slug} • {selectedRun.workflow_mode} •{" "}
                        {selectedRun.github_ref
                          ? `${selectedRun.github_ref.kind} #${selectedRun.github_ref.number}`
                          : "no GitHub ref"}
                      </CardDescription>
                      <div className="mt-3 flex flex-wrap gap-2">
                        {selectedDuplicateMatches.length > 0 ? (
                          <button
                            type="button"
                            onClick={() => setDetailTab("artifacts")}
                            className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-amber-100 transition-colors hover:bg-amber-500/15"
                          >
                            {selectedDuplicateMatches.length} duplicate matches
                          </button>
                        ) : null}
                        {selectedRunOverview.duplicateArtifacts > 0 ? (
                          <button
                            type="button"
                            onClick={() => {
                              if (latestDuplicateArtifact) {
                                setSelectedArtifactPath(latestDuplicateArtifact.path);
                              }
                              setDetailTab("artifacts");
                            }}
                            className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-amber-100 transition-colors hover:bg-amber-500/15"
                          >
                            {selectedRunOverview.duplicateArtifacts} duplicate artifacts
                          </button>
                        ) : null}
                        {selectedValidationSummary?.validationsAttempted ? (
                          <button
                            type="button"
                            onClick={() => setDetailTab("validation")}
                            className="rounded-full border border-violet-500/20 bg-violet-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-violet-200 transition-colors hover:bg-violet-500/15"
                          >
                            {selectedValidationSummary.validationsAttempted} validation checks
                          </button>
                        ) : null}
                        {memoryHits.length > 0 ? (
                          <button
                            type="button"
                            onClick={() => setDetailTab("memory")}
                            className="rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-emerald-200 transition-colors hover:bg-emerald-500/15"
                          >
                            {memoryHits.length} memory hits
                          </button>
                        ) : null}
                        {memoryCandidates.length > 0 ? (
                          <button
                            type="button"
                            onClick={() => setDetailTab("memory")}
                            className="rounded-full border border-sky-500/20 bg-sky-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-sky-200 transition-colors hover:bg-sky-500/15"
                          >
                            {memoryCandidates.length} candidates
                          </button>
                        ) : null}
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <Button
                        variant="secondary"
                        size="sm"
                        loading={loadingDetail}
                        onClick={() => selectedRunId && void loadRunDetail(selectedRunId)}
                      >
                        <RefreshCw className="h-4 w-4" />
                        Refresh
                      </Button>
                      <Button
                        variant="primary"
                        size="sm"
                        loading={acting === "approve"}
                        onClick={() => void handleAction("approve")}
                      >
                        <SquareCheckBig className="h-4 w-4" />
                        Approve
                      </Button>
                      <Button
                        variant="danger"
                        size="sm"
                        loading={acting === "cancel"}
                        onClick={() => void handleAction("cancel")}
                      >
                        <SquareX className="h-4 w-4" />
                        Cancel
                      </Button>
                    </div>
                  </div>
                </CardHeader>
                <CardContent className="grid gap-3 md:grid-cols-4">
                  {[
                    {
                      label: "Status",
                      value: selectedRun.status ?? "unknown",
                      onClick: () => {
                        const status = selectedRun.status ?? "unknown";
                        setStatusFilter(status);
                        setRunSortMode(
                          status === "awaiting_approval"
                            ? "approval"
                            : runNeedsAttention(selectedRun)
                              ? "attention"
                              : "updated"
                        );
                      },
                    },
                    {
                      label: "Phase",
                      value: selectedRun.phase ?? "analysis",
                      onClick: () => setDetailTab("overview"),
                    },
                    {
                      label: "Context Run",
                      value: copiedContextRun ? "Copied" : selectedRun.linked_context_run_id,
                      onClick: () => void copyContextRunId(),
                    },
                    {
                      label: "Updated",
                      value: formatTimestamp(selectedRun.updated_at_ms),
                      onClick: () => selectedRunId && void loadRunDetail(selectedRunId),
                    },
                  ].map(({ label, value, onClick }) => (
                    <button
                      key={label}
                      type="button"
                      onClick={onClick}
                      className="rounded-2xl border border-border bg-surface-elevated/50 p-3 text-left transition-colors hover:bg-surface-elevated"
                    >
                      <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                        {label}
                      </p>
                      <p className="mt-1 text-sm font-medium text-text">{value}</p>
                    </button>
                  ))}
                </CardContent>
                <CardContent className="pt-0">
                  <div className="mb-3 flex flex-wrap gap-2">
                    {latestValidationArtifact ? (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => {
                          setSelectedArtifactPath(latestValidationArtifact.path);
                          setDetailTab("validation");
                        }}
                      >
                        <SquareCheckBig className="h-4 w-4" />
                        Latest validation
                      </Button>
                    ) : null}
                    {latestDuplicateArtifact ? (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => {
                          setSelectedArtifactPath(latestDuplicateArtifact.path);
                          setDetailTab("artifacts");
                        }}
                      >
                        <Database className="h-4 w-4" />
                        Latest duplicate
                      </Button>
                    ) : null}
                    {latestTriageArtifact ? (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => {
                          setSelectedArtifactPath(latestTriageArtifact.path);
                          setDetailTab("artifacts");
                        }}
                      >
                        <Database className="h-4 w-4" />
                        Latest triage
                      </Button>
                    ) : null}
                    {latestMemoryArtifact ? (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => {
                          setSelectedArtifactPath(latestMemoryArtifact.path);
                          setDetailTab("memory");
                        }}
                      >
                        <Brain className="h-4 w-4" />
                        Latest memory
                      </Button>
                    ) : null}
                  </div>
                  <div className="grid gap-3 md:grid-cols-4">
                    {[
                      ["Tasks", selectedRunOverview.tasks],
                      ["Decisions", selectedRunOverview.decisions],
                      ["Artifacts", selectedRunOverview.artifacts],
                      ["Validation tasks", selectedRunOverview.validationTasks],
                    ].map(([label, value]) => (
                      <button
                        key={label}
                        type="button"
                        onClick={() => {
                          if (label === "Tasks") {
                            focusOverviewSection("kanban");
                          } else if (label === "Decisions") {
                            focusOverviewSection("blackboard");
                          } else if (label === "Artifacts") {
                            setDetailTab("artifacts");
                          } else if (label === "Validation tasks") {
                            setDetailTab("validation");
                          } else {
                            setDetailTab("overview");
                          }
                        }}
                        className="rounded-2xl border border-border bg-surface-elevated/30 p-3 text-left transition-colors hover:bg-surface-elevated"
                      >
                        <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                          {label}
                        </p>
                        <p className="mt-1 text-lg font-semibold text-text">{String(value)}</p>
                      </button>
                    ))}
                  </div>
                </CardContent>
              </Card>

              {error ? (
                <Card className="border-rose-500/40 bg-rose-500/10">
                  <CardContent className="flex items-center gap-3 py-4">
                    <AlertCircle className="h-5 w-5 text-rose-200" />
                    <div className="min-w-0 flex-1">
                      <p className="text-sm text-rose-100">{error}</p>
                      {readinessHint ? (
                        <p className="mt-1 text-xs text-rose-200/90">{readinessHint}</p>
                      ) : null}
                    </div>
                    {readinessHint && onOpenMcpSettings ? (
                      <Button variant="secondary" size="sm" onClick={onOpenMcpSettings}>
                        <ExternalLink className="h-4 w-4" />
                        Open MCP
                      </Button>
                    ) : null}
                  </CardContent>
                </Card>
              ) : null}

              <div className="flex flex-wrap gap-2">
                {detailTabMeta.map(({ key, label, count }) => (
                  <button
                    key={key}
                    type="button"
                    onClick={() => setDetailTab(key)}
                    className={cn(
                      "rounded-full border px-3 py-1.5 text-xs font-medium transition-colors",
                      detailTab === key
                        ? "border-primary/40 bg-primary/10 text-primary"
                        : "border-border bg-surface text-text-muted hover:text-text"
                    )}
                  >
                    <span className="inline-flex items-center gap-2">
                      <span>{label}</span>
                      <span
                        className={cn(
                          "rounded-full border px-1.5 py-0.5 text-[10px] leading-none",
                          detailTab === key
                            ? "border-primary/30 bg-primary/10 text-primary"
                            : "border-border bg-surface-elevated/40 text-text-muted"
                        )}
                      >
                        {count}
                      </span>
                    </span>
                  </button>
                ))}
              </div>

              {detailTab === "overview" ? (
                <div className="flex flex-wrap gap-2">
                  {[
                    ["kanban", "Kanban"],
                    ["blackboard", "Blackboard"],
                    ["timeline", "Timeline"],
                    ["memory", "Memory"],
                  ].map(([key, label]) => (
                    <button
                      key={key}
                      type="button"
                      onClick={() =>
                        focusOverviewSection(key as "kanban" | "blackboard" | "timeline" | "memory")
                      }
                      className="rounded-full border border-border bg-surface-elevated/30 px-3 py-1.5 text-xs font-medium text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                    >
                      Jump to {label}
                    </button>
                  ))}
                </div>
              ) : null}

              {detailTab === "overview" ? (
                <>
                  <div ref={kanbanRef}>
                    <Card>
                      <CardHeader>
                        <CardTitle className="flex items-center gap-2 text-base">
                          <PanelsTopLeft className="h-4 w-4" />
                          Coder Kanban
                        </CardTitle>
                        <CardDescription>
                          Projected directly from engine task state.
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="overflow-x-auto">
                        <div className="grid min-w-[980px] grid-cols-5 gap-3">
                          {taskColumns.map((column) => (
                            <div
                              key={column.key}
                              className="rounded-2xl border border-border bg-surface-elevated/30 p-3"
                            >
                              <div className="mb-3 flex items-center justify-between gap-2">
                                <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-muted">
                                  {column.label}
                                </p>
                                <span className="rounded-full border border-border px-2 py-0.5 text-[10px] text-text-muted">
                                  {column.tasks.length}
                                </span>
                              </div>
                              <div className="space-y-2">
                                {column.tasks.length === 0 ? (
                                  <div className="rounded-xl border border-dashed border-border px-3 py-4 text-center text-xs text-text-muted">
                                    No tasks
                                  </div>
                                ) : (
                                  column.tasks.map((task, index) =>
                                    (() => {
                                      const relatedArtifacts = relatedArtifactsForTask(
                                        artifacts,
                                        task
                                      ).slice(0, 3);
                                      return (
                                        <div
                                          key={String(
                                            task.id ?? task.command_id ?? `${column.key}-${index}`
                                          )}
                                          className="rounded-xl border border-border bg-surface p-3"
                                        >
                                          <p className="text-sm font-medium text-text">
                                            {String(
                                              task.title ??
                                                task.workflow_node_id ??
                                                task.task_type ??
                                                task.id ??
                                                "task"
                                            )}
                                          </p>
                                          <p className="mt-1 text-[11px] text-text-muted">
                                            {String(task.workflow_node_id ?? task.task_type ?? "")}
                                          </p>
                                          {relatedArtifacts.length > 0 ? (
                                            <div className="mt-3 flex flex-wrap gap-2">
                                              {relatedArtifacts.map((artifact) => (
                                                <button
                                                  key={artifact.id}
                                                  type="button"
                                                  onClick={() => {
                                                    setSelectedArtifactPath(artifact.path);
                                                    setDetailTab(
                                                      artifactCategory(artifact) === "validation"
                                                        ? "validation"
                                                        : "artifacts"
                                                    );
                                                  }}
                                                  className="rounded-full border border-border bg-surface-elevated/40 px-2 py-1 text-[10px] uppercase tracking-[0.16em] text-text-muted transition-colors hover:bg-surface-elevated"
                                                >
                                                  {artifact.artifact_type}
                                                </button>
                                              ))}
                                            </div>
                                          ) : null}
                                        </div>
                                      );
                                    })()
                                  )
                                )}
                              </div>
                            </div>
                          ))}
                        </div>
                      </CardContent>
                    </Card>
                  </div>

                  <div className="grid gap-4 xl:grid-cols-2">
                    <div ref={blackboardRef}>
                      <Card>
                        <CardHeader>
                          <CardTitle className="flex items-center gap-2 text-base">
                            <GitBranch className="h-4 w-4" />
                            Blackboard And Decisions
                          </CardTitle>
                        </CardHeader>
                        <CardContent className="space-y-4">
                          {selectedBlackboard ? (
                            <>
                              <div className="grid gap-3 md:grid-cols-3">
                                {[
                                  ["Decisions", decisions.length],
                                  ["Open questions", openQuestions.length],
                                  [
                                    "Artifacts",
                                    Array.isArray(selectedBlackboard.artifacts)
                                      ? selectedBlackboard.artifacts.length
                                      : 0,
                                  ],
                                ].map(([label, value]) => (
                                  <div
                                    key={label}
                                    className="rounded-2xl border border-border bg-surface-elevated/40 p-3"
                                  >
                                    <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                      {label}
                                    </p>
                                    <p className="mt-1 text-lg font-semibold text-text">
                                      {String(value)}
                                    </p>
                                  </div>
                                ))}
                              </div>
                              {latestDecision ? (
                                <div className="rounded-3xl border border-primary/20 bg-primary/5 p-4">
                                  <div className="flex flex-wrap items-start justify-between gap-3">
                                    <div>
                                      <p className="text-[11px] uppercase tracking-[0.24em] text-primary/80">
                                        Current decision
                                      </p>
                                      <p className="mt-2 text-sm font-medium text-text">
                                        {blackboardRowText(latestDecision)}
                                      </p>
                                    </div>
                                    <p className="text-xs text-text-muted">
                                      {formatTimestamp(blackboardRowTimestamp(latestDecision))}
                                    </p>
                                  </div>
                                  <div className="mt-3 flex flex-wrap gap-2">
                                    {blackboardRowStepId(latestDecision) ? (
                                      <span className="rounded-full border border-primary/20 bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.2em] text-text-muted">
                                        Step {blackboardRowStepId(latestDecision)}
                                      </span>
                                    ) : null}
                                    {blackboardRowSourceEventId(latestDecision) ? (
                                      <span className="rounded-full border border-primary/20 bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.2em] text-text-muted">
                                        Event {blackboardRowSourceEventId(latestDecision)}
                                      </span>
                                    ) : null}
                                  </div>
                                </div>
                              ) : (
                                <p className="text-sm text-text-muted">
                                  No blackboard decisions yet.
                                </p>
                              )}
                              {blackboardTimeline.length > 0 ? (
                                <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_260px]">
                                  <div className="rounded-3xl border border-border bg-surface-elevated/30 p-4">
                                    <div className="mb-4 flex items-center justify-between gap-3">
                                      <div>
                                        <p className="text-sm font-medium text-text">
                                          Decision lineage
                                        </p>
                                        <p className="text-xs text-text-muted">
                                          Chronological blackboard activity from the current run.
                                        </p>
                                      </div>
                                      <span className="rounded-full border border-border px-2.5 py-1 text-[10px] uppercase tracking-[0.2em] text-text-muted">
                                        {blackboardTimeline.length} items
                                      </span>
                                    </div>
                                    <div className="space-y-3">
                                      {blackboardTimeline.map((item, index) => (
                                        <div key={item.id} className="flex gap-3">
                                          <div className="flex w-10 flex-col items-center">
                                            <span
                                              className={cn(
                                                "mt-1 flex h-8 w-8 items-center justify-center rounded-full border text-[10px] font-semibold uppercase tracking-[0.16em]",
                                                item.kind === "decision"
                                                  ? "border-primary/30 bg-primary/10 text-primary"
                                                  : "border-amber-500/30 bg-amber-500/10 text-amber-100"
                                              )}
                                            >
                                              {item.kind === "decision" ? "D" : "Q"}
                                            </span>
                                            {index < blackboardTimeline.length - 1 ? (
                                              <div className="mt-2 h-full min-h-10 w-px bg-border" />
                                            ) : null}
                                          </div>
                                          <div className="min-w-0 flex-1 rounded-2xl border border-border bg-surface p-3">
                                            <div className="flex flex-wrap items-center gap-2">
                                              <span
                                                className={cn(
                                                  "rounded-full border px-2 py-0.5 text-[10px] uppercase tracking-[0.18em]",
                                                  item.kind === "decision"
                                                    ? "border-primary/20 bg-primary/10 text-primary"
                                                    : "border-amber-500/20 bg-amber-500/10 text-amber-100"
                                                )}
                                              >
                                                {item.kind === "decision"
                                                  ? "Decision"
                                                  : "Open question"}
                                              </span>
                                              <span className="text-[11px] text-text-muted">
                                                {formatTimestamp(item.tsMs)}
                                              </span>
                                            </div>
                                            <p className="mt-2 whitespace-pre-wrap break-words text-sm text-text">
                                              {item.text}
                                            </p>
                                            <div className="mt-3 flex flex-wrap gap-2">
                                              {item.stepId ? (
                                                <span className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                                  Step {item.stepId}
                                                </span>
                                              ) : null}
                                              {item.sourceEventId ? (
                                                <span className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                                  Source {item.sourceEventId}
                                                </span>
                                              ) : null}
                                            </div>
                                          </div>
                                        </div>
                                      ))}
                                    </div>
                                  </div>

                                  <div className="space-y-3">
                                    <div className="rounded-3xl border border-border bg-surface-elevated/30 p-4">
                                      <p className="text-sm font-medium text-text">
                                        Open questions
                                      </p>
                                      <p className="mt-1 text-xs text-text-muted">
                                        Outstanding uncertainty captured on the blackboard.
                                      </p>
                                      <div className="mt-3 space-y-2">
                                        {openQuestions.length > 0 ? (
                                          openQuestions.slice(0, 4).map((question, index) => (
                                            <div
                                              key={String(
                                                question.id ??
                                                  question.source_event_id ??
                                                  `open-question-${index}`
                                              )}
                                              className="rounded-2xl border border-border bg-surface p-3"
                                            >
                                              <p className="text-sm text-text">
                                                {blackboardRowText(question)}
                                              </p>
                                              <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-text-muted">
                                                <span>
                                                  {formatTimestamp(
                                                    blackboardRowTimestamp(question)
                                                  )}
                                                </span>
                                                {blackboardRowStepId(question) ? (
                                                  <span>step {blackboardRowStepId(question)}</span>
                                                ) : null}
                                              </div>
                                            </div>
                                          ))
                                        ) : (
                                          <p className="text-sm text-text-muted">
                                            No open questions recorded.
                                          </p>
                                        )}
                                      </div>
                                    </div>

                                    <div className="rounded-3xl border border-border bg-surface-elevated/30 p-4">
                                      <p className="text-sm font-medium text-text">Lineage notes</p>
                                      <div className="mt-3 space-y-2 text-xs text-text-muted">
                                        <p>
                                          Decisions and questions are ordered by their recorded
                                          blackboard timestamp.
                                        </p>
                                        <p>
                                          Step and source ids come directly from blackboard rows so
                                          you can correlate them with run tasks and artifacts.
                                        </p>
                                      </div>
                                    </div>
                                  </div>
                                </div>
                              ) : null}
                            </>
                          ) : (
                            <p className="text-sm text-text-muted">
                              No blackboard payload returned for this run.
                            </p>
                          )}
                        </CardContent>
                      </Card>
                    </div>

                    <div ref={timelineRef}>
                      <Card>
                        <CardHeader>
                          <div className="flex items-center justify-between gap-3">
                            <div>
                              <CardTitle className="flex items-center gap-2 text-base">
                                <Workflow className="h-4 w-4" />
                                Run Timeline
                              </CardTitle>
                              <CardDescription>
                                Recent engine events from the linked context run.
                              </CardDescription>
                            </div>
                            <Button
                              variant="secondary"
                              size="sm"
                              onClick={() => setDetailTab("artifacts")}
                            >
                              <Database className="h-4 w-4" />
                              Open artifacts
                            </Button>
                          </div>
                        </CardHeader>
                        <CardContent className="space-y-3">
                          <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_220px]">
                            <div className="flex items-center gap-2 rounded-2xl border border-border bg-surface-elevated/40 px-3 py-2">
                              <Search className="h-4 w-4 text-text-muted" />
                              <input
                                value={eventQuery}
                                onChange={(event) => setEventQuery(event.target.value)}
                                placeholder="Filter events by type, step, id, or text"
                                className="w-full bg-transparent text-sm text-text outline-none placeholder:text-text-subtle"
                              />
                            </div>
                            <select
                              value={eventTypeFilter}
                              onChange={(event) => setEventTypeFilter(event.target.value)}
                              className="rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text outline-none"
                            >
                              {eventTypes.map((type) => (
                                <option key={type} value={type}>
                                  {type === "all" ? "All event types" : type}
                                </option>
                              ))}
                            </select>
                          </div>
                          {filteredEventTimeline.length === 0 ? (
                            <p className="text-sm text-text-muted">
                              No run events match the current filters.
                            </p>
                          ) : (
                            filteredEventTimeline.map((event, index) => {
                              const eventType = runEventType(event);
                              const eventText = runEventText(event);
                              const stepId = pickText(event.step_id);
                              const sourceEventId = pickText(event.event_id);
                              const relatedArtifacts = relatedArtifactsForEvent(
                                artifacts,
                                stepId,
                                sourceEventId
                              ).slice(0, 3);
                              return (
                                <div key={runEventId(event, index)} className="flex gap-3">
                                  <div className="flex w-10 flex-col items-center">
                                    <span className="mt-1 flex h-8 w-8 items-center justify-center rounded-full border border-sky-500/30 bg-sky-500/10 text-[10px] font-semibold uppercase tracking-[0.16em] text-sky-200">
                                      {eventType.slice(0, 1).toUpperCase()}
                                    </span>
                                    {index < filteredEventTimeline.length - 1 ? (
                                      <div className="mt-2 h-full min-h-10 w-px bg-border" />
                                    ) : null}
                                  </div>
                                  <div className="min-w-0 flex-1 rounded-2xl border border-border bg-surface p-3">
                                    <div className="flex flex-wrap items-center gap-2">
                                      <span className="rounded-full border border-sky-500/20 bg-sky-500/10 px-2 py-0.5 text-[10px] uppercase tracking-[0.18em] text-sky-200">
                                        {eventType.replace(/_/g, " ")}
                                      </span>
                                      <span className="text-[11px] text-text-muted">
                                        {formatTimestamp(runEventTimestamp(event))}
                                      </span>
                                    </div>
                                    {eventText ? (
                                      <p className="mt-2 whitespace-pre-wrap break-words text-sm text-text">
                                        {eventText}
                                      </p>
                                    ) : null}
                                    <div className="mt-3 flex flex-wrap gap-2">
                                      {stepId ? (
                                        <span className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                          Step {stepId}
                                        </span>
                                      ) : null}
                                      {sourceEventId ? (
                                        <span className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                          Event {sourceEventId}
                                        </span>
                                      ) : null}
                                    </div>
                                    {relatedArtifacts.length > 0 ? (
                                      <div className="mt-3 rounded-2xl border border-border bg-surface-elevated/40 p-3">
                                        <div className="mb-2 flex items-center justify-between gap-3">
                                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                            Related artifacts
                                          </p>
                                          <span className="text-[11px] text-text-muted">
                                            {relatedArtifacts.length}
                                          </span>
                                        </div>
                                        <div className="space-y-2">
                                          {relatedArtifacts.map((artifact) => (
                                            <button
                                              key={artifact.id}
                                              type="button"
                                              onClick={() => {
                                                setSelectedArtifactPath(artifact.path);
                                                setDetailTab("artifacts");
                                              }}
                                              className="w-full rounded-2xl border border-border bg-surface px-3 py-2 text-left transition-colors hover:bg-surface-elevated"
                                            >
                                              <div className="flex items-center justify-between gap-3">
                                                <span className="text-sm font-medium text-text">
                                                  {artifact.artifact_type}
                                                </span>
                                                <span className="text-[11px] text-text-muted">
                                                  {formatTimestamp(artifact.ts_ms)}
                                                </span>
                                              </div>
                                              <p className="mt-1 break-all font-mono text-[11px] text-text-muted">
                                                {artifact.path}
                                              </p>
                                            </button>
                                          ))}
                                        </div>
                                      </div>
                                    ) : null}
                                  </div>
                                </div>
                              );
                            })
                          )}
                        </CardContent>
                      </Card>
                    </div>

                    <div ref={memorySnapshotRef}>
                      <Card>
                        <CardHeader>
                          <div className="flex items-center justify-between gap-3">
                            <div>
                              <CardTitle className="flex items-center gap-2 text-base">
                                <Brain className="h-4 w-4" />
                                Memory Snapshot
                              </CardTitle>
                            </div>
                            <Button
                              variant="secondary"
                              size="sm"
                              onClick={() => setDetailTab("memory")}
                            >
                              <Brain className="h-4 w-4" />
                              Open memory
                            </Button>
                          </div>
                        </CardHeader>
                        <CardContent className="space-y-3">
                          <div className="grid gap-3 md:grid-cols-2">
                            <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Hits
                              </p>
                              <p className="mt-1 text-lg font-semibold text-text">
                                {memoryHits.length}
                              </p>
                            </div>
                            <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Candidates
                              </p>
                              <p className="mt-1 text-lg font-semibold text-text">
                                {memoryCandidates.length}
                              </p>
                            </div>
                          </div>
                          <div className="space-y-2">
                            {memoryHits.slice(0, 3).map((hit, index) => (
                              <div
                                key={String(hit.candidate_id ?? hit.memory_id ?? index)}
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3"
                              >
                                <pre className="whitespace-pre-wrap break-words text-[11px] text-text-muted">
                                  {renderValue(hit.summary ?? hit.content ?? hit.payload ?? hit)}
                                </pre>
                              </div>
                            ))}
                          </div>
                        </CardContent>
                      </Card>
                    </div>
                  </div>
                </>
              ) : null}

              {detailTab === "artifacts" ? (
                <div className="grid gap-4 xl:grid-cols-2">
                  <Card>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2 text-base">
                        <Database className="h-4 w-4" />
                        Artifact Feed
                      </CardTitle>
                      <CardDescription>
                        Includes duplicate and memory-backed history from engine artifacts.
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-4">
                      {artifacts.length === 0 ? (
                        <p className="text-sm text-text-muted">No artifacts yet.</p>
                      ) : (
                        <>
                          <div className="flex items-center gap-2 rounded-2xl border border-border bg-surface-elevated/40 px-3 py-2">
                            <Search className="h-4 w-4 text-text-muted" />
                            <input
                              value={artifactQuery}
                              onChange={(event) => setArtifactQuery(event.target.value)}
                              placeholder="Filter artifacts by type, path, step, or event"
                              className="w-full bg-transparent text-sm text-text outline-none placeholder:text-text-subtle"
                            />
                          </div>
                          <div className="grid gap-3 md:grid-cols-2">
                            {[
                              {
                                label: "Duplicate",
                                group: artifactGroups.find((group) => group.key === "duplicate"),
                              },
                              {
                                label: "Triage",
                                group: artifactGroups.find((group) => group.key === "triage"),
                              },
                              {
                                label: "Memory",
                                group: artifactGroups.find((group) => group.key === "memory"),
                              },
                              {
                                label: "Validation",
                                group: artifactGroups.find((group) => group.key === "validation"),
                              },
                            ].map(({ label, group }) => (
                              <div
                                key={label}
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3"
                              >
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  {label}
                                </p>
                                <p className="mt-1 text-lg font-semibold text-text">
                                  {String(group?.artifacts.length ?? 0)}
                                </p>
                              </div>
                            ))}
                          </div>

                          <div className="space-y-4">
                            {artifactGroups.length === 0 ? (
                              <p className="text-sm text-text-muted">
                                No artifacts match the current filter.
                              </p>
                            ) : null}
                            {(artifactGroups.length > 0 ? artifactGroups : []).map((group) => (
                              <div
                                key={group.key}
                                className="rounded-3xl border border-border bg-surface-elevated/20 p-3"
                              >
                                <div className="mb-3 flex items-start justify-between gap-3">
                                  <div>
                                    <p className="text-sm font-medium text-text">{group.label}</p>
                                    <p className="text-xs text-text-muted">{group.description}</p>
                                  </div>
                                  <span
                                    className={cn(
                                      "rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em]",
                                      artifactCategoryTone(group.key)
                                    )}
                                  >
                                    {group.artifacts.length}
                                  </span>
                                </div>
                                <div className="space-y-2">
                                  {group.artifacts.map((artifact) => (
                                    <button
                                      key={artifact.id}
                                      type="button"
                                      onClick={() => setSelectedArtifactPath(artifact.path)}
                                      className={cn(
                                        "w-full rounded-2xl border p-3 text-left",
                                        selectedArtifactPath === artifact.path
                                          ? "border-primary/40 bg-primary/10"
                                          : "border-border bg-surface"
                                      )}
                                    >
                                      <div className="flex items-center justify-between gap-3">
                                        <p className="text-sm font-medium text-text">
                                          {artifact.artifact_type}
                                        </p>
                                        <span className="text-xs text-text-muted">
                                          {formatTimestamp(artifact.ts_ms)}
                                        </span>
                                      </div>
                                      <div className="mt-2 flex flex-wrap gap-2">
                                        {artifact.step_id ? (
                                          <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                            Step {artifact.step_id}
                                          </span>
                                        ) : null}
                                        {artifact.source_event_id ? (
                                          <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                            Event {artifact.source_event_id}
                                          </span>
                                        ) : null}
                                      </div>
                                      <p className="mt-2 break-all font-mono text-[11px] text-text-muted">
                                        {artifact.path}
                                      </p>
                                    </button>
                                  ))}
                                </div>
                              </div>
                            ))}
                          </div>
                        </>
                      )}
                    </CardContent>
                  </Card>

                  <Card>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2 text-base">
                        <Database className="h-4 w-4" />
                        Artifact Inspector
                      </CardTitle>
                    </CardHeader>
                    <CardContent className="space-y-3">
                      {selectedArtifactPath ? (
                        <>
                          <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                            <div className="flex items-start justify-between gap-3">
                              <div>
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  Selected
                                </p>
                                <p className="mt-2 break-all font-mono text-[11px] text-text-muted">
                                  {selectedArtifactPath}
                                </p>
                              </div>
                              {selectedArtifactRecord ? (
                                <span
                                  className={cn(
                                    "rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em]",
                                    artifactCategoryTone(artifactCategory(selectedArtifactRecord))
                                  )}
                                >
                                  {artifactCategory(selectedArtifactRecord)}
                                </span>
                              ) : null}
                            </div>
                          </div>
                          <div className="flex flex-wrap gap-2">
                            {selectedDuplicateMatches.length > 0 ? (
                              <button
                                type="button"
                                onClick={() => setDetailTab("artifacts")}
                                className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-amber-100 transition-colors hover:bg-amber-500/15"
                              >
                                {selectedDuplicateMatches.length} duplicate matches
                              </button>
                            ) : null}
                            {selectedValidationSummary?.outcome ? (
                              <button
                                type="button"
                                onClick={() => setDetailTab("validation")}
                                className="rounded-full border border-violet-500/20 bg-violet-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-violet-200 transition-colors hover:bg-violet-500/15"
                              >
                                {selectedValidationSummary.outcome}
                              </button>
                            ) : null}
                            {selectedValidationSummary?.validationsAttempted ? (
                              <button
                                type="button"
                                onClick={() => setDetailTab("validation")}
                                className="rounded-full border border-violet-500/20 bg-violet-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-violet-200 transition-colors hover:bg-violet-500/15"
                              >
                                {selectedValidationSummary.validationsAttempted} checks
                              </button>
                            ) : null}
                          </div>

                          {selectedDuplicateMatches.length > 0 ? (
                            <div className="rounded-3xl border border-amber-500/20 bg-amber-500/5 p-4">
                              <div className="flex items-center justify-between gap-3">
                                <div>
                                  <p className="text-sm font-medium text-text">Duplicate history</p>
                                  <p className="text-xs text-text-muted">
                                    Parsed directly from the selected artifact payload.
                                  </p>
                                </div>
                                <span className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-amber-100">
                                  {selectedDuplicateMatches.length} matches
                                </span>
                              </div>
                              <div className="mt-3 space-y-2">
                                {selectedDuplicateMatches.slice(0, 5).map((match, index) => (
                                  <div
                                    key={String(match.id ?? match.candidate_id ?? index)}
                                    className="rounded-2xl border border-amber-500/20 bg-surface p-3"
                                  >
                                    <p className="text-sm font-medium text-text">
                                      {duplicateMatchLabel(match)}
                                    </p>
                                    <div className="mt-2 flex flex-wrap gap-2">
                                      {duplicateMatchBadges(match).map((badge) => (
                                        <span
                                          key={badge}
                                          className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted"
                                        >
                                          {badge}
                                        </span>
                                      ))}
                                    </div>
                                  </div>
                                ))}
                              </div>
                            </div>
                          ) : null}

                          {selectedValidationSummary ? (
                            <div className="grid gap-3 md:grid-cols-3">
                              <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  Outcome
                                </p>
                                <p className="mt-1 text-sm font-medium text-text">
                                  {selectedValidationSummary.outcome || "Unknown"}
                                </p>
                              </div>
                              <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  Passed
                                </p>
                                <p className="mt-1 text-sm font-medium text-text">
                                  {selectedValidationSummary.passed === null
                                    ? "Unknown"
                                    : selectedValidationSummary.passed
                                      ? "Yes"
                                      : "No"}
                                </p>
                              </div>
                              <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  Checks
                                </p>
                                <p className="mt-1 text-sm font-medium text-text">
                                  {selectedValidationSummary.validationsAttempted ?? 0}
                                </p>
                              </div>
                            </div>
                          ) : null}
                          {loadingArtifact ? (
                            <p className="text-sm text-text-muted">Loading artifact preview…</p>
                          ) : artifactPreview?.kind === "diff" ? (
                            <DiffViewer
                              oldValue={artifactPreview.oldValue}
                              newValue={artifactPreview.newValue}
                              oldTitle="Before"
                              newTitle="After"
                            />
                          ) : (
                            <pre className="max-h-[420px] overflow-auto rounded-2xl border border-border bg-surface-elevated/40 p-3 text-[11px] text-text-muted">
                              {artifactPreview?.value ?? "No artifact preview available."}
                            </pre>
                          )}
                        </>
                      ) : (
                        <p className="text-sm text-text-muted">
                          Select an artifact to inspect its contents.
                        </p>
                      )}
                    </CardContent>
                  </Card>
                </div>
              ) : null}

              {detailTab === "validation" ? (
                <div className="grid gap-4 xl:grid-cols-2">
                  <Card>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2 text-base">
                        <SquareCheckBig className="h-4 w-4" />
                        Validation Status
                      </CardTitle>
                      <CardDescription>
                        Validation tasks and artifacts already emitted by the engine.
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-4">
                      <div className="grid gap-3 md:grid-cols-3">
                        {[
                          ["Validation tasks", validationTasks.length],
                          ["Validation artifacts", validationArtifacts.length],
                          ["Selected checks", selectedValidationSummary?.validationsAttempted ?? 0],
                        ].map(([label, value]) => (
                          <div
                            key={label}
                            className="rounded-2xl border border-border bg-surface-elevated/40 p-3"
                          >
                            <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                              {label}
                            </p>
                            <p className="mt-1 text-lg font-semibold text-text">{String(value)}</p>
                          </div>
                        ))}
                      </div>

                      <div className="space-y-2">
                        <p className="text-sm font-medium text-text">Validation tasks</p>
                        {validationTasks.length === 0 ? (
                          <p className="text-sm text-text-muted">
                            No validation tasks are recorded for this run.
                          </p>
                        ) : (
                          validationTasks.map((task, index) => {
                            const relatedArtifacts = relatedArtifactsForTask(artifacts, task).slice(
                              0,
                              3
                            );
                            return (
                              <div
                                key={String(
                                  task.id ?? task.command_id ?? `validation-task-${index}`
                                )}
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3"
                              >
                                <div className="flex items-center justify-between gap-3">
                                  <p className="text-sm font-medium text-text">{taskLabel(task)}</p>
                                  <span
                                    className={cn(
                                      "rounded-full border px-2 py-1 text-[10px] font-medium uppercase tracking-[0.18em]",
                                      statusTone(pickText(task.status))
                                    )}
                                  >
                                    {pickText(task.status) || "unknown"}
                                  </span>
                                </div>
                                <p className="mt-2 text-[11px] text-text-muted">
                                  {pickText(task.workflow_node_id) ||
                                    pickText(task.task_type) ||
                                    "validation"}
                                </p>
                                {relatedArtifacts.length > 0 ? (
                                  <div className="mt-3 flex flex-wrap gap-2">
                                    {relatedArtifacts.map((artifact) => (
                                      <button
                                        key={artifact.id}
                                        type="button"
                                        onClick={() => {
                                          setSelectedArtifactPath(artifact.path);
                                          setDetailTab("validation");
                                        }}
                                        className="rounded-full border border-violet-500/20 bg-violet-500/10 px-2 py-1 text-[10px] uppercase tracking-[0.16em] text-violet-200"
                                      >
                                        {artifact.artifact_type}
                                      </button>
                                    ))}
                                  </div>
                                ) : null}
                              </div>
                            );
                          })
                        )}
                      </div>

                      <div className="space-y-2">
                        <p className="text-sm font-medium text-text">Validation artifacts</p>
                        {validationArtifacts.length === 0 ? (
                          <p className="text-sm text-text-muted">
                            No validation artifacts have been emitted yet.
                          </p>
                        ) : (
                          validationArtifacts.map((artifact) => (
                            <button
                              key={artifact.id}
                              type="button"
                              onClick={() => {
                                setSelectedArtifactPath(artifact.path);
                                setDetailTab("validation");
                              }}
                              className={cn(
                                "w-full rounded-2xl border p-3 text-left",
                                selectedArtifactPath === artifact.path
                                  ? "border-primary/40 bg-primary/10"
                                  : "border-border bg-surface-elevated/40"
                              )}
                            >
                              <div className="flex items-center justify-between gap-3">
                                <p className="text-sm font-medium text-text">
                                  {artifact.artifact_type}
                                </p>
                                <span className="text-xs text-text-muted">
                                  {formatTimestamp(artifact.ts_ms)}
                                </span>
                              </div>
                              <div className="mt-2 flex flex-wrap gap-2">
                                {artifact.step_id ? (
                                  <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                    Step {artifact.step_id}
                                  </span>
                                ) : null}
                                {artifact.source_event_id ? (
                                  <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                    Event {artifact.source_event_id}
                                  </span>
                                ) : null}
                              </div>
                              <p className="mt-2 break-all font-mono text-[11px] text-text-muted">
                                {artifact.path}
                              </p>
                            </button>
                          ))
                        )}
                      </div>
                    </CardContent>
                  </Card>

                  <Card>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2 text-base">
                        <Database className="h-4 w-4" />
                        Validation Inspector
                      </CardTitle>
                      <CardDescription>
                        Parsed pass/fail metadata when the selected artifact exposes it.
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-3">
                      {selectedArtifactPath ? (
                        <>
                          <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                            <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                              Selected artifact
                            </p>
                            <p className="mt-2 break-all font-mono text-[11px] text-text-muted">
                              {selectedArtifactPath}
                            </p>
                          </div>
                          <div className="grid gap-3 md:grid-cols-3">
                            <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Outcome
                              </p>
                              <p className="mt-1 text-sm font-medium text-text">
                                {selectedValidationSummary?.outcome || "Unknown"}
                              </p>
                            </div>
                            <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Passed
                              </p>
                              <p className="mt-1 text-sm font-medium text-text">
                                {selectedValidationSummary?.passed === null ||
                                selectedValidationSummary?.passed === undefined
                                  ? "Unknown"
                                  : selectedValidationSummary.passed
                                    ? "Yes"
                                    : "No"}
                              </p>
                            </div>
                            <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Checks
                              </p>
                              <p className="mt-1 text-sm font-medium text-text">
                                {selectedValidationSummary?.validationsAttempted ?? 0}
                              </p>
                            </div>
                          </div>
                          {loadingArtifact ? (
                            <p className="text-sm text-text-muted">Loading validation preview…</p>
                          ) : artifactPreview?.kind === "diff" ? (
                            <DiffViewer
                              oldValue={artifactPreview.oldValue}
                              newValue={artifactPreview.newValue}
                              oldTitle="Before"
                              newTitle="After"
                            />
                          ) : (
                            <pre className="max-h-[420px] overflow-auto rounded-2xl border border-border bg-surface-elevated/40 p-3 text-[11px] text-text-muted">
                              {artifactPreview?.value ?? "No validation preview available."}
                            </pre>
                          )}
                        </>
                      ) : (
                        <p className="text-sm text-text-muted">
                          Select a validation artifact to inspect its outcome.
                        </p>
                      )}
                    </CardContent>
                  </Card>
                </div>
              ) : null}

              {detailTab === "memory" ? (
                <div className="grid gap-4 xl:grid-cols-2">
                  <Card>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2 text-base">
                        <Brain className="h-4 w-4" />
                        Memory Hits
                      </CardTitle>
                    </CardHeader>
                    <CardContent className="space-y-4">
                      <div className="grid gap-3 md:grid-cols-2">
                        <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                            Hit count
                          </p>
                          <p className="mt-1 text-lg font-semibold text-text">
                            {memoryHits.length}
                          </p>
                        </div>
                        <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                            Scored hits
                          </p>
                          <p className="mt-1 text-lg font-semibold text-text">
                            {memoryHits.filter((hit) => typeof hit.score === "number").length}
                          </p>
                        </div>
                      </div>
                      {memoryHits.length === 0 ? (
                        <p className="text-sm text-text-muted">No memory hits returned.</p>
                      ) : (
                        memoryHits.map((hit, index) => (
                          <div
                            key={String(hit.candidate_id ?? hit.memory_id ?? index)}
                            className="rounded-3xl border border-border bg-surface-elevated/40 p-4"
                          >
                            <div className="flex items-center justify-between gap-3">
                              <div>
                                <p className="text-sm font-medium text-text">
                                  {memoryKindLabel(hit.kind ?? hit.source ?? "memory_hit")}
                                </p>
                                <p className="mt-1 text-[11px] text-text-muted">
                                  {pickText(hit.candidate_id) ||
                                    pickText(hit.memory_id) ||
                                    `hit-${index + 1}`}
                                </p>
                              </div>
                              {typeof hit.score === "number" ? (
                                <span className="rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-emerald-200">
                                  score {hit.score.toFixed(2)}
                                </span>
                              ) : null}
                            </div>
                            <div className="mt-3 flex flex-wrap gap-2">
                              {pickText(hit.kind) ? (
                                <span className="rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                  {pickText(hit.kind)}
                                </span>
                              ) : null}
                              {pickText(hit.source) ? (
                                <span className="rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                  {pickText(hit.source)}
                                </span>
                              ) : null}
                              {pickText(hit.memory_id) ? (
                                <span className="rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                  memory {pickText(hit.memory_id)}
                                </span>
                              ) : null}
                            </div>
                            <div className="mt-3 rounded-2xl border border-border bg-surface p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Summary
                              </p>
                              <pre className="mt-2 overflow-x-auto whitespace-pre-wrap break-words text-[11px] text-text-muted">
                                {renderValue(hit.summary ?? hit.content ?? hit.payload ?? hit)}
                              </pre>
                            </div>
                          </div>
                        ))
                      )}
                    </CardContent>
                  </Card>

                  <Card>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2 text-base">
                        <Brain className="h-4 w-4" />
                        Memory Candidates
                      </CardTitle>
                    </CardHeader>
                    <CardContent className="space-y-4">
                      <div className="grid gap-3 md:grid-cols-2">
                        <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                            Candidate count
                          </p>
                          <p className="mt-1 text-lg font-semibold text-text">
                            {memoryCandidates.length}
                          </p>
                        </div>
                        <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                            Linked artifacts
                          </p>
                          <p className="mt-1 text-lg font-semibold text-text">
                            {memoryCandidates.filter((candidate) => candidate.artifact).length}
                          </p>
                        </div>
                      </div>
                      {memoryCandidates.length === 0 ? (
                        <p className="text-sm text-text-muted">No memory candidates recorded.</p>
                      ) : (
                        memoryCandidates.map((candidate) => (
                          <div
                            key={candidate.candidate_id}
                            className="rounded-3xl border border-border bg-surface-elevated/40 p-4"
                          >
                            <div className="flex items-center justify-between gap-3">
                              <div>
                                <p className="text-sm font-medium text-text">{candidate.kind}</p>
                                <p className="mt-1 text-[11px] text-text-muted">
                                  {candidate.candidate_id}
                                </p>
                              </div>
                              <span className="text-xs text-text-muted">
                                {formatTimestamp(candidate.created_at_ms)}
                              </span>
                            </div>
                            <div className="mt-3 flex flex-wrap gap-2">
                              <span className="rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-emerald-200">
                                {candidate.kind}
                              </span>
                              {candidate.artifact?.artifact_type ? (
                                <span className="rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                  {candidate.artifact.artifact_type}
                                </span>
                              ) : null}
                            </div>
                            {candidate.artifact ? (
                              <button
                                type="button"
                                onClick={() => {
                                  setSelectedArtifactPath(candidate.artifact?.path ?? null);
                                  setDetailTab("artifacts");
                                }}
                                className="mt-3 w-full rounded-2xl border border-border bg-surface p-3 text-left transition-colors hover:bg-surface-elevated"
                              >
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  Linked artifact
                                </p>
                                <p className="mt-2 break-all font-mono text-[11px] text-text-muted">
                                  {candidate.artifact.path}
                                </p>
                              </button>
                            ) : null}
                            <div className="mt-3 rounded-2xl border border-border bg-surface p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Summary
                              </p>
                              <pre className="mt-2 overflow-x-auto whitespace-pre-wrap break-words text-[11px] text-text-muted">
                                {renderValue(candidate.summary ?? candidate.payload)}
                              </pre>
                            </div>
                          </div>
                        ))
                      )}
                    </CardContent>
                  </Card>
                </div>
              ) : null}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
