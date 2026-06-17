import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  Brain,
  Database,
  ExternalLink,
  GitBranch,
  History,
  PanelsTopLeft,
  RefreshCw,
  Search,
  Shield,
  SquareCheckBig,
  SquareX,
  Workflow,
} from "lucide-react";
import {
  approveCoderRun,
  cancelCoderRun,
  executeContextRunRollback,
  getCoderMemoryHits,
  getCoderRun,
  getContextRunRollbackHistory,
  getContextRunRollbackPreview,
  listCoderArtifacts,
  listCoderMemoryCandidates,
  listCoderRuns,
  readFileText,
  type CoderArtifactRecord,
  type CoderMemoryCandidateRecord,
  type CoderRunRecord,
  type CoderTaskTelemetry,
  type CoderRunTelemetrySummary,
} from "@/lib/tauri";
import { DiffViewer } from "@/components/plan/DiffViewer";
import { summarizeWorkflowEvent } from "@/components/coder/shared/coderRunUtils";
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

type ArtifactDiffFile = {
  key: string;
  label: string;
  oldValue: string;
  newValue: string;
  lineCountBefore: number;
  lineCountAfter: number;
  extensionLabel: string;
};

type ArtifactPreview =
  | {
      kind: "raw";
      value: string;
    }
  | {
      kind: "diff";
      rawValue: string;
      files: ArtifactDiffFile[];
    };

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

function lineCount(value: string): number {
  if (!value) return 0;
  return value.split("\n").length;
}

function pickText(value: unknown): string {
  if (typeof value === "string" && value.trim().length > 0) return value.trim();
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

function pickNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function fileExtensionLabel(path: string): string {
  const segment = path.split("/").pop() ?? path;
  const extension = segment.includes(".") ? (segment.split(".").pop() ?? "") : "";
  return extension ? extension.toUpperCase() : "TEXT";
}

function basename(path: string): string {
  return path.split("/").pop() ?? path;
}

function normalizedPath(path: string): string {
  return path.replace(/\\/g, "/").trim().toLowerCase();
}

function pathSuffixes(path: string): string[] {
  const parts = normalizedPath(path).split("/").filter(Boolean);
  const suffixes: string[] = [];
  for (let index = 0; index < parts.length; index += 1) {
    suffixes.push(parts.slice(index).join("/"));
  }
  return suffixes;
}

function nestedContent(value: unknown): string | null {
  if (typeof value === "string") return value;
  const record = asRecord(value);
  if (!record) return null;
  for (const candidate of [
    record.content,
    record.text,
    record.body,
    record.value,
    record.snapshot,
  ]) {
    if (typeof candidate === "string") return candidate;
  }
  return null;
}

function diffSide(record: Record<string, unknown>, side: "old" | "new"): string | null {
  const directKeys =
    side === "old"
      ? ["old", "before", "old_content", "before_text", "previous_content", "original"]
      : ["new", "after", "new_content", "after_text", "updated_content", "proposed_content"];
  for (const key of directKeys) {
    const value = record[key];
    if (typeof value === "string") return value;
  }
  const nestedKeys =
    side === "old"
      ? ["before_snapshot", "before_state", "old_snapshot"]
      : ["after_snapshot", "after_state", "new_snapshot"];
  for (const key of nestedKeys) {
    const value = nestedContent(record[key]);
    if (value !== null) return value;
  }
  return null;
}

function diffPath(record: Record<string, unknown>, fallbackPath?: string | null): string {
  for (const candidate of [
    record.path,
    record.file,
    record.file_path,
    record.target_file,
    record.relative_path,
    record.filename,
  ]) {
    const text = pickText(candidate);
    if (text) return text;
  }
  return fallbackPath || "Artifact preview";
}

function buildDiffFile(
  record: Record<string, unknown>,
  index: number,
  fallbackPath?: string | null
): ArtifactDiffFile | null {
  const oldValue = diffSide(record, "old");
  const newValue = diffSide(record, "new");
  if (oldValue === null || newValue === null) return null;
  const label = diffPath(record, fallbackPath);
  return {
    key: `${label}-${index}`,
    label,
    oldValue,
    newValue,
    lineCountBefore: lineCount(oldValue),
    lineCountAfter: lineCount(newValue),
    extensionLabel: fileExtensionLabel(label),
  };
}

function extractArtifactDiffFiles(
  parsed: Record<string, unknown>,
  fallbackPath?: string | null
): ArtifactDiffFile[] {
  const files: ArtifactDiffFile[] = [];
  for (const key of ["files", "changes", "edits", "diffs", "file_diffs"]) {
    const items = parsed[key];
    if (!Array.isArray(items)) continue;
    items.forEach((item, index) => {
      const record = asRecord(item);
      if (!record) return;
      const file = buildDiffFile(record, index, fallbackPath);
      if (file) files.push(file);
    });
  }
  const topLevelFile = buildDiffFile(parsed, files.length, fallbackPath);
  if (topLevelFile) files.push(topLevelFile);
  return files.filter(
    (file, index, all) => all.findIndex((candidate) => candidate.key === file.key) === index
  );
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

function rollbackExecutionAllowedStatus(status: string): boolean {
  return ["awaiting_approval", "paused", "blocked", "failed", "completed", "cancelled"].includes(
    status.trim().toLowerCase()
  );
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

function taskStableId(task: RunTaskRecord, fallback = "task"): string {
  return String(task.id ?? task.workflow_node_id ?? task.task_type ?? fallback);
}

function taskOwner(task: RunTaskRecord): string {
  const payload = asRecord(task.payload);
  return (
    pickText(task.owner) ||
    pickText(task.claimed_by) ||
    pickText(task.assignee) ||
    pickText(payload?.owner) ||
    pickText(payload?.claimed_by) ||
    pickText(payload?.assignee) ||
    ""
  );
}

function taskFailureDetail(task: RunTaskRecord): string {
  const payload = asRecord(task.payload);
  return (
    pickText(task.error_message) ||
    pickText(task.error) ||
    pickText(task.last_error) ||
    pickText(payload?.error_message) ||
    pickText(payload?.error) ||
    pickText(payload?.last_error) ||
    pickText(payload?.failure_reason) ||
    pickText(payload?.reason) ||
    pickText(payload?.stderr) ||
    ""
  );
}

function taskChangedFiles(task: RunTaskRecord): string[] {
  return uniqueStrings(extractChangedFiles([task, asRecord(task.payload)]), 8);
}

function taskKindLabel(task: RunTaskRecord): string {
  const payload = asRecord(task.payload);
  return pickText(payload?.task_kind) || pickText(task.task_type) || "";
}

function taskWriteScope(task: RunTaskRecord): string {
  const payload = asRecord(task.payload);
  return (
    pickText(payload?.write_scope) ||
    pickText(payload?.writeScope) ||
    pickText(payload?.target_scope) ||
    ""
  );
}

function taskVerificationPlan(task: RunTaskRecord): string {
  const payload = asRecord(task.payload);
  return (
    pickText(payload?.verification_command) ||
    pickText(payload?.verificationCommand) ||
    pickText(payload?.build_command) ||
    pickText(payload?.test_command) ||
    pickText(payload?.lint_command) ||
    ""
  );
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

function phaseDetailTab(phase?: string | null): "overview" | "artifacts" | "validation" {
  const normalized = (phase ?? "").toLowerCase();
  if (
    normalized.includes("validation") ||
    normalized.includes("approve") ||
    normalized.includes("approval")
  ) {
    return "validation";
  }
  if (
    normalized.includes("artifact") ||
    normalized.includes("fix") ||
    normalized.includes("report")
  ) {
    return "artifacts";
  }
  return "overview";
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

function relatedArtifactsForBlackboardRow(
  artifacts: CoderArtifactRecord[],
  row: BlackboardRow
): CoderArtifactRecord[] {
  return relatedArtifactsForEvent(
    artifacts,
    blackboardRowStepId(row) ?? "",
    blackboardRowSourceEventId(row) ?? ""
  );
}

function includesContextValue(value: unknown, candidates: string[]): boolean {
  const text = pickText(value).toLowerCase();
  if (!text) return false;
  return candidates.some((candidate) => candidate && text.includes(candidate.toLowerCase()));
}

function uniqueStrings(values: string[], limit = 12): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const raw of values) {
    const value = raw.trim();
    if (!value) continue;
    const key = value.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(value);
    if (result.length >= limit) break;
  }
  return result;
}

function collectStrings(value: unknown, depth = 0): string[] {
  if (depth > 4 || value == null) return [];
  if (typeof value === "string") return [value];
  if (typeof value === "number" || typeof value === "boolean") return [String(value)];
  if (Array.isArray(value)) {
    return value.flatMap((item) => collectStrings(item, depth + 1));
  }
  const record = asRecord(value);
  if (!record) return [];
  return Object.values(record).flatMap((item) => collectStrings(item, depth + 1));
}

function collectKeyedStrings(
  value: unknown,
  keyMatcher: (key: string) => boolean,
  depth = 0
): string[] {
  if (depth > 5 || value == null) return [];
  if (Array.isArray(value)) {
    return value.flatMap((item) => collectKeyedStrings(item, keyMatcher, depth + 1));
  }
  const record = asRecord(value);
  if (!record) return [];
  const values: string[] = [];
  for (const [key, child] of Object.entries(record)) {
    if (keyMatcher(key.toLowerCase())) {
      values.push(...collectStrings(child, depth + 1));
    }
    values.push(...collectKeyedStrings(child, keyMatcher, depth + 1));
  }
  return values;
}

function looksLikeFilePath(value: string): boolean {
  const text = value.trim();
  if (!text || text.startsWith("http://") || text.startsWith("https://")) return false;
  if (text.length > 260 || text.includes("\n")) return false;
  return (
    text.includes("/") ||
    text.includes("\\") ||
    /\.[a-z0-9]{1,8}$/i.test(text) ||
    /^[a-z0-9_.-]+\.[a-z0-9]{1,8}$/i.test(text)
  );
}

function looksLikeCommand(value: string): boolean {
  const text = value.trim();
  if (!text || text.length > 240 || text.includes("\n")) return false;
  return /(^|[\s])(cargo|pnpm|npm|yarn|bun|node|python|pytest|ruff|uv|make|cmake|go|rustc|git|bash|sh|zsh|deno|turbo|nx|jest|vitest|playwright|mvn|gradle)([\s]|$)/i.test(
    text
  );
}

function extractChangedFiles(value: unknown): string[] {
  const keyed = collectKeyedStrings(
    value,
    (key) =>
      key.includes("changed_file") ||
      key.includes("files_changed") ||
      key.includes("touched_file") ||
      key.includes("modified_file") ||
      key.includes("updated_file") ||
      key.includes("created_file") ||
      key === "path" ||
      key === "file" ||
      key === "file_path" ||
      key === "target_file" ||
      key === "relative_path"
  );
  return uniqueStrings(keyed.filter(looksLikeFilePath), 16);
}

function extractVerificationCommands(value: unknown): string[] {
  const keyed = collectKeyedStrings(
    value,
    (key) =>
      key === "command" ||
      key === "cmd" ||
      key === "commands" ||
      key.includes("command_run") ||
      key.includes("commands_run") ||
      key.includes("verification_command") ||
      key.includes("validation_command") ||
      key.includes("test_command") ||
      key.includes("lint_command") ||
      key.includes("build_command")
  );
  return uniqueStrings(keyed.filter(looksLikeCommand), 12);
}

function extractPatchSummaries(value: unknown): string[] {
  const keyed = collectKeyedStrings(
    value,
    (key) =>
      key === "summary" ||
      key.includes("patch_summary") ||
      key.includes("diff_summary") ||
      key.includes("change_summary") ||
      key.includes("result_summary")
  );
  return uniqueStrings(
    keyed.filter((entry) => entry.trim().length >= 12 && !looksLikeFilePath(entry)),
    6
  );
}

function extractTaskVerificationCommands(task: RunTaskRecord): string[] {
  return extractVerificationCommands(task).concat(
    extractVerificationCommands(asRecord(task.payload) ?? null)
  );
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
  const [memoryHitFilter, setMemoryHitFilter] = useState<"all" | "scored">("all");
  const [memoryCandidateFilter, setMemoryCandidateFilter] = useState<"all" | "artifact_backed">(
    "all"
  );
  const [detailTab, setDetailTab] = useState<"overview" | "artifacts" | "memory" | "validation">(
    "overview"
  );
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [selectedRun, setSelectedRun] = useState<CoderRunRecord | null>(null);
  const [runState, setRunState] = useState<Record<string, unknown> | null>(null);
  const [codingSummary, setCodingSummary] = useState<CoderRunTelemetrySummary | null>(null);
  const [artifacts, setArtifacts] = useState<CoderArtifactRecord[]>([]);
  const [memoryHits, setMemoryHits] = useState<Record<string, unknown>[]>([]);
  const [memoryCandidates, setMemoryCandidates] = useState<CoderMemoryCandidateRecord[]>([]);
  const [selectedArtifactPath, setSelectedArtifactPath] = useState<string | null>(null);
  const [selectedArtifactContent, setSelectedArtifactContent] = useState<string>("");
  const [compareArtifactPath, setCompareArtifactPath] = useState<string | null>(null);
  const [compareArtifactContent, setCompareArtifactContent] = useState<string>("");
  const [selectedDiffFileKey, setSelectedDiffFileKey] = useState<string | null>(null);
  const [compareDiffFileKey, setCompareDiffFileKey] = useState<string | null>(null);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [artifactPreviewMode, setArtifactPreviewMode] = useState<"diff" | "raw">("diff");
  const [artifactDiffSplitView, setArtifactDiffSplitView] = useState(true);
  const [loadingArtifact, setLoadingArtifact] = useState(false);
  const [loadingCompareArtifact, setLoadingCompareArtifact] = useState(false);
  const [loadingRuns, setLoadingRuns] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [acting, setActing] = useState<"approve" | "cancel" | null>(null);
  const [rollbackPreview, setRollbackPreview] = useState<Record<string, unknown> | null>(null);
  const [loadingRollbackPreview, setLoadingRollbackPreview] = useState(false);
  const [rollbackPreviewError, setRollbackPreviewError] = useState<string | null>(null);
  const [rollbackHistory, setRollbackHistory] = useState<Record<string, unknown> | null>(null);
  const [loadingRollbackHistory, setLoadingRollbackHistory] = useState(false);
  const [rollbackHistoryError, setRollbackHistoryError] = useState<string | null>(null);
  const [rollbackSelectedEventIds, setRollbackSelectedEventIds] = useState<string[]>([]);
  const [rollbackPolicyAcknowledged, setRollbackPolicyAcknowledged] = useState(false);
  const [rollbackExecuting, setRollbackExecuting] = useState(false);
  const [rollbackExecutionResult, setRollbackExecutionResult] = useState<Record<
    string,
    unknown
  > | null>(null);
  const [copiedContextRun, setCopiedContextRun] = useState(false);
  const [copiedDuplicateBadge, setCopiedDuplicateBadge] = useState<string | null>(null);
  const [copiedMemoryValue, setCopiedMemoryValue] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const kanbanRef = useRef<HTMLDivElement | null>(null);
  const blackboardRef = useRef<HTMLDivElement | null>(null);
  const blackboardLineageRef = useRef<HTMLDivElement | null>(null);
  const blackboardQuestionsRef = useRef<HTMLDivElement | null>(null);
  const timelineRef = useRef<HTMLDivElement | null>(null);
  const memorySnapshotRef = useRef<HTMLDivElement | null>(null);
  const validationTasksRef = useRef<HTMLDivElement | null>(null);
  const validationArtifactsRef = useRef<HTMLDivElement | null>(null);
  const validationInspectorRef = useRef<HTMLDivElement | null>(null);
  const memoryHitsRef = useRef<HTMLDivElement | null>(null);
  const memoryCandidatesRef = useRef<HTMLDivElement | null>(null);

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
      setCodingSummary(runPayload.coding_summary ?? null);
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

  const loadRollbackPreview = useCallback(async (contextRunId: string) => {
    setLoadingRollbackPreview(true);
    try {
      const payload = await getContextRunRollbackPreview(contextRunId);
      setRollbackPreview(payload);
      setRollbackPreviewError(null);
    } catch (err) {
      setRollbackPreview(null);
      setRollbackPreviewError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoadingRollbackPreview(false);
    }
  }, []);

  const loadRollbackHistory = useCallback(async (contextRunId: string) => {
    setLoadingRollbackHistory(true);
    try {
      const payload = await getContextRunRollbackHistory(contextRunId);
      setRollbackHistory(payload);
      setRollbackHistoryError(null);
    } catch (err) {
      setRollbackHistory(null);
      setRollbackHistoryError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoadingRollbackHistory(false);
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
      setCodingSummary(null);
      setArtifacts([]);
      setMemoryHits([]);
      setMemoryCandidates([]);
      setSelectedTaskId(null);
      setSelectedArtifactPath(null);
      setSelectedArtifactContent("");
      setCompareArtifactPath(null);
      setCompareArtifactContent("");
      setRollbackPreview(null);
      setRollbackPreviewError(null);
      setRollbackHistory(null);
      setRollbackHistoryError(null);
      setRollbackSelectedEventIds([]);
      setRollbackPolicyAcknowledged(false);
      setRollbackExecutionResult(null);
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

  useEffect(() => {
    setSelectedDiffFileKey(null);
    setCompareDiffFileKey(null);
    setArtifactPreviewMode("diff");
    setArtifactDiffSplitView(true);
    setCompareArtifactPath(null);
    setCompareArtifactContent("");
  }, [selectedArtifactPath]);

  useEffect(() => {
    if (!compareArtifactPath) {
      setCompareArtifactContent("");
      return;
    }
    let cancelled = false;
    const loadArtifact = async () => {
      setLoadingCompareArtifact(true);
      try {
        const content = await readFileText(compareArtifactPath, 512 * 1024, 120_000);
        if (!cancelled) {
          setCompareArtifactContent(content);
        }
      } catch (err) {
        if (!cancelled) {
          setCompareArtifactContent(err instanceof Error ? err.message : String(err));
        }
      } finally {
        if (!cancelled) {
          setLoadingCompareArtifact(false);
        }
      }
    };
    void loadArtifact();
    return () => {
      cancelled = true;
    };
  }, [compareArtifactPath]);

  const selectedTaskRows = useMemo(() => {
    const tasks = runState?.tasks;
    return (Array.isArray(tasks) ? tasks : []) as RunTaskRecord[];
  }, [runState]);

  useEffect(() => {
    if (selectedTaskRows.length === 0) {
      setSelectedTaskId(null);
      return;
    }
    setSelectedTaskId((current) => {
      if (current && selectedTaskRows.some((task) => taskStableId(task) === current)) {
        return current;
      }
      return taskStableId(selectedTaskRows[0]);
    });
  }, [selectedTaskRows]);

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

  const timelineContextArtifact = useMemo(() => {
    for (const event of filteredEventTimeline) {
      const relatedArtifacts = relatedArtifactsForEvent(
        artifacts,
        pickText(event.step_id),
        pickText(event.event_id)
      );
      if (relatedArtifacts.length > 0) return relatedArtifacts[0] ?? null;
    }
    return null;
  }, [artifacts, filteredEventTimeline]);

  const readinessHint = useMemo(() => {
    if (!error) return null;
    const normalized = error.toLowerCase();
    if (normalized.includes("readiness") || normalized.includes("mcp")) {
      return "Coder readiness is blocked. Open MCP settings to inspect server connectivity.";
    }
    return null;
  }, [error]);

  const linkedContextRunId = useMemo(
    () => pickText(selectedRun?.linked_context_run_id),
    [selectedRun?.linked_context_run_id]
  );

  useEffect(() => {
    if (!linkedContextRunId) {
      setRollbackPreview(null);
      setRollbackPreviewError(null);
      setRollbackHistory(null);
      setRollbackHistoryError(null);
      setRollbackSelectedEventIds([]);
      setRollbackPolicyAcknowledged(false);
      setRollbackExecutionResult(null);
      return;
    }
    setRollbackSelectedEventIds([]);
    setRollbackPolicyAcknowledged(false);
    setRollbackExecutionResult(null);
    void loadRollbackPreview(linkedContextRunId);
    void loadRollbackHistory(linkedContextRunId);
  }, [linkedContextRunId, loadRollbackHistory, loadRollbackPreview]);

  const selectedRollbackHistorySummary = useMemo(() => {
    const serverSummary = asRecord(runState?.rollback_history_summary);
    if (serverSummary) {
      const byOutcome = asRecord(serverSummary.by_outcome) ?? {};
      return {
        entryCount: pickNumber(serverSummary.entry_count) ?? 0,
        appliedCount: pickNumber(byOutcome.applied) ?? 0,
        blockedCount: pickNumber(byOutcome.blocked) ?? 0,
      };
    }
    return selectedRunEvents.reduce(
      (summary: { entryCount: number; appliedCount: number; blockedCount: number }, event) => {
        const type = runEventType(event);
        if (type === "rollback_execution_applied") {
          summary.entryCount += 1;
          summary.appliedCount += 1;
        } else if (type === "rollback_execution_blocked") {
          summary.entryCount += 1;
          summary.blockedCount += 1;
        }
        return summary;
      },
      { entryCount: 0, appliedCount: 0, blockedCount: 0 }
    );
  }, [runState, selectedRunEvents]);

  const selectedLastRollbackOutcome = useMemo(() => {
    const serverOutcome = asRecord(runState?.last_rollback_outcome);
    if (serverOutcome) {
      return {
        outcome: pickText(serverOutcome.outcome),
        reason: pickText(serverOutcome.reason),
        tsMs: pickNumber(serverOutcome.ts_ms),
      };
    }
    const rollbackEvents = selectedRunEvents.filter((event) => {
      const type = runEventType(event);
      return type === "rollback_execution_applied" || type === "rollback_execution_blocked";
    });
    if (rollbackEvents.length === 0) return null;
    const latest =
      [...rollbackEvents].sort(
        (left, right) => (runEventTimestamp(right) ?? 0) - (runEventTimestamp(left) ?? 0)
      )[0] ?? null;
    if (!latest) return null;
    const payload = asRecord(latest.payload);
    return {
      outcome: runEventType(latest) === "rollback_execution_applied" ? "applied" : "blocked",
      reason: pickText(payload?.reason),
      tsMs: runEventTimestamp(latest),
    };
  }, [runState, selectedRunEvents]);

  const selectedRollbackPolicy = useMemo(() => {
    const serverPolicy = asRecord(runState?.rollback_policy);
    if (serverPolicy) {
      return {
        eligible: serverPolicy.eligible === true,
        runStatus: pickText(serverPolicy.run_status) || pickText(runState?.status) || "unknown",
        requiredConfirm: pickText(serverPolicy.required_confirm) || "rollback",
        requiredPolicyAck: pickText(serverPolicy.required_policy_ack) || "allow_rollback_execution",
      };
    }
    const runStatus = pickText(runState?.status) || "unknown";
    return {
      eligible: rollbackExecutionAllowedStatus(runStatus),
      runStatus,
      requiredConfirm: "rollback",
      requiredPolicyAck: "allow_rollback_execution",
    };
  }, [runState]);

  const rollbackPreviewSteps = useMemo(() => {
    const rawSteps = asRecord(rollbackPreview)?.steps;
    return (Array.isArray(rawSteps) ? rawSteps : [])
      .map((value) => {
        const record = asRecord(value);
        return {
          eventId: pickText(record?.event_id),
          seq: pickNumber(record?.seq),
          tool: pickText(record?.tool) || "unknown",
          executable: record?.executable === true,
          operationCount: pickNumber(record?.operation_count) ?? 0,
        };
      })
      .filter((step) => step.eventId.length > 0);
  }, [rollbackPreview]);

  const rollbackPreviewSummary = useMemo(() => {
    const record = asRecord(rollbackPreview);
    return {
      stepCount: pickNumber(record?.step_count) ?? rollbackPreviewSteps.length,
      executable: record?.executable === true,
      executableStepCount: pickNumber(record?.executable_step_count) ?? 0,
      advisoryStepCount: pickNumber(record?.advisory_step_count) ?? 0,
    };
  }, [rollbackPreview, rollbackPreviewSteps.length]);

  const rollbackHistoryEntries = useMemo(() => {
    const rawEntries = asRecord(rollbackHistory)?.entries;
    return (Array.isArray(rawEntries) ? rawEntries : [])
      .map((value) => {
        const record = asRecord(value);
        const rawSelectedIds = record?.selected_event_ids;
        const rawMissingIds = record?.missing_event_ids;
        const rawAppliedByAction = asRecord(record?.applied_by_action);
        return {
          eventId: pickText(record?.event_id),
          outcome: pickText(record?.outcome) || "unknown",
          reason: pickText(record?.reason),
          tsMs: pickNumber(record?.ts_ms),
          seq: pickNumber(record?.seq),
          selectedEventIds: (Array.isArray(rawSelectedIds) ? rawSelectedIds : [])
            .map(pickText)
            .filter((value) => value.length > 0),
          missingEventIds: (Array.isArray(rawMissingIds) ? rawMissingIds : [])
            .map(pickText)
            .filter((value) => value.length > 0),
          appliedStepCount: pickNumber(record?.applied_step_count) ?? 0,
          appliedOperationCount: pickNumber(record?.applied_operation_count) ?? 0,
          appliedByAction: rawAppliedByAction
            ? Object.entries(rawAppliedByAction)
                .map(([key, value]) => [key, pickNumber(value) ?? 0] as const)
                .filter(([, value]) => value > 0)
            : [],
        };
      })
      .filter((entry) => entry.eventId.length > 0)
      .sort((left, right) => (right.tsMs ?? 0) - (left.tsMs ?? 0));
  }, [rollbackHistory]);

  const rollbackExecutionMessage = useMemo(() => {
    const record = asRecord(rollbackExecutionResult);
    if (!record) return "";
    if (record.applied === true) {
      const count = pickNumber(record.applied_operation_count) ?? 0;
      return `Applied ${count} rollback operation${count === 1 ? "" : "s"}.`;
    }
    return pickText(record.reason) || "Rollback request was blocked.";
  }, [rollbackExecutionResult]);

  const filteredRuns = useMemo(() => {
    const query = runQuery.trim().toLowerCase();
    return runs.filter((run) => {
      const normalizedStatus = (run.status ?? "unknown").toLowerCase();
      if (statusFilter === "active") {
        if (normalizedStatus !== "running" && normalizedStatus !== "planning") return false;
      } else if (statusFilter !== "all" && normalizedStatus !== statusFilter) {
        return false;
      }
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
    return ["all", "active", ...new Set(runs.map((run) => run.status ?? "unknown"))];
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

  const artifactPreview = useMemo<ArtifactPreview | null>(() => {
    if (!selectedArtifactContent) return null;
    try {
      const parsed = JSON.parse(selectedArtifactContent) as Record<string, unknown>;
      const files = extractArtifactDiffFiles(parsed, selectedArtifactRecord?.path);
      if (files.length > 0) {
        return {
          kind: "diff",
          rawValue: selectedArtifactContent,
          files,
        };
      }
      return { kind: "raw", value: selectedArtifactContent };
    } catch {
      return { kind: "raw", value: selectedArtifactContent };
    }
  }, [selectedArtifactContent, selectedArtifactRecord?.path]);

  useEffect(() => {
    if (artifactPreview?.kind !== "diff") return;
    setSelectedDiffFileKey((current) =>
      artifactPreview.files.some((file) => file.key === current)
        ? current
        : (artifactPreview.files[0]?.key ?? null)
    );
  }, [artifactPreview]);

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

  const selectedDiffFile = useMemo(() => {
    if (artifactPreview?.kind !== "diff") return null;
    return artifactPreview.files.find((file) => file.key === selectedDiffFileKey) ?? null;
  }, [artifactPreview, selectedDiffFileKey]);

  const selectedArtifactContext = useMemo(() => {
    if (!selectedArtifactRecord) return null;
    return {
      path: selectedArtifactRecord.path,
      stepId: selectedArtifactRecord.step_id ?? "",
      sourceEventId: selectedArtifactRecord.source_event_id ?? "",
    };
  }, [selectedArtifactRecord]);

  const selectedArtifactSiblings = useMemo(() => {
    if (!selectedArtifactRecord) {
      return {
        olderInCategory: null as CoderArtifactRecord | null,
        newerInCategory: null as CoderArtifactRecord | null,
        sameStepLatest: null as CoderArtifactRecord | null,
        sameEventLatest: null as CoderArtifactRecord | null,
        sameStepCount: 0,
        sameEventCount: 0,
      };
    }
    const category = artifactCategory(selectedArtifactRecord);
    const inCategory = [...artifacts]
      .filter(
        (artifact) =>
          artifact.path !== selectedArtifactRecord.path && artifactCategory(artifact) === category
      )
      .sort((left, right) => right.ts_ms - left.ts_ms);
    const olderInCategory =
      [...inCategory]
        .filter((artifact) => artifact.ts_ms < selectedArtifactRecord.ts_ms)
        .sort((left, right) => right.ts_ms - left.ts_ms)[0] ?? null;
    const newerInCategory =
      [...inCategory]
        .filter((artifact) => artifact.ts_ms > selectedArtifactRecord.ts_ms)
        .sort((left, right) => left.ts_ms - right.ts_ms)[0] ?? null;
    const sameStepArtifacts = selectedArtifactRecord.step_id
      ? artifacts
          .filter(
            (artifact) =>
              artifact.path !== selectedArtifactRecord.path &&
              artifact.step_id === selectedArtifactRecord.step_id
          )
          .sort((left, right) => right.ts_ms - left.ts_ms)
      : [];
    const sameEventArtifacts = selectedArtifactRecord.source_event_id
      ? artifacts
          .filter(
            (artifact) =>
              artifact.path !== selectedArtifactRecord.path &&
              artifact.source_event_id === selectedArtifactRecord.source_event_id
          )
          .sort((left, right) => right.ts_ms - left.ts_ms)
      : [];
    return {
      olderInCategory,
      newerInCategory,
      sameStepLatest: sameStepArtifacts[0] ?? null,
      sameEventLatest: sameEventArtifacts[0] ?? null,
      sameStepCount: sameStepArtifacts.length,
      sameEventCount: sameEventArtifacts.length,
    };
  }, [artifacts, selectedArtifactRecord]);

  const compareArtifactRecord = useMemo(() => {
    if (!compareArtifactPath) return null;
    return artifacts.find((artifact) => artifact.path === compareArtifactPath) ?? null;
  }, [artifacts, compareArtifactPath]);

  const compareArtifactPreview = useMemo<ArtifactPreview | null>(() => {
    if (!compareArtifactContent) return null;
    try {
      const parsed = JSON.parse(compareArtifactContent) as Record<string, unknown>;
      const files = extractArtifactDiffFiles(parsed, compareArtifactRecord?.path);
      if (files.length > 0) {
        return {
          kind: "diff",
          rawValue: compareArtifactContent,
          files,
        };
      }
      return { kind: "raw", value: compareArtifactContent };
    } catch {
      return { kind: "raw", value: compareArtifactContent };
    }
  }, [compareArtifactContent, compareArtifactRecord?.path]);

  const compareDiffFile = useMemo(() => {
    if (compareArtifactPreview?.kind !== "diff") return null;
    return compareArtifactPreview.files.find((file) => file.key === compareDiffFileKey) ?? null;
  }, [compareArtifactPreview, compareDiffFileKey]);

  const structuredComparePair = useMemo(() => {
    if (artifactPreview?.kind !== "diff" || compareArtifactPreview?.kind !== "diff") return null;
    const compareByExact = new Map(
      compareArtifactPreview.files.map((file) => [normalizedPath(file.label), file])
    );
    const selectedBaseCounts = new Map<string, number>();
    const compareBaseCounts = new Map<string, number>();
    const selectedSuffixCounts = new Map<string, number>();
    const compareSuffixCounts = new Map<string, number>();
    artifactPreview.files.forEach((file) => {
      const base = basename(normalizedPath(file.label));
      selectedBaseCounts.set(base, (selectedBaseCounts.get(base) ?? 0) + 1);
      const suffix = pathSuffixes(file.label).slice(0, 2).pop();
      if (suffix) selectedSuffixCounts.set(suffix, (selectedSuffixCounts.get(suffix) ?? 0) + 1);
    });
    compareArtifactPreview.files.forEach((file) => {
      const base = basename(normalizedPath(file.label));
      compareBaseCounts.set(base, (compareBaseCounts.get(base) ?? 0) + 1);
      const suffix = pathSuffixes(file.label).slice(0, 2).pop();
      if (suffix) compareSuffixCounts.set(suffix, (compareSuffixCounts.get(suffix) ?? 0) + 1);
    });
    const compareByBase = new Map<string, ArtifactDiffFile>();
    const compareBySuffix = new Map<string, ArtifactDiffFile>();
    compareArtifactPreview.files.forEach((file) => {
      const base = basename(normalizedPath(file.label));
      if ((compareBaseCounts.get(base) ?? 0) === 1) {
        compareByBase.set(base, file);
      }
      const suffix = pathSuffixes(file.label).slice(0, 2).pop();
      if (suffix && (compareSuffixCounts.get(suffix) ?? 0) === 1) {
        compareBySuffix.set(suffix, file);
      }
    });
    const pairs = artifactPreview.files
      .map((file) => {
        const exact = compareByExact.get(normalizedPath(file.label));
        if (exact) {
          return {
            label: file.label,
            selectedFile: file,
            compareFile: exact,
            matchKind: "exact" as const,
          };
        }
        const fileBase = basename(normalizedPath(file.label));
        const byBase =
          (selectedBaseCounts.get(fileBase) ?? 0) === 1 ? compareByBase.get(fileBase) : undefined;
        if (byBase) {
          return {
            label: file.label,
            selectedFile: file,
            compareFile: byBase,
            matchKind: "basename" as const,
          };
        }
        const suffix2 = pathSuffixes(file.label).slice(0, 2).pop();
        const bySuffix =
          suffix2 && (selectedSuffixCounts.get(suffix2) ?? 0) === 1
            ? compareBySuffix.get(suffix2)
            : undefined;
        if (bySuffix) {
          return {
            label: file.label,
            selectedFile: file,
            compareFile: bySuffix,
            matchKind: "suffix" as const,
          };
        }
        return null;
      })
      .filter(
        (
          pair
        ): pair is {
          label: string;
          selectedFile: ArtifactDiffFile;
          compareFile: ArtifactDiffFile;
          matchKind: "exact" | "basename" | "suffix";
        } => !!pair
      );
    if (pairs.length === 0) return null;
    const pairByLabel = new Map(pairs.map((pair) => [pair.label, pair]));
    const activeLabel =
      (selectedDiffFile && pairByLabel.has(selectedDiffFile.label) && selectedDiffFile.label) ||
      (compareDiffFile && pairs.some((pair) => pair.compareFile.key === compareDiffFile.key)
        ? pairs.find((pair) => pair.compareFile.key === compareDiffFile.key)?.label
        : null) ||
      pairs[0]?.label ||
      null;
    if (!activeLabel) return null;
    const activePair = pairByLabel.get(activeLabel);
    if (!activePair) return null;
    return {
      sharedLabels: pairs.map((pair) => pair.label),
      selectedFile: activePair.selectedFile,
      compareFile: activePair.compareFile,
      activeLabel,
      matchKind: activePair.matchKind,
    };
  }, [artifactPreview, compareArtifactPreview, compareDiffFile, selectedDiffFile]);

  useEffect(() => {
    if (compareArtifactPreview?.kind !== "diff") return;
    if (structuredComparePair?.activeLabel) {
      const target =
        compareArtifactPreview.files.find(
          (file) => file.label === structuredComparePair.activeLabel
        )?.key ?? null;
      setCompareDiffFileKey(target);
      return;
    }
    setCompareDiffFileKey((current) =>
      compareArtifactPreview.files.some((file) => file.key === current)
        ? current
        : (compareArtifactPreview.files[0]?.key ?? null)
    );
  }, [compareArtifactPreview, structuredComparePair]);

  const relatedBlackboardRows = useMemo(() => {
    if (!selectedArtifactContext) return [];
    const rows = [...decisions, ...openQuestions];
    return rows
      .filter((row) => {
        const stepId = blackboardRowStepId(row) ?? "";
        const sourceEventId = blackboardRowSourceEventId(row) ?? "";
        return (
          (selectedArtifactContext.stepId && stepId === selectedArtifactContext.stepId) ||
          (selectedArtifactContext.sourceEventId &&
            sourceEventId === selectedArtifactContext.sourceEventId)
        );
      })
      .slice(0, 5);
  }, [decisions, openQuestions, selectedArtifactContext]);

  const relatedTimelineEvents = useMemo(() => {
    if (!selectedArtifactContext) return [];
    return eventTimeline
      .filter((event) => {
        const stepId = pickText(event.step_id);
        const sourceEventId = pickText(event.event_id);
        return (
          (selectedArtifactContext.stepId && stepId === selectedArtifactContext.stepId) ||
          (selectedArtifactContext.sourceEventId &&
            sourceEventId === selectedArtifactContext.sourceEventId)
        );
      })
      .slice(0, 5);
  }, [eventTimeline, selectedArtifactContext]);

  const relatedMemoryCandidates = useMemo(() => {
    if (!selectedArtifactContext) return [];
    return memoryCandidates
      .filter((candidate) => {
        const artifact = candidate.artifact;
        if (!artifact) return false;
        return (
          artifact.path === selectedArtifactContext.path ||
          (!!selectedArtifactContext.stepId &&
            artifact.step_id === selectedArtifactContext.stepId) ||
          (!!selectedArtifactContext.sourceEventId &&
            artifact.source_event_id === selectedArtifactContext.sourceEventId)
        );
      })
      .slice(0, 5);
  }, [memoryCandidates, selectedArtifactContext]);

  const relatedMemoryHits = useMemo(() => {
    if (!selectedArtifactContext) return [];
    const needles = [
      selectedArtifactContext.path,
      selectedArtifactContext.stepId,
      selectedArtifactContext.sourceEventId,
    ].filter(Boolean);
    return memoryHits
      .filter((hit) => {
        if (needles.length === 0) return false;
        return (
          includesContextValue(hit.path, needles) ||
          includesContextValue(hit.source_event_id, needles) ||
          includesContextValue(hit.step_id, needles) ||
          includesContextValue(hit.summary, needles) ||
          includesContextValue(hit.content, needles) ||
          includesContextValue(hit.memory_id, needles) ||
          includesContextValue(hit.candidate_id, needles) ||
          includesContextValue(hit.payload, needles)
        );
      })
      .slice(0, 5);
  }, [memoryHits, selectedArtifactContext]);

  const filteredMemoryHits = useMemo(() => {
    return memoryHitFilter === "scored"
      ? memoryHits.filter((hit) => typeof hit.score === "number")
      : memoryHits;
  }, [memoryHitFilter, memoryHits]);

  const filteredMemoryCandidates = useMemo(() => {
    return memoryCandidateFilter === "artifact_backed"
      ? memoryCandidates.filter((candidate) => candidate.artifact)
      : memoryCandidates;
  }, [memoryCandidateFilter, memoryCandidates]);

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

  const selectedCodingSignals = useMemo(() => {
    const changedFiles = uniqueStrings(
      [
        ...(codingSummary?.changed_files ?? []),
        ...(artifactPreview?.kind === "diff"
          ? artifactPreview.files.map((file) => file.label)
          : []),
        ...extractChangedFiles(selectedArtifactJson),
        ...selectedTaskRows.flatMap((task) => extractChangedFiles(task)),
        ...selectedRunEvents.flatMap((event) => extractChangedFiles(asRecord(event.payload))),
      ],
      18
    );
    const verificationCommands = uniqueStrings(
      [
        ...(codingSummary?.verification_commands ?? []),
        ...validationTasks.flatMap((task) => extractTaskVerificationCommands(task)),
        ...extractVerificationCommands(selectedArtifactJson),
        ...selectedRunEvents.flatMap((event) =>
          extractVerificationCommands(asRecord(event.payload))
        ),
      ],
      12
    );
    const patchSummaries = uniqueStrings(
      [
        ...(codingSummary?.patch_summaries ?? []),
        ...extractPatchSummaries(selectedArtifactJson),
        ...selectedRunEvents.flatMap((event) => extractPatchSummaries(asRecord(event.payload))),
      ],
      6
    );
    return {
      changedFiles,
      verificationCommands,
      patchSummaries,
      validationFailures: uniqueStrings(codingSummary?.validation_failures ?? [], 8),
    };
  }, [
    artifactPreview,
    codingSummary,
    selectedArtifactJson,
    selectedRunEvents,
    selectedTaskRows,
    validationTasks,
  ]);

  const codingTaskSummaryById = useMemo(() => {
    const entries: Array<[string, CoderTaskTelemetry]> =
      codingSummary?.task_summaries.map((summary): [string, CoderTaskTelemetry] => [
        summary.task_id.toLowerCase(),
        summary,
      ]) ?? [];
    return new Map<string, CoderTaskTelemetry>(entries);
  }, [codingSummary]);

  const codingTaskSummaryForTask = useCallback(
    (task: RunTaskRecord) => {
      const candidates = [
        pickText(task.id),
        pickText(task.workflow_node_id),
        pickText(task.task_type),
      ]
        .map((value) => value.toLowerCase())
        .filter(Boolean);
      for (const candidate of candidates) {
        const match = codingTaskSummaryById.get(candidate);
        if (match) return match;
      }
      return null;
    },
    [codingTaskSummaryById]
  );

  const selectedTask = useMemo(() => {
    if (!selectedTaskId) return null;
    return selectedTaskRows.find((task) => taskStableId(task) === selectedTaskId) ?? null;
  }, [selectedTaskId, selectedTaskRows]);

  const selectedTaskSummary = useMemo(
    () => (selectedTask ? codingTaskSummaryForTask(selectedTask) : null),
    [codingTaskSummaryForTask, selectedTask]
  );

  const selectedTaskArtifacts = useMemo(
    () => (selectedTask ? relatedArtifactsForTask(artifacts, selectedTask) : []),
    [artifacts, selectedTask]
  );

  const selectedRunOverview = useMemo(() => {
    return {
      tasks: selectedTaskRows.length,
      decisions: decisions.length,
      artifacts: artifacts.length,
      validationTasks: validationTasks.length,
      changedFiles: selectedCodingSignals.changedFiles.length,
      verificationCommands: selectedCodingSignals.verificationCommands.length,
      duplicateArtifacts:
        artifactGroups.find((group) => group.key === "duplicate")?.artifacts.length ?? 0,
    };
  }, [
    artifactGroups,
    artifacts.length,
    decisions.length,
    selectedCodingSignals.changedFiles.length,
    selectedCodingSignals.verificationCommands.length,
    selectedTaskRows.length,
    validationTasks.length,
  ]);

  const validationArtifacts = useMemo(() => {
    return artifactGroups.find((group) => group.key === "validation")?.artifacts ?? [];
  }, [artifactGroups]);

  const latestValidationArtifact = useMemo(() => {
    return [...validationArtifacts].sort((left, right) => right.ts_ms - left.ts_ms)[0] ?? null;
  }, [validationArtifacts]);

  const latestBlackboardArtifact = useMemo(() => {
    const blackboardArtifacts = Array.isArray(selectedBlackboard?.artifacts)
      ? selectedBlackboard.artifacts
      : [];
    if (blackboardArtifacts.length === 0 || artifacts.length === 0) return null;
    const refs = new Set<string>();
    for (const item of blackboardArtifacts) {
      if (typeof item === "string" && item.trim().length > 0) {
        refs.add(item.trim());
        continue;
      }
      const record = asRecord(item);
      if (!record) continue;
      for (const candidate of [
        pickText(record.path),
        pickText(record.artifact_path),
        pickText(record.id),
        pickText(record.artifact_id),
        pickText(record.artifact_type),
        pickText(record.step_id),
        pickText(record.source_event_id),
      ]) {
        if (candidate) refs.add(candidate);
      }
    }
    if (refs.size === 0) return null;
    return (
      [...artifacts]
        .filter((artifact) =>
          [
            artifact.path,
            artifact.id,
            artifact.artifact_type,
            artifact.step_id ?? "",
            artifact.source_event_id ?? "",
          ].some((value) => value && refs.has(value))
        )
        .sort((left, right) => right.ts_ms - left.ts_ms)[0] ?? null
    );
  }, [artifacts, selectedBlackboard]);

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

  const latestArtifactByCategory = useMemo(() => {
    const latest = new Map<ArtifactCategory, CoderArtifactRecord>();
    for (const group of artifactGroups) {
      const top = [...group.artifacts].sort((left, right) => right.ts_ms - left.ts_ms)[0];
      if (top) latest.set(group.key, top);
    }
    return latest;
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
          await approveCoderRun(selectedRunId, "approved from desktop coder compatibility view");
        } else {
          await cancelCoderRun(selectedRunId, "cancelled from desktop coder compatibility view");
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

  const toggleRollbackStepSelection = useCallback((eventId: string) => {
    setRollbackSelectedEventIds((current) =>
      current.includes(eventId)
        ? current.filter((value) => value !== eventId)
        : current.concat(eventId)
    );
  }, []);

  const selectAllExecutableRollbackSteps = useCallback(() => {
    setRollbackSelectedEventIds(
      rollbackPreviewSteps.filter((step) => step.executable).map((step) => step.eventId)
    );
  }, [rollbackPreviewSteps]);

  const executeRollbackSelection = useCallback(async () => {
    if (!selectedRunId || !linkedContextRunId || rollbackSelectedEventIds.length === 0) return;
    setRollbackExecuting(true);
    try {
      const result = await executeContextRunRollback(linkedContextRunId, {
        confirm: "rollback",
        policy_ack: rollbackPolicyAcknowledged ? "allow_rollback_execution" : undefined,
        event_ids: rollbackSelectedEventIds,
      });
      setRollbackExecutionResult(result);
      await Promise.all([
        loadRuns(),
        loadRunDetail(selectedRunId),
        loadRollbackPreview(linkedContextRunId),
        loadRollbackHistory(linkedContextRunId),
      ]);
      if (result.applied === true) {
        setRollbackSelectedEventIds([]);
        setRollbackPolicyAcknowledged(false);
      }
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setRollbackExecuting(false);
    }
  }, [
    linkedContextRunId,
    loadRollbackHistory,
    loadRollbackPreview,
    loadRunDetail,
    loadRuns,
    rollbackPolicyAcknowledged,
    rollbackSelectedEventIds,
    selectedRunId,
  ]);

  const copyDuplicateBadgeValue = useCallback(async (value: string) => {
    try {
      if (!globalThis.navigator?.clipboard?.writeText) return;
      await globalThis.navigator.clipboard.writeText(value);
      setCopiedDuplicateBadge(value);
      globalThis.setTimeout(() => {
        setCopiedDuplicateBadge((current) => (current === value ? null : current));
      }, 1200);
    } catch {
      // Ignore clipboard failures and keep the row action available.
    }
  }, []);

  const copyMemoryValue = useCallback(async (value: string) => {
    try {
      if (!globalThis.navigator?.clipboard?.writeText) return;
      await globalThis.navigator.clipboard.writeText(value);
      setCopiedMemoryValue(value);
      globalThis.setTimeout(() => {
        setCopiedMemoryValue((current) => (current === value ? null : current));
      }, 1200);
    } catch {
      // Ignore clipboard failures and preserve the surrounding actions.
    }
  }, []);

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

  const focusBlackboardSection = useCallback((section: "lineage" | "questions") => {
    setDetailTab("overview");
    globalThis.setTimeout(() => {
      (section === "lineage"
        ? blackboardLineageRef
        : blackboardQuestionsRef
      ).current?.scrollIntoView({ behavior: "smooth", block: "start" });
    }, 0);
  }, []);

  const focusTabSection = useCallback(
    (
      tab: "validation" | "memory",
      section:
        | "validation_tasks"
        | "validation_artifacts"
        | "validation_inspector"
        | "memory_hits"
        | "memory_candidates"
    ) => {
      const target =
        section === "validation_tasks"
          ? validationTasksRef
          : section === "validation_artifacts"
            ? validationArtifactsRef
            : section === "validation_inspector"
              ? validationInspectorRef
              : section === "memory_hits"
                ? memoryHitsRef
                : memoryCandidatesRef;
      setDetailTab(tab);
      globalThis.setTimeout(() => {
        target.current?.scrollIntoView({ behavior: "smooth", block: "start" });
      }, 0);
    },
    []
  );

  const openArtifactRecord = useCallback((artifact: CoderArtifactRecord) => {
    setSelectedArtifactPath(artifact.path);
    setDetailTab(artifactCategory(artifact) === "validation" ? "validation" : "artifacts");
  }, []);

  const resetRunFilters = useCallback(() => {
    setRunQuery("");
    setStatusFilter("all");
    setWorkflowFilter("all");
    setRunSortMode("updated");
  }, []);

  const resetTimelineFilters = useCallback(() => {
    setEventQuery("");
    setEventTypeFilter("all");
  }, []);

  const resetArtifactFilters = useCallback(() => {
    setArtifactQuery("");
  }, []);

  const resetMemoryFilters = useCallback(() => {
    setMemoryHitFilter("all");
    setMemoryCandidateFilter("all");
  }, []);

  const openArtifactRecordContext = useCallback(
    (artifact: CoderArtifactRecord, target: "task" | "event") => {
      if (target === "task") {
        if (!artifact.step_id) return;
        focusOverviewSection("kanban");
        return;
      }
      if (!artifact.source_event_id) return;
      setEventTypeFilter("all");
      setEventQuery(artifact.source_event_id);
      focusOverviewSection("timeline");
    },
    [focusOverviewSection]
  );

  const openArtifactContext = useCallback(
    (target: "task" | "event") => {
      if (!selectedArtifactRecord) return;
      openArtifactRecordContext(selectedArtifactRecord, target);
    },
    [openArtifactRecordContext, selectedArtifactRecord]
  );

  const focusRunStatus = useCallback((run: CoderRunRecord) => {
    const status = (run.status ?? "unknown").toLowerCase();
    setSelectedRunId(run.coder_run_id);
    if (status === "running" || status === "planning") {
      setStatusFilter("active");
      setRunSortMode("updated");
      return;
    }
    setStatusFilter(status);
    setRunSortMode(status === "awaiting_approval" ? "approval" : "updated");
  }, []);

  const openRunPhase = useCallback((run: CoderRunRecord) => {
    setSelectedRunId(run.coder_run_id);
    setDetailTab(phaseDetailTab(run.phase));
  }, []);

  const openDuplicateArtifactContext = useCallback(() => {
    const duplicateArtifact = latestArtifactByCategory.get("duplicate");
    if (duplicateArtifact) {
      setSelectedArtifactPath(duplicateArtifact.path);
    }
    setDetailTab("artifacts");
  }, [latestArtifactByCategory]);

  const openBlackboardContext = useCallback(
    (target: "task" | "event", stepId?: string | null, sourceEventId?: string | null) => {
      if (target === "task") {
        if (!stepId) return;
        focusOverviewSection("kanban");
        return;
      }
      if (!sourceEventId) return;
      setEventTypeFilter("all");
      setEventQuery(sourceEventId);
      focusOverviewSection("timeline");
    },
    [focusOverviewSection]
  );

  const openRelatedEventContext = useCallback(
    (eventId?: string | null) => {
      if (!eventId) return;
      setEventTypeFilter("all");
      setEventQuery(eventId);
      focusOverviewSection("timeline");
    },
    [focusOverviewSection]
  );

  const renderArtifactPreviewContent = useCallback(
    (emptyLabel: string) => {
      if (loadingArtifact) {
        return <p className="text-sm text-text-muted">Loading artifact preview…</p>;
      }
      if (artifactPreview?.kind === "diff" && selectedDiffFile) {
        return (
          <div className="space-y-3">
            <div className="rounded-2xl border border-border bg-surface-elevated/30 p-3">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                    Code-aware diff
                  </p>
                  <p className="mt-1 break-all font-mono text-[11px] text-text-muted">
                    {selectedDiffFile.label}
                  </p>
                </div>
                <div className="flex flex-wrap gap-2">
                  <span className="rounded-full border border-border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                    {selectedDiffFile.extensionLabel}
                  </span>
                  <button
                    type="button"
                    onClick={() => setArtifactPreviewMode("diff")}
                    className={cn(
                      "rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] transition-colors",
                      artifactPreviewMode === "diff"
                        ? "border-primary/40 bg-primary/10 text-text"
                        : "border-border text-text-muted hover:bg-surface-elevated hover:text-text"
                    )}
                  >
                    Diff
                  </button>
                  <button
                    type="button"
                    onClick={() => setArtifactPreviewMode("raw")}
                    className={cn(
                      "rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] transition-colors",
                      artifactPreviewMode === "raw"
                        ? "border-primary/40 bg-primary/10 text-text"
                        : "border-border text-text-muted hover:bg-surface-elevated hover:text-text"
                    )}
                  >
                    Raw
                  </button>
                  <button
                    type="button"
                    onClick={() => setArtifactDiffSplitView((current) => !current)}
                    className="rounded-full border border-border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                  >
                    {artifactDiffSplitView ? "Split" : "Unified"}
                  </button>
                </div>
              </div>
              <div className="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                <div className="rounded-2xl border border-border bg-surface px-3 py-2">
                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">Files</p>
                  <p className="mt-1 text-sm font-medium text-text">
                    {artifactPreview.files.length}
                  </p>
                </div>
                <div className="rounded-2xl border border-border bg-surface px-3 py-2">
                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">Before</p>
                  <p className="mt-1 text-sm font-medium text-text">
                    {selectedDiffFile.lineCountBefore} lines
                  </p>
                </div>
                <div className="rounded-2xl border border-border bg-surface px-3 py-2">
                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">After</p>
                  <p className="mt-1 text-sm font-medium text-text">
                    {selectedDiffFile.lineCountAfter} lines
                  </p>
                </div>
                <div className="rounded-2xl border border-border bg-surface px-3 py-2">
                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">Delta</p>
                  <p className="mt-1 text-sm font-medium text-text">
                    {selectedDiffFile.lineCountAfter - selectedDiffFile.lineCountBefore >= 0
                      ? "+"
                      : ""}
                    {selectedDiffFile.lineCountAfter - selectedDiffFile.lineCountBefore} lines
                  </p>
                </div>
              </div>
              {artifactPreview.files.length > 1 ? (
                <div className="mt-3 flex flex-wrap gap-2">
                  {artifactPreview.files.map((file) => (
                    <button
                      key={file.key}
                      type="button"
                      onClick={() => {
                        setSelectedDiffFileKey(file.key);
                        setArtifactPreviewMode("diff");
                      }}
                      className={cn(
                        "rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] transition-colors",
                        selectedDiffFile.key === file.key
                          ? "border-primary/40 bg-primary/10 text-text"
                          : "border-border text-text-muted hover:bg-surface-elevated hover:text-text"
                      )}
                    >
                      {file.label}
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
            {artifactPreviewMode === "raw" ? (
              <pre className="max-h-[420px] overflow-auto rounded-2xl border border-border bg-surface-elevated/40 p-3 text-[11px] text-text-muted">
                {artifactPreview.rawValue}
              </pre>
            ) : (
              <DiffViewer
                oldValue={selectedDiffFile.oldValue}
                newValue={selectedDiffFile.newValue}
                oldTitle={`Before (${selectedDiffFile.lineCountBefore} lines)`}
                newTitle={`After (${selectedDiffFile.lineCountAfter} lines)`}
                splitView={artifactDiffSplitView}
              />
            )}
          </div>
        );
      }
      return (
        <pre className="max-h-[420px] overflow-auto rounded-2xl border border-border bg-surface-elevated/40 p-3 text-[11px] text-text-muted">
          {artifactPreview?.kind === "raw" ? artifactPreview.value : emptyLabel}
        </pre>
      );
    },
    [artifactDiffSplitView, artifactPreview, artifactPreviewMode, loadingArtifact, selectedDiffFile]
  );

  const renderCompareArtifactPreview = useCallback(() => {
    if (!compareArtifactPath) return null;
    if (loadingCompareArtifact) {
      return <p className="text-sm text-text-muted">Loading comparison artifact…</p>;
    }
    const structuredCompare =
      structuredComparePair?.selectedFile && structuredComparePair?.compareFile ? (
        <div className="space-y-3">
          <div className="rounded-2xl border border-border bg-surface-elevated/30 p-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                  Structured compare
                </p>
                <p className="mt-1 break-all font-mono text-[11px] text-text-muted">
                  {structuredComparePair.activeLabel}
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                <span className="rounded-full border border-border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                  {structuredComparePair.selectedFile.extensionLabel}
                </span>
                <span
                  className={cn(
                    "rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em]",
                    structuredComparePair.matchKind === "exact"
                      ? "border-emerald-500/20 bg-emerald-500/10 text-emerald-200"
                      : "border-amber-500/20 bg-amber-500/10 text-amber-100"
                  )}
                >
                  {structuredComparePair.matchKind === "exact"
                    ? "Exact match"
                    : structuredComparePair.matchKind === "basename"
                      ? "Basename match"
                      : "Suffix match"}
                </span>
                <button
                  type="button"
                  onClick={() => setArtifactDiffSplitView((current) => !current)}
                  className="rounded-full border border-border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                >
                  {artifactDiffSplitView ? "Split" : "Unified"}
                </button>
              </div>
            </div>
            {structuredComparePair.sharedLabels.length > 1 ? (
              <div className="mt-3 flex flex-wrap gap-2">
                {structuredComparePair.sharedLabels.map((label) => (
                  <button
                    key={label}
                    type="button"
                    onClick={() => {
                      const currentFile =
                        artifactPreview?.kind === "diff"
                          ? artifactPreview.files.find((file) => file.label === label)
                          : null;
                      const siblingFile =
                        compareArtifactPreview?.kind === "diff"
                          ? compareArtifactPreview.files.find((file) => file.label === label)
                          : null;
                      setSelectedDiffFileKey(currentFile?.key ?? null);
                      setCompareDiffFileKey(siblingFile?.key ?? null);
                    }}
                    className={cn(
                      "rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] transition-colors",
                      structuredComparePair.activeLabel === label
                        ? "border-primary/40 bg-primary/10 text-text"
                        : "border-border text-text-muted hover:bg-surface-elevated hover:text-text"
                    )}
                  >
                    {label}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
          <DiffViewer
            oldValue={structuredComparePair.selectedFile.newValue}
            newValue={structuredComparePair.compareFile.newValue}
            oldTitle={`Current ${structuredComparePair.selectedFile.label}`}
            newTitle={`Compared ${structuredComparePair.compareFile.label}`}
            splitView={artifactDiffSplitView}
          />
        </div>
      ) : null;
    return (
      <div className="space-y-3 rounded-3xl border border-primary/20 bg-primary/5 p-4">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <p className="text-sm font-medium text-text">Compare artifacts</p>
            <p className="mt-1 break-all font-mono text-[11px] text-text-muted">
              {selectedArtifactPath}
            </p>
            <p className="mt-1 break-all font-mono text-[11px] text-text-muted">
              {compareArtifactPath}
            </p>
          </div>
          <button
            type="button"
            onClick={() => {
              setCompareArtifactPath(null);
              setCompareArtifactContent("");
            }}
            className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
          >
            Clear compare
          </button>
        </div>
        {structuredCompare ?? (
          <DiffViewer
            oldValue={selectedArtifactContent}
            newValue={compareArtifactContent}
            oldTitle={
              selectedArtifactRecord
                ? `${selectedArtifactRecord.artifact_type} (${formatTimestamp(selectedArtifactRecord.ts_ms)})`
                : "Current artifact"
            }
            newTitle={
              compareArtifactRecord
                ? `${compareArtifactRecord.artifact_type} (${formatTimestamp(compareArtifactRecord.ts_ms)})`
                : "Compared artifact"
            }
            splitView={artifactDiffSplitView}
          />
        )}
      </div>
    );
  }, [
    artifactDiffSplitView,
    artifactPreview,
    compareArtifactPath,
    compareArtifactContent,
    compareArtifactPreview,
    compareDiffFile,
    structuredComparePair,
    compareArtifactRecord,
    loadingCompareArtifact,
    selectedArtifactContent,
    selectedArtifactPath,
    selectedArtifactRecord,
  ]);

  return (
    <div className="h-full w-full overflow-hidden app-background">
      <div className="grid h-full grid-cols-[340px_minmax(0,1fr)] gap-4 p-4">
        <Card className="flex min-h-0 flex-col p-0">
          <CardHeader className="border-b border-border px-5 py-4">
            <div className="flex items-start justify-between gap-3">
              <div>
                <CardTitle className="text-base">Legacy Coder Runs</CardTitle>
                <CardDescription>
                  Existing coder runs, artifacts, memory hits, and operator controls.
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
                        setStatusFilter("active");
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
                      {status === "all"
                        ? "All statuses"
                        : status === "active"
                          ? "Active (running/planning)"
                          : status}
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
                <Button variant="secondary" size="sm" onClick={resetRunFilters}>
                  Reset filters
                </Button>
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
                          <button
                            type="button"
                            onClick={(event) => {
                              event.stopPropagation();
                              focusRunStatus(run);
                            }}
                            className={cn(
                              "rounded-full border px-2 py-1 text-[10px] font-medium uppercase tracking-[0.2em]",
                              statusTone(run.status)
                            )}
                          >
                            {run.status ?? "unknown"}
                          </button>
                        </div>
                        <div className="mt-3 flex flex-wrap gap-2">
                          <button
                            type="button"
                            onClick={(event) => {
                              event.stopPropagation();
                              openRunPhase(run);
                            }}
                            className="rounded-full border border-border bg-surface-elevated/40 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted transition-colors hover:bg-surface hover:text-text"
                          >
                            {run.phase ?? "analysis"}
                          </button>
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
                  <div className="mt-3 grid gap-3 xl:grid-cols-3">
                    <Card className="border-border bg-surface-elevated/30">
                      <CardHeader className="pb-2">
                        <CardTitle className="flex items-center gap-2 text-sm">
                          <Shield className="h-4 w-4" />
                          Rollback Policy
                        </CardTitle>
                        <CardDescription>
                          Execution stays gated until the linked context run is in an eligible state
                          and the operator provides the required acknowledgement.
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="space-y-2 pt-0 text-sm text-text-muted">
                        <div className="flex items-center justify-between gap-3 rounded-xl border border-border bg-surface px-3 py-2">
                          <span>Eligibility</span>
                          <span
                            className={cn(
                              "rounded-full px-2.5 py-1 text-[10px] uppercase tracking-[0.18em]",
                              selectedRollbackPolicy.eligible
                                ? "border border-emerald-500/20 bg-emerald-500/10 text-emerald-200"
                                : "border border-amber-500/20 bg-amber-500/10 text-amber-100"
                            )}
                          >
                            {selectedRollbackPolicy.eligible ? "Eligible" : "Blocked"}
                          </span>
                        </div>
                        <div className="rounded-xl border border-border bg-surface px-3 py-2">
                          <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                            Run status
                          </p>
                          <p className="mt-1 text-text">{selectedRollbackPolicy.runStatus}</p>
                        </div>
                        <div className="rounded-xl border border-border bg-surface px-3 py-2">
                          <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                            Required ack
                          </p>
                          <p className="mt-1 font-mono text-[11px] text-text">
                            {selectedRollbackPolicy.requiredPolicyAck}
                          </p>
                        </div>
                      </CardContent>
                    </Card>

                    <Card className="border-border bg-surface-elevated/30">
                      <CardHeader className="pb-2">
                        <CardTitle className="flex items-center gap-2 text-sm">
                          <History className="h-4 w-4" />
                          Rollback Audit
                        </CardTitle>
                        <CardDescription>
                          Applied and blocked rollback attempts recovered from the linked context
                          run.
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="space-y-2 pt-0 text-sm text-text-muted">
                        <div className="grid gap-2 sm:grid-cols-3">
                          {[
                            ["Entries", selectedRollbackHistorySummary.entryCount],
                            ["Applied", selectedRollbackHistorySummary.appliedCount],
                            ["Blocked", selectedRollbackHistorySummary.blockedCount],
                          ].map(([label, value]) => (
                            <div
                              key={String(label)}
                              className="rounded-xl border border-border bg-surface px-3 py-2"
                            >
                              <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                                {label}
                              </p>
                              <p className="mt-1 text-base font-semibold text-text">
                                {String(value)}
                              </p>
                            </div>
                          ))}
                        </div>
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => {
                            setEventQuery("rollback_execution");
                            setEventTypeFilter("all");
                            focusOverviewSection("timeline");
                          }}
                        >
                          <History className="h-4 w-4" />
                          Show rollback events
                        </Button>
                      </CardContent>
                    </Card>

                    <Card className="border-border bg-surface-elevated/30">
                      <CardHeader className="pb-2">
                        <CardTitle className="flex items-center gap-2 text-sm">
                          <RefreshCw className="h-4 w-4" />
                          Last Rollback Outcome
                        </CardTitle>
                        <CardDescription>
                          Most recent rollback result surfaced from the linked context run.
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="space-y-2 pt-0 text-sm text-text-muted">
                        {selectedLastRollbackOutcome ? (
                          <>
                            <div className="flex items-center justify-between gap-3 rounded-xl border border-border bg-surface px-3 py-2">
                              <span>Outcome</span>
                              <span
                                className={cn(
                                  "rounded-full px-2.5 py-1 text-[10px] uppercase tracking-[0.18em]",
                                  selectedLastRollbackOutcome.outcome === "applied"
                                    ? "border border-emerald-500/20 bg-emerald-500/10 text-emerald-200"
                                    : "border border-amber-500/20 bg-amber-500/10 text-amber-100"
                                )}
                              >
                                {selectedLastRollbackOutcome.outcome}
                              </span>
                            </div>
                            <div className="rounded-xl border border-border bg-surface px-3 py-2">
                              <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                                Recorded
                              </p>
                              <p className="mt-1 text-text">
                                {formatTimestamp(selectedLastRollbackOutcome.tsMs)}
                              </p>
                            </div>
                            <div className="rounded-xl border border-border bg-surface px-3 py-2">
                              <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                                Reason
                              </p>
                              <p className="mt-1 text-text">
                                {selectedLastRollbackOutcome.reason || "No reason recorded."}
                              </p>
                            </div>
                          </>
                        ) : (
                          <p className="rounded-xl border border-dashed border-border bg-surface px-3 py-4">
                            No rollback attempts recorded for this context run yet.
                          </p>
                        )}
                      </CardContent>
                    </Card>
                  </div>
                  <Card className="mt-3 border-border bg-surface-elevated/30">
                    <CardHeader className="pb-2">
                      <CardTitle className="flex items-center gap-2 text-sm">
                        <RefreshCw className="h-4 w-4" />
                        Rollback Operator
                      </CardTitle>
                      <CardDescription>
                        Review rollback preview steps, select executable work only, then execute
                        with explicit acknowledgement.
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-3 pt-0 text-sm text-text-muted">
                      <div className="flex flex-wrap gap-2">
                        <div className="rounded-xl border border-border bg-surface px-3 py-2">
                          <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                            Preview steps
                          </p>
                          <p className="mt-1 text-text">{rollbackPreviewSummary.stepCount}</p>
                        </div>
                        <div className="rounded-xl border border-border bg-surface px-3 py-2">
                          <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                            Executable
                          </p>
                          <p className="mt-1 text-text">
                            {rollbackPreviewSummary.executableStepCount}
                          </p>
                        </div>
                        <div className="rounded-xl border border-border bg-surface px-3 py-2">
                          <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                            Advisory
                          </p>
                          <p className="mt-1 text-text">
                            {rollbackPreviewSummary.advisoryStepCount}
                          </p>
                        </div>
                        <div className="rounded-xl border border-border bg-surface px-3 py-2">
                          <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                            Plan status
                          </p>
                          <p className="mt-1 text-text">
                            {rollbackPreviewSummary.executable ? "Fully executable" : "Mixed"}
                          </p>
                        </div>
                      </div>

                      <div className="flex flex-wrap gap-2">
                        <Button
                          variant="secondary"
                          size="sm"
                          loading={loadingRollbackPreview}
                          disabled={!linkedContextRunId}
                          onClick={() =>
                            linkedContextRunId && void loadRollbackPreview(linkedContextRunId)
                          }
                        >
                          <RefreshCw className="h-4 w-4" />
                          Reload preview
                        </Button>
                        <Button
                          variant="secondary"
                          size="sm"
                          disabled={rollbackPreviewSteps.every((step) => !step.executable)}
                          onClick={() => selectAllExecutableRollbackSteps()}
                        >
                          Select executable
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={rollbackSelectedEventIds.length === 0}
                          onClick={() => setRollbackSelectedEventIds([])}
                        >
                          Clear selection
                        </Button>
                      </div>

                      {rollbackPreviewError ? (
                        <div className="rounded-xl border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-amber-100">
                          {rollbackPreviewError}
                        </div>
                      ) : null}

                      {rollbackPreviewSteps.length > 0 ? (
                        <div className="grid gap-2">
                          {rollbackPreviewSteps.map((step) => {
                            const selected = rollbackSelectedEventIds.includes(step.eventId);
                            return (
                              <label
                                key={step.eventId}
                                className={cn(
                                  "flex cursor-pointer items-start gap-3 rounded-xl border px-3 py-3",
                                  selected
                                    ? "border-primary/40 bg-primary/10"
                                    : "border-border bg-surface",
                                  !step.executable && "opacity-80"
                                )}
                              >
                                <input
                                  type="checkbox"
                                  className="mt-1"
                                  checked={selected}
                                  disabled={!step.executable}
                                  onChange={() => toggleRollbackStepSelection(step.eventId)}
                                />
                                <div className="min-w-0 flex-1">
                                  <div className="flex flex-wrap items-center gap-2">
                                    <span className="font-medium text-text">{step.tool}</span>
                                    <span className="text-text-subtle">seq {step.seq ?? "?"}</span>
                                    <span
                                      className={cn(
                                        "rounded-full px-2 py-0.5 text-[10px] uppercase tracking-[0.18em]",
                                        step.executable
                                          ? "border border-emerald-500/20 bg-emerald-500/10 text-emerald-200"
                                          : "border border-amber-500/20 bg-amber-500/10 text-amber-100"
                                      )}
                                    >
                                      {step.executable ? "Executable" : "Advisory only"}
                                    </span>
                                  </div>
                                  <p className="mt-1 text-xs text-text-muted">
                                    {step.operationCount} rollback operation
                                    {step.operationCount === 1 ? "" : "s"} for {step.eventId}
                                  </p>
                                </div>
                              </label>
                            );
                          })}
                        </div>
                      ) : (
                        <div className="rounded-xl border border-dashed border-border bg-surface px-3 py-4">
                          {linkedContextRunId
                            ? "No rollback preview steps are available for this run."
                            : "This coder run is not linked to a context run yet."}
                        </div>
                      )}

                      <label className="flex items-start gap-3 rounded-xl border border-border bg-surface px-3 py-3 text-text">
                        <input
                          type="checkbox"
                          className="mt-1"
                          checked={rollbackPolicyAcknowledged}
                          onChange={(event) => setRollbackPolicyAcknowledged(event.target.checked)}
                        />
                        <span>
                          I acknowledge the rollback policy gate and want to submit the required
                          token{" "}
                          <span className="font-mono text-[11px]">
                            {selectedRollbackPolicy.requiredPolicyAck}
                          </span>
                          .
                        </span>
                      </label>

                      {rollbackExecutionResult ? (
                        <div
                          className={cn(
                            "rounded-xl border px-3 py-2",
                            asRecord(rollbackExecutionResult)?.applied === true
                              ? "border-emerald-500/20 bg-emerald-500/10 text-emerald-100"
                              : "border-amber-500/20 bg-amber-500/10 text-amber-100"
                          )}
                        >
                          {rollbackExecutionMessage}
                        </div>
                      ) : null}

                      {rollbackHistoryError ? (
                        <div className="rounded-xl border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-amber-100">
                          {rollbackHistoryError}
                        </div>
                      ) : null}

                      <div className="rounded-xl border border-border bg-surface px-3 py-3">
                        <div className="flex items-center justify-between gap-3">
                          <div>
                            <p className="text-[11px] uppercase tracking-[0.18em] text-text-subtle">
                              Rollback receipts
                            </p>
                            <p className="mt-1 text-xs text-text-muted">
                              Detailed applied and blocked rollback attempts from the linked context
                              run.
                            </p>
                          </div>
                          <Button
                            variant="ghost"
                            size="sm"
                            loading={loadingRollbackHistory}
                            disabled={!linkedContextRunId}
                            onClick={() =>
                              linkedContextRunId && void loadRollbackHistory(linkedContextRunId)
                            }
                          >
                            <History className="h-4 w-4" />
                            Reload
                          </Button>
                        </div>
                        <div className="mt-3 grid gap-2">
                          {rollbackHistoryEntries.length > 0 ? (
                            rollbackHistoryEntries.slice(0, 6).map((entry) => (
                              <div
                                key={entry.eventId}
                                className="rounded-xl border border-border bg-surface-elevated/40 px-3 py-3"
                              >
                                <div className="flex flex-wrap items-center gap-2">
                                  <span
                                    className={cn(
                                      "rounded-full px-2 py-0.5 text-[10px] uppercase tracking-[0.18em]",
                                      entry.outcome === "applied"
                                        ? "border border-emerald-500/20 bg-emerald-500/10 text-emerald-200"
                                        : "border border-amber-500/20 bg-amber-500/10 text-amber-100"
                                    )}
                                  >
                                    {entry.outcome}
                                  </span>
                                  <span className="text-xs text-text-subtle">
                                    {formatTimestamp(entry.tsMs)}
                                  </span>
                                  <span className="font-mono text-[11px] text-text-subtle">
                                    {entry.eventId}
                                  </span>
                                </div>
                                {entry.reason ? (
                                  <p className="mt-2 text-xs text-text">{entry.reason}</p>
                                ) : null}
                                <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-text-muted">
                                  <span>selected: {entry.selectedEventIds.length}</span>
                                  {entry.outcome === "applied" ? (
                                    <>
                                      <span>steps: {entry.appliedStepCount}</span>
                                      <span>operations: {entry.appliedOperationCount}</span>
                                    </>
                                  ) : null}
                                  {entry.seq !== null ? <span>seq: {entry.seq}</span> : null}
                                </div>
                                {entry.selectedEventIds.length > 0 ? (
                                  <p className="mt-2 text-[11px] text-text-muted">
                                    Selected: {entry.selectedEventIds.join(", ")}
                                  </p>
                                ) : null}
                                {entry.missingEventIds.length > 0 ? (
                                  <p className="mt-1 text-[11px] text-amber-100">
                                    Missing: {entry.missingEventIds.join(", ")}
                                  </p>
                                ) : null}
                                {entry.appliedByAction.length > 0 ? (
                                  <p className="mt-1 text-[11px] text-text-muted">
                                    Actions:{" "}
                                    {entry.appliedByAction
                                      .map(([action, count]) => `${action} (${count})`)
                                      .join(", ")}
                                  </p>
                                ) : null}
                              </div>
                            ))
                          ) : (
                            <p className="rounded-xl border border-dashed border-border bg-surface px-3 py-4 text-xs text-text-muted">
                              No detailed rollback receipts recorded yet.
                            </p>
                          )}
                        </div>
                      </div>

                      <div className="flex flex-wrap gap-2">
                        <Button
                          variant="primary"
                          size="sm"
                          loading={rollbackExecuting}
                          disabled={
                            !selectedRollbackPolicy.eligible ||
                            rollbackSelectedEventIds.length === 0 ||
                            !rollbackPolicyAcknowledged
                          }
                          onClick={() => void executeRollbackSelection()}
                        >
                          <RefreshCw className="h-4 w-4" />
                          Execute selected rollback
                        </Button>
                        <p className="self-center text-xs text-text-subtle">
                          Select at least one executable step and acknowledge policy before
                          execution.
                        </p>
                      </div>
                    </CardContent>
                  </Card>
                  <div className="mt-3 grid gap-3 xl:grid-cols-3">
                    <Card className="border-border bg-surface-elevated/30">
                      <CardHeader className="pb-2">
                        <CardTitle className="flex items-center gap-2 text-sm">
                          <GitBranch className="h-4 w-4" />
                          Changed Files
                        </CardTitle>
                        <CardDescription>
                          Files inferred from diff artifacts, task payloads, and run events.
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="space-y-2 pt-0">
                        {selectedCodingSignals.changedFiles.length === 0 ? (
                          <p className="text-sm text-text-muted">No changed files surfaced yet.</p>
                        ) : (
                          selectedCodingSignals.changedFiles.map((file) => (
                            <div
                              key={file}
                              className="rounded-xl border border-border bg-surface px-2.5 py-2 font-mono text-[11px] text-text-muted"
                            >
                              {file}
                            </div>
                          ))
                        )}
                      </CardContent>
                    </Card>

                    <Card className="border-border bg-surface-elevated/30">
                      <CardHeader className="pb-2">
                        <CardTitle className="flex items-center gap-2 text-sm">
                          <SquareCheckBig className="h-4 w-4" />
                          Verification Commands
                        </CardTitle>
                        <CardDescription>
                          Validation, build, test, and lint commands detected for this run.
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="space-y-2 pt-0">
                        {selectedCodingSignals.verificationCommands.length === 0 ? (
                          <p className="text-sm text-text-muted">
                            No verification commands surfaced yet.
                          </p>
                        ) : (
                          selectedCodingSignals.verificationCommands.map((command) => (
                            <div
                              key={command}
                              className="rounded-xl border border-border bg-surface px-2.5 py-2 font-mono text-[11px] text-text-muted"
                            >
                              {command}
                            </div>
                          ))
                        )}
                      </CardContent>
                    </Card>

                    <Card className="border-border bg-surface-elevated/30">
                      <CardHeader className="pb-2">
                        <CardTitle className="flex items-center gap-2 text-sm">
                          <Database className="h-4 w-4" />
                          Patch Summary
                        </CardTitle>
                        <CardDescription>
                          Latest high-level diff and implementation notes recovered from artifacts.
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="space-y-2 pt-0">
                        {selectedCodingSignals.patchSummaries.length === 0 ? (
                          <p className="text-sm text-text-muted">No patch summary surfaced yet.</p>
                        ) : (
                          selectedCodingSignals.patchSummaries.map((summary, index) => (
                            <div
                              key={`${index}-${summary.slice(0, 24)}`}
                              className="rounded-xl border border-border bg-surface px-2.5 py-2 text-sm text-text-muted"
                            >
                              {summary}
                            </div>
                          ))
                        )}
                      </CardContent>
                    </Card>

                    <Card className="border-border bg-surface-elevated/30">
                      <CardHeader className="pb-2">
                        <CardTitle className="flex items-center gap-2 text-sm">
                          <AlertCircle className="h-4 w-4" />
                          Validation Failures
                        </CardTitle>
                        <CardDescription>
                          Latest explicit failure details recovered from task and artifact
                          telemetry.
                        </CardDescription>
                      </CardHeader>
                      <CardContent className="space-y-2 pt-0">
                        {selectedCodingSignals.validationFailures.length === 0 ? (
                          <p className="text-sm text-text-muted">
                            No validation failure details surfaced yet.
                          </p>
                        ) : (
                          selectedCodingSignals.validationFailures.map((failure, index) => (
                            <div
                              key={`${index}-${failure.slice(0, 24)}`}
                              className="rounded-xl border border-rose-500/20 bg-rose-500/10 px-2.5 py-2 text-sm text-rose-100"
                            >
                              {failure}
                            </div>
                          ))
                        )}
                      </CardContent>
                    </Card>
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
                                      const taskSummary = codingTaskSummaryForTask(task);
                                      const owner = taskSummary?.owner || taskOwner(task);
                                      const verificationCommands = uniqueStrings(
                                        [
                                          ...(taskSummary?.verification_commands ?? []),
                                          ...extractTaskVerificationCommands(task),
                                        ],
                                        2
                                      );
                                      const changedFiles = uniqueStrings(
                                        [
                                          ...(taskSummary?.changed_files ?? []),
                                          ...taskChangedFiles(task),
                                        ],
                                        2
                                      );
                                      const diffFiles = uniqueStrings(
                                        taskSummary?.diff_files ?? [],
                                        2
                                      );
                                      const failure =
                                        taskSummary?.failure_detail || taskFailureDetail(task);
                                      const artifactPaths = uniqueStrings(
                                        taskSummary?.artifact_paths ?? [],
                                        2
                                      );
                                      const verificationOutcome = taskSummary?.verification_outcome;
                                      const verificationPassed = taskSummary?.verification_passed;
                                      const validationsAttempted =
                                        taskSummary?.validations_attempted;
                                      const latestFailingCommand =
                                        taskSummary?.latest_failing_command;
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
                                          <div className="mt-2">
                                            <Button
                                              variant="secondary"
                                              size="sm"
                                              onClick={() => {
                                                setSelectedTaskId(taskStableId(task));
                                                setDetailTab("overview");
                                              }}
                                            >
                                              Inspect
                                            </Button>
                                          </div>
                                          {owner ? (
                                            <div className="mt-2">
                                              <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                Owner {owner}
                                              </span>
                                            </div>
                                          ) : null}
                                          {changedFiles.length > 0 ? (
                                            <div className="mt-3 space-y-1">
                                              <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                Changed files
                                              </p>
                                              {changedFiles.map((file) => (
                                                <div
                                                  key={file}
                                                  className="rounded-lg border border-border bg-surface-elevated/40 px-2 py-1 font-mono text-[10px] text-text-muted"
                                                >
                                                  {file}
                                                </div>
                                              ))}
                                            </div>
                                          ) : null}
                                          {diffFiles.length > 0 ? (
                                            <div className="mt-3 space-y-1">
                                              <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                Diff files
                                              </p>
                                              {diffFiles.map((file) => (
                                                <div
                                                  key={file}
                                                  className="rounded-lg border border-border bg-surface-elevated/40 px-2 py-1 font-mono text-[10px] text-text-muted"
                                                >
                                                  {file}
                                                </div>
                                              ))}
                                            </div>
                                          ) : null}
                                          {verificationCommands.length > 0 ? (
                                            <div className="mt-3 space-y-1">
                                              <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                Verification
                                              </p>
                                              {verificationCommands.map((command) => (
                                                <div
                                                  key={command}
                                                  className="rounded-lg border border-border bg-surface-elevated/40 px-2 py-1 font-mono text-[10px] text-text-muted"
                                                >
                                                  {command}
                                                </div>
                                              ))}
                                            </div>
                                          ) : null}
                                          {verificationOutcome ||
                                          verificationPassed !== undefined ? (
                                            <div className="mt-3 grid gap-2 md:grid-cols-2">
                                              <div className="rounded-lg border border-border bg-surface-elevated/40 px-2 py-2">
                                                <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                  Outcome
                                                </p>
                                                <p className="mt-1 text-[11px] text-text">
                                                  {verificationOutcome || "Unknown"}
                                                </p>
                                              </div>
                                              <div className="rounded-lg border border-border bg-surface-elevated/40 px-2 py-2">
                                                <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                  Passed
                                                </p>
                                                <p className="mt-1 text-[11px] text-text">
                                                  {verificationPassed === undefined ||
                                                  verificationPassed === null
                                                    ? "Unknown"
                                                    : verificationPassed
                                                      ? "Yes"
                                                      : "No"}
                                                </p>
                                              </div>
                                            </div>
                                          ) : null}
                                          {typeof validationsAttempted === "number" ? (
                                            <div className="mt-3 rounded-lg border border-border bg-surface-elevated/40 px-2 py-2 text-[11px] text-text-muted">
                                              {validationsAttempted} validation checks recorded
                                            </div>
                                          ) : null}
                                          {latestFailingCommand ? (
                                            <div className="mt-3 rounded-lg border border-amber-500/20 bg-amber-500/10 px-2 py-2 font-mono text-[10px] text-amber-100">
                                              {latestFailingCommand}
                                            </div>
                                          ) : null}
                                          {failure ? (
                                            <div className="mt-3 rounded-lg border border-rose-500/20 bg-rose-500/10 px-2 py-2 text-[11px] text-rose-100">
                                              {failure}
                                            </div>
                                          ) : null}
                                          {artifactPaths.length > 0 ? (
                                            <div className="mt-3 space-y-1">
                                              <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                Artifacts
                                              </p>
                                              {artifactPaths.map((path) => (
                                                <div
                                                  key={path}
                                                  className="rounded-lg border border-border bg-surface-elevated/40 px-2 py-1 font-mono text-[10px] text-text-muted"
                                                >
                                                  {path}
                                                </div>
                                              ))}
                                            </div>
                                          ) : null}
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

                  <Card>
                    <CardHeader>
                      <CardTitle className="flex items-center gap-2 text-base">
                        <Search className="h-4 w-4" />
                        Task Inspector
                      </CardTitle>
                      <CardDescription>
                        Focus a single coding task to review ownership, artifacts, and verification.
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-4">
                      {!selectedTask ? (
                        <p className="text-sm text-text-muted">
                          Select a task from the kanban or validation list to inspect it here.
                        </p>
                      ) : (
                        <>
                          <div className="flex flex-wrap items-start justify-between gap-3">
                            <div>
                              <p className="text-sm font-medium text-text">
                                {taskLabel(selectedTask)}
                              </p>
                              <p className="mt-1 text-[11px] text-text-muted">
                                {pickText(selectedTask.workflow_node_id) ||
                                  pickText(selectedTask.task_type) ||
                                  "task"}
                              </p>
                            </div>
                            <span
                              className={cn(
                                "rounded-full border px-2 py-1 text-[10px] font-medium uppercase tracking-[0.18em]",
                                statusTone(
                                  selectedTaskSummary?.status ?? pickText(selectedTask.status)
                                )
                              )}
                            >
                              {selectedTaskSummary?.status ||
                                pickText(selectedTask.status) ||
                                "unknown"}
                            </span>
                          </div>

                          <div className="flex flex-wrap gap-2">
                            {taskKindLabel(selectedTask) ? (
                              <span className="rounded-full border border-border bg-surface-elevated/40 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                {taskKindLabel(selectedTask)}
                              </span>
                            ) : null}
                            {pickText(selectedTask.workflow_node_id) ? (
                              <span className="rounded-full border border-border bg-surface-elevated/40 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                {pickText(selectedTask.workflow_node_id)}
                              </span>
                            ) : null}
                            {pickText(selectedTask.id) ? (
                              <span className="rounded-full border border-border bg-surface-elevated/40 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                {pickText(selectedTask.id)}
                              </span>
                            ) : null}
                          </div>

                          <div className="grid gap-3 md:grid-cols-4">
                            {[
                              [
                                "Owner",
                                selectedTaskSummary?.owner || taskOwner(selectedTask) || "Unknown",
                              ],
                              ["Workflow", selectedTaskSummary?.workflow_class || "Unknown"],
                              ["Phase", selectedTaskSummary?.phase || "Unknown"],
                              ["Outcome", selectedTaskSummary?.verification_outcome || "Unknown"],
                              ["Failure Kind", selectedTaskSummary?.failure_kind || "None"],
                            ].map(([label, value]) => (
                              <div
                                key={label}
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3"
                              >
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  {label}
                                </p>
                                <p className="mt-1 text-sm font-medium text-text">{value}</p>
                              </div>
                            ))}
                          </div>

                          <div className="grid gap-3 md:grid-cols-2">
                            {[
                              [
                                "Passed",
                                selectedTaskSummary?.verification_passed === undefined ||
                                selectedTaskSummary?.verification_passed === null
                                  ? "Unknown"
                                  : selectedTaskSummary.verification_passed
                                    ? "Yes"
                                    : "No",
                              ],
                              [
                                "Checks",
                                typeof selectedTaskSummary?.validations_attempted === "number"
                                  ? String(selectedTaskSummary.validations_attempted)
                                  : "Unknown",
                              ],
                            ].map(([label, value]) => (
                              <div
                                key={label}
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3"
                              >
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  {label}
                                </p>
                                <p className="mt-1 text-sm font-medium text-text">{value}</p>
                              </div>
                            ))}
                          </div>

                          <div className="grid gap-3 md:grid-cols-2">
                            <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Write Scope
                              </p>
                              <p className="mt-1 text-sm text-text">
                                {taskWriteScope(selectedTask) || "Not declared"}
                              </p>
                            </div>
                            <div className="rounded-2xl border border-border bg-surface-elevated/40 p-3">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Verification Plan
                              </p>
                              <p className="mt-1 font-mono text-[11px] text-text">
                                {taskVerificationPlan(selectedTask) || "Not declared"}
                              </p>
                            </div>
                          </div>

                          <div className="flex flex-wrap gap-2">
                            {pickText(selectedTask.workflow_node_id) ? (
                              <Button
                                variant="secondary"
                                size="sm"
                                onClick={() =>
                                  openBlackboardContext(
                                    "task",
                                    pickText(selectedTask.workflow_node_id),
                                    null
                                  )
                                }
                              >
                                <PanelsTopLeft className="h-4 w-4" />
                                Jump To Task Column
                              </Button>
                            ) : null}
                            {pickText(selectedTask.source_event_id) ? (
                              <Button
                                variant="secondary"
                                size="sm"
                                onClick={() =>
                                  openRelatedEventContext(pickText(selectedTask.source_event_id))
                                }
                              >
                                <Workflow className="h-4 w-4" />
                                Jump To Timeline
                              </Button>
                            ) : null}
                          </div>

                          <div className="grid gap-4 xl:grid-cols-2">
                            <div className="space-y-2">
                              <p className="text-sm font-medium text-text">Changed files</p>
                              {uniqueStrings(
                                [
                                  ...(selectedTaskSummary?.changed_files ?? []),
                                  ...taskChangedFiles(selectedTask),
                                ],
                                8
                              ).map((file) => (
                                <div
                                  key={file}
                                  className="rounded-xl border border-border bg-surface-elevated/40 px-2.5 py-2 font-mono text-[11px] text-text-muted"
                                >
                                  {file}
                                </div>
                              ))}
                            </div>

                            <div className="space-y-2">
                              <p className="text-sm font-medium text-text">Verification commands</p>
                              {uniqueStrings(
                                [
                                  ...(selectedTaskSummary?.verification_commands ?? []),
                                  ...extractTaskVerificationCommands(selectedTask),
                                ],
                                8
                              ).map((command) => (
                                <div
                                  key={command}
                                  className="rounded-xl border border-border bg-surface-elevated/40 px-2.5 py-2 font-mono text-[11px] text-text-muted"
                                >
                                  {command}
                                </div>
                              ))}
                            </div>

                            <div className="space-y-2">
                              <p className="text-sm font-medium text-text">Diff files</p>
                              {(selectedTaskSummary?.diff_files ?? []).length === 0 ? (
                                <p className="text-sm text-text-muted">
                                  No diff file hints recorded for this task.
                                </p>
                              ) : (
                                (selectedTaskSummary?.diff_files ?? []).map((file) => (
                                  <div
                                    key={file}
                                    className="rounded-xl border border-border bg-surface-elevated/40 px-2.5 py-2 font-mono text-[11px] text-text-muted"
                                  >
                                    {file}
                                  </div>
                                ))
                              )}
                            </div>

                            <div className="space-y-2">
                              <p className="text-sm font-medium text-text">Artifact paths</p>
                              {(selectedTaskSummary?.artifact_paths ?? []).length === 0 ? (
                                <p className="text-sm text-text-muted">
                                  No artifact paths recorded for this task.
                                </p>
                              ) : (
                                (selectedTaskSummary?.artifact_paths ?? []).map((path) => (
                                  <button
                                    key={path}
                                    type="button"
                                    onClick={() => {
                                      setSelectedArtifactPath(path);
                                      setDetailTab("artifacts");
                                    }}
                                    className="w-full rounded-xl border border-border bg-surface-elevated/40 px-2.5 py-2 text-left font-mono text-[11px] text-text-muted transition-colors hover:bg-surface-elevated"
                                  >
                                    {path}
                                  </button>
                                ))
                              )}
                            </div>
                          </div>

                          {selectedTaskSummary?.latest_failing_command ? (
                            <div className="rounded-xl border border-amber-500/20 bg-amber-500/10 px-3 py-2">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-amber-200">
                                Latest Failing Command
                              </p>
                              <p className="mt-2 font-mono text-[11px] text-amber-100">
                                {selectedTaskSummary.latest_failing_command}
                              </p>
                            </div>
                          ) : null}

                          {selectedTaskSummary?.failure_detail ||
                          taskFailureDetail(selectedTask) ? (
                            <div className="rounded-xl border border-rose-500/20 bg-rose-500/10 px-3 py-2">
                              <p className="text-[11px] uppercase tracking-[0.2em] text-rose-200">
                                Failure Detail
                              </p>
                              <p className="mt-2 text-sm text-rose-100">
                                {selectedTaskSummary?.failure_detail ||
                                  taskFailureDetail(selectedTask)}
                              </p>
                            </div>
                          ) : null}

                          {(selectedTaskSummary?.workflow_events ?? []).length > 0 ? (
                            <div className="space-y-2">
                              <p className="text-sm font-medium text-text">
                                Recent Workflow Events
                              </p>
                              {(selectedTaskSummary?.workflow_events ?? [])
                                .slice(0, 6)
                                .map((event, index) => {
                                  const summary = summarizeWorkflowEvent(
                                    event as Record<string, unknown>
                                  );
                                  return (
                                    <div
                                      key={`${summary.event}-${summary.recordedAtMs ?? index}`}
                                      className="rounded-xl border border-border bg-surface-elevated/40 px-3 py-2"
                                    >
                                      <div className="flex flex-wrap items-center gap-2">
                                        <span className="rounded-full border border-border bg-surface px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted">
                                          {summary.event}
                                        </span>
                                        {summary.phase ? (
                                          <span className="rounded-full border border-sky-500/30 bg-sky-500/10 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-sky-200">
                                            {summary.phase}
                                          </span>
                                        ) : null}
                                        {summary.failureKind ? (
                                          <span className="rounded-full border border-amber-500/30 bg-amber-500/10 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-amber-100">
                                            {summary.failureKind}
                                          </span>
                                        ) : null}
                                      </div>
                                      <p className="mt-2 text-[11px] text-text-muted">
                                        {summary.reason}
                                      </p>
                                    </div>
                                  );
                                })}
                            </div>
                          ) : null}

                          {selectedTaskArtifacts.length > 0 ? (
                            <div className="flex flex-wrap gap-2">
                              {selectedTaskArtifacts.slice(0, 6).map((artifact) => (
                                <Button
                                  key={artifact.id}
                                  variant="secondary"
                                  size="sm"
                                  onClick={() => {
                                    setSelectedArtifactPath(artifact.path);
                                    setDetailTab(
                                      artifactCategory(artifact) === "validation"
                                        ? "validation"
                                        : "artifacts"
                                    );
                                  }}
                                >
                                  {artifact.artifact_type}
                                </Button>
                              ))}
                            </div>
                          ) : null}
                        </>
                      )}
                    </CardContent>
                  </Card>

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
                                  <button
                                    key={label}
                                    type="button"
                                    onClick={() => {
                                      if (label === "Decisions") {
                                        focusBlackboardSection("lineage");
                                      } else if (label === "Open questions") {
                                        focusBlackboardSection("questions");
                                      } else {
                                        if (latestBlackboardArtifact) {
                                          setSelectedArtifactPath(latestBlackboardArtifact.path);
                                          setDetailTab(
                                            artifactCategory(latestBlackboardArtifact) ===
                                              "validation"
                                              ? "validation"
                                              : "artifacts"
                                          );
                                        } else if (latestTriageArtifact) {
                                          setSelectedArtifactPath(latestTriageArtifact.path);
                                          setDetailTab("artifacts");
                                        } else {
                                          setDetailTab("artifacts");
                                        }
                                      }
                                    }}
                                    className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                                  >
                                    <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                      {label}
                                    </p>
                                    <p className="mt-1 text-lg font-semibold text-text">
                                      {String(value)}
                                    </p>
                                  </button>
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
                                      <button
                                        type="button"
                                        onClick={() =>
                                          openBlackboardContext(
                                            "task",
                                            blackboardRowStepId(latestDecision),
                                            blackboardRowSourceEventId(latestDecision)
                                          )
                                        }
                                        className="rounded-full border border-primary/20 bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.2em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                      >
                                        Step {blackboardRowStepId(latestDecision)}
                                      </button>
                                    ) : null}
                                    {blackboardRowSourceEventId(latestDecision) ? (
                                      <button
                                        type="button"
                                        onClick={() => {
                                          openBlackboardContext(
                                            "event",
                                            blackboardRowStepId(latestDecision),
                                            blackboardRowSourceEventId(latestDecision)
                                          );
                                        }}
                                        className="rounded-full border border-primary/20 bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.2em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                      >
                                        Event {blackboardRowSourceEventId(latestDecision)}
                                      </button>
                                    ) : null}
                                    {relatedArtifactsForBlackboardRow(artifacts, latestDecision)
                                      .slice(0, 3)
                                      .map((artifact) => (
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
                                          className="rounded-full border border-primary/20 bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated"
                                        >
                                          {artifact.artifact_type}
                                        </button>
                                      ))}
                                  </div>
                                </div>
                              ) : (
                                <p className="text-sm text-text-muted">
                                  No blackboard decisions yet.
                                </p>
                              )}
                              {blackboardTimeline.length > 0 ? (
                                <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_260px]">
                                  <div
                                    ref={blackboardLineageRef}
                                    className="rounded-3xl border border-border bg-surface-elevated/30 p-4"
                                  >
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
                                      {blackboardTimeline.map((item, index) => {
                                        const relatedArtifacts = relatedArtifactsForEvent(
                                          artifacts,
                                          item.stepId ?? "",
                                          item.sourceEventId ?? ""
                                        ).slice(0, 3);
                                        return (
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
                                                  <button
                                                    type="button"
                                                    onClick={() =>
                                                      openBlackboardContext(
                                                        "task",
                                                        item.stepId,
                                                        item.sourceEventId
                                                      )
                                                    }
                                                    className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                                  >
                                                    Step {item.stepId}
                                                  </button>
                                                ) : null}
                                                {item.sourceEventId ? (
                                                  <button
                                                    type="button"
                                                    onClick={() =>
                                                      openBlackboardContext(
                                                        "event",
                                                        item.stepId,
                                                        item.sourceEventId
                                                      )
                                                    }
                                                    className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                                  >
                                                    Source {item.sourceEventId}
                                                  </button>
                                                ) : null}
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
                                            </div>
                                          </div>
                                        );
                                      })}
                                    </div>
                                  </div>

                                  <div className="space-y-3">
                                    <div
                                      ref={blackboardQuestionsRef}
                                      className="rounded-3xl border border-border bg-surface-elevated/30 p-4"
                                    >
                                      <p className="text-sm font-medium text-text">
                                        Open questions
                                      </p>
                                      <p className="mt-1 text-xs text-text-muted">
                                        Outstanding uncertainty captured on the blackboard.
                                      </p>
                                      <div className="mt-3 space-y-2">
                                        {openQuestions.length > 0 ? (
                                          openQuestions.slice(0, 4).map((question, index) => {
                                            const relatedArtifacts =
                                              relatedArtifactsForBlackboardRow(
                                                artifacts,
                                                question
                                              ).slice(0, 2);
                                            return (
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
                                                    <button
                                                      type="button"
                                                      onClick={() =>
                                                        openBlackboardContext(
                                                          "task",
                                                          blackboardRowStepId(question),
                                                          blackboardRowSourceEventId(question)
                                                        )
                                                      }
                                                      className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                                    >
                                                      step {blackboardRowStepId(question)}
                                                    </button>
                                                  ) : null}
                                                  {blackboardRowSourceEventId(question) ? (
                                                    <button
                                                      type="button"
                                                      onClick={() =>
                                                        openBlackboardContext(
                                                          "event",
                                                          blackboardRowStepId(question),
                                                          blackboardRowSourceEventId(question)
                                                        )
                                                      }
                                                      className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                                    >
                                                      source {blackboardRowSourceEventId(question)}
                                                    </button>
                                                  ) : null}
                                                </div>
                                                {relatedArtifacts.length > 0 ? (
                                                  <div className="mt-3 flex flex-wrap gap-2">
                                                    {relatedArtifacts.map((artifact) => (
                                                      <button
                                                        key={artifact.id}
                                                        type="button"
                                                        onClick={() => {
                                                          setSelectedArtifactPath(artifact.path);
                                                          setDetailTab(
                                                            artifactCategory(artifact) ===
                                                              "validation"
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
                                          })
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
                              onClick={() => {
                                if (timelineContextArtifact) {
                                  openArtifactRecord(timelineContextArtifact);
                                } else {
                                  setDetailTab("artifacts");
                                }
                              }}
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
                            <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-dashed border-border bg-surface-elevated/30 p-3">
                              <p className="text-sm text-text-muted">
                                No run events match the current filters.
                              </p>
                              <Button variant="secondary" size="sm" onClick={resetTimelineFilters}>
                                Reset filters
                              </Button>
                            </div>
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
                                      <button
                                        type="button"
                                        onClick={() => {
                                          setEventQuery("");
                                          setEventTypeFilter((current) =>
                                            current === eventType ? "all" : eventType
                                          );
                                          focusOverviewSection("timeline");
                                        }}
                                        className="rounded-full border border-sky-500/20 bg-sky-500/10 px-2 py-0.5 text-[10px] uppercase tracking-[0.18em] text-sky-200 transition-colors hover:bg-sky-500/15"
                                      >
                                        {eventType.replace(/_/g, " ")}
                                      </button>
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
                                        <button
                                          type="button"
                                          onClick={() => focusOverviewSection("kanban")}
                                          className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                        >
                                          Step {stepId}
                                        </button>
                                      ) : null}
                                      {sourceEventId ? (
                                        <button
                                          type="button"
                                          onClick={() => {
                                            setEventTypeFilter("all");
                                            setEventQuery(sourceEventId);
                                            focusOverviewSection("timeline");
                                          }}
                                          className="rounded-full border border-border bg-surface-elevated/50 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                        >
                                          Event {sourceEventId}
                                        </button>
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
                                              onClick={() => openArtifactRecord(artifact)}
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
                            <button
                              type="button"
                              onClick={() => focusTabSection("memory", "memory_hits")}
                              className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                            >
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Hits
                              </p>
                              <p className="mt-1 text-lg font-semibold text-text">
                                {memoryHits.length}
                              </p>
                            </button>
                            <button
                              type="button"
                              onClick={() => focusTabSection("memory", "memory_candidates")}
                              className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                            >
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Candidates
                              </p>
                              <p className="mt-1 text-lg font-semibold text-text">
                                {memoryCandidates.length}
                              </p>
                            </button>
                          </div>
                          <div className="space-y-2">
                            {memoryHits.slice(0, 3).map((hit, index) => (
                              <button
                                key={String(hit.candidate_id ?? hit.memory_id ?? index)}
                                type="button"
                                onClick={() => focusTabSection("memory", "memory_hits")}
                                className="w-full rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                              >
                                <pre className="whitespace-pre-wrap break-words text-[11px] text-text-muted">
                                  {renderValue(hit.summary ?? hit.content ?? hit.payload ?? hit)}
                                </pre>
                              </button>
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
                                key: "duplicate" as const,
                                group: artifactGroups.find((group) => group.key === "duplicate"),
                              },
                              {
                                label: "Triage",
                                key: "triage" as const,
                                group: artifactGroups.find((group) => group.key === "triage"),
                              },
                              {
                                label: "Memory",
                                key: "memory" as const,
                                group: artifactGroups.find((group) => group.key === "memory"),
                              },
                              {
                                label: "Validation",
                                key: "validation" as const,
                                group: artifactGroups.find((group) => group.key === "validation"),
                              },
                            ].map(({ key, label, group }) => (
                              <button
                                key={label}
                                type="button"
                                onClick={() => {
                                  const artifact = latestArtifactByCategory.get(key);
                                  if (artifact) {
                                    setSelectedArtifactPath(artifact.path);
                                  }
                                  if (key === "validation") {
                                    setDetailTab("validation");
                                  }
                                }}
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                              >
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  {label}
                                </p>
                                <p className="mt-1 text-lg font-semibold text-text">
                                  {String(group?.artifacts.length ?? 0)}
                                </p>
                              </button>
                            ))}
                          </div>

                          <div className="space-y-4">
                            {artifactGroups.length === 0 ? (
                              <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-dashed border-border bg-surface-elevated/30 p-3">
                                <p className="text-sm text-text-muted">
                                  No artifacts match the current filter.
                                </p>
                                <Button
                                  variant="secondary"
                                  size="sm"
                                  onClick={resetArtifactFilters}
                                >
                                  Reset filters
                                </Button>
                              </div>
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
                                          <button
                                            type="button"
                                            onClick={(event) => {
                                              event.stopPropagation();
                                              openArtifactRecordContext(artifact, "task");
                                            }}
                                            className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                          >
                                            Step {artifact.step_id}
                                          </button>
                                        ) : null}
                                        {artifact.source_event_id ? (
                                          <button
                                            type="button"
                                            onClick={(event) => {
                                              event.stopPropagation();
                                              openArtifactRecordContext(artifact, "event");
                                            }}
                                            className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                          >
                                            Event {artifact.source_event_id}
                                          </button>
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
                          <button
                            type="button"
                            onClick={() => {
                              if (
                                selectedArtifactRecord &&
                                artifactCategory(selectedArtifactRecord) === "validation"
                              ) {
                                focusTabSection("validation", "validation_artifacts");
                              } else {
                                setDetailTab("artifacts");
                              }
                            }}
                            className="w-full rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                          >
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
                                <button
                                  type="button"
                                  onClick={() => {
                                    const category = artifactCategory(selectedArtifactRecord);
                                    const artifact = latestArtifactByCategory.get(category);
                                    if (artifact) {
                                      setSelectedArtifactPath(artifact.path);
                                    }
                                    setDetailTab(
                                      category === "validation" ? "validation" : "artifacts"
                                    );
                                  }}
                                  className={cn(
                                    "rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] transition-colors hover:opacity-90",
                                    artifactCategoryTone(artifactCategory(selectedArtifactRecord))
                                  )}
                                >
                                  {artifactCategory(selectedArtifactRecord)}
                                </button>
                              ) : null}
                            </div>
                            {selectedArtifactRecord?.step_id ||
                            selectedArtifactRecord?.source_event_id ? (
                              <div className="mt-3 flex flex-wrap gap-2">
                                {selectedArtifactRecord.step_id ? (
                                  <button
                                    type="button"
                                    onClick={() => openArtifactContext("task")}
                                    className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                  >
                                    Step {selectedArtifactRecord.step_id}
                                  </button>
                                ) : null}
                                {selectedArtifactRecord.source_event_id ? (
                                  <button
                                    type="button"
                                    onClick={() => openArtifactContext("event")}
                                    className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                  >
                                    Event {selectedArtifactRecord.source_event_id}
                                  </button>
                                ) : null}
                              </div>
                            ) : null}
                            {selectedArtifactRecord ? (
                              <div className="mt-3 flex flex-wrap gap-2">
                                {selectedArtifactSiblings.olderInCategory ? (
                                  <>
                                    <button
                                      type="button"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setSelectedArtifactPath(
                                          selectedArtifactSiblings.olderInCategory?.path ?? null
                                        );
                                      }}
                                      className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                    >
                                      Older
                                    </button>
                                    <button
                                      type="button"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setCompareArtifactPath(
                                          selectedArtifactSiblings.olderInCategory?.path ?? null
                                        );
                                      }}
                                      className="rounded-full border border-primary/20 bg-primary/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text transition-colors hover:bg-primary/15"
                                    >
                                      Compare older
                                    </button>
                                  </>
                                ) : null}
                                {selectedArtifactSiblings.newerInCategory ? (
                                  <>
                                    <button
                                      type="button"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setSelectedArtifactPath(
                                          selectedArtifactSiblings.newerInCategory?.path ?? null
                                        );
                                      }}
                                      className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                    >
                                      Newer
                                    </button>
                                    <button
                                      type="button"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setCompareArtifactPath(
                                          selectedArtifactSiblings.newerInCategory?.path ?? null
                                        );
                                      }}
                                      className="rounded-full border border-primary/20 bg-primary/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text transition-colors hover:bg-primary/15"
                                    >
                                      Compare newer
                                    </button>
                                  </>
                                ) : null}
                                {selectedArtifactSiblings.sameStepLatest ? (
                                  <>
                                    <button
                                      type="button"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setSelectedArtifactPath(
                                          selectedArtifactSiblings.sameStepLatest?.path ?? null
                                        );
                                      }}
                                      className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                    >
                                      Same step{" "}
                                      {selectedArtifactSiblings.sameStepCount > 1
                                        ? `· ${selectedArtifactSiblings.sameStepCount}`
                                        : ""}
                                    </button>
                                    <button
                                      type="button"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setCompareArtifactPath(
                                          selectedArtifactSiblings.sameStepLatest?.path ?? null
                                        );
                                      }}
                                      className="rounded-full border border-primary/20 bg-primary/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text transition-colors hover:bg-primary/15"
                                    >
                                      Compare step
                                    </button>
                                  </>
                                ) : null}
                                {selectedArtifactSiblings.sameEventLatest ? (
                                  <>
                                    <button
                                      type="button"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setSelectedArtifactPath(
                                          selectedArtifactSiblings.sameEventLatest?.path ?? null
                                        );
                                      }}
                                      className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                    >
                                      Same event{" "}
                                      {selectedArtifactSiblings.sameEventCount > 1
                                        ? `· ${selectedArtifactSiblings.sameEventCount}`
                                        : ""}
                                    </button>
                                    <button
                                      type="button"
                                      onClick={(event) => {
                                        event.stopPropagation();
                                        setCompareArtifactPath(
                                          selectedArtifactSiblings.sameEventLatest?.path ?? null
                                        );
                                      }}
                                      className="rounded-full border border-primary/20 bg-primary/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text transition-colors hover:bg-primary/15"
                                    >
                                      Compare event
                                    </button>
                                  </>
                                ) : null}
                              </div>
                            ) : null}
                          </button>
                          <div className="flex flex-wrap gap-2">
                            {selectedDuplicateMatches.length > 0 ? (
                              <button
                                type="button"
                                onClick={() => {
                                  openDuplicateArtifactContext();
                                }}
                                className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-amber-100 transition-colors hover:bg-amber-500/15"
                              >
                                {selectedDuplicateMatches.length} duplicate matches
                              </button>
                            ) : null}
                            {selectedValidationSummary?.outcome ? (
                              <button
                                type="button"
                                onClick={() =>
                                  focusTabSection("validation", "validation_inspector")
                                }
                                className="rounded-full border border-violet-500/20 bg-violet-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-violet-200 transition-colors hover:bg-violet-500/15"
                              >
                                {selectedValidationSummary.outcome}
                              </button>
                            ) : null}
                            {selectedValidationSummary?.validationsAttempted ? (
                              <button
                                type="button"
                                onClick={() => focusTabSection("validation", "validation_tasks")}
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
                                <button
                                  type="button"
                                  onClick={openDuplicateArtifactContext}
                                  className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-amber-100 transition-colors hover:bg-amber-500/15"
                                >
                                  {selectedDuplicateMatches.length} matches
                                </button>
                              </div>
                              <div className="mt-3 space-y-2">
                                {selectedDuplicateMatches.slice(0, 5).map((match, index) => (
                                  <button
                                    key={String(match.id ?? match.candidate_id ?? index)}
                                    type="button"
                                    onClick={openDuplicateArtifactContext}
                                    className="w-full rounded-2xl border border-amber-500/20 bg-surface p-3 text-left transition-colors hover:bg-surface-elevated"
                                  >
                                    <p className="text-sm font-medium text-text">
                                      {duplicateMatchLabel(match)}
                                    </p>
                                    <div className="mt-2 flex flex-wrap gap-2">
                                      {duplicateMatchBadges(match).map((badge) => (
                                        <button
                                          key={badge}
                                          type="button"
                                          onClick={(event) => {
                                            event.stopPropagation();
                                            void copyDuplicateBadgeValue(badge);
                                          }}
                                          className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                        >
                                          {copiedDuplicateBadge === badge ? "Copied" : badge}
                                        </button>
                                      ))}
                                    </div>
                                  </button>
                                ))}
                              </div>
                            </div>
                          ) : null}

                          {selectedValidationSummary ? (
                            <div className="grid gap-3 md:grid-cols-3">
                              <button
                                type="button"
                                onClick={() =>
                                  focusTabSection("validation", "validation_inspector")
                                }
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                              >
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  Outcome
                                </p>
                                <p className="mt-1 text-sm font-medium text-text">
                                  {selectedValidationSummary.outcome || "Unknown"}
                                </p>
                              </button>
                              <button
                                type="button"
                                onClick={() =>
                                  focusTabSection("validation", "validation_artifacts")
                                }
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                              >
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
                              </button>
                              <button
                                type="button"
                                onClick={() => focusTabSection("validation", "validation_tasks")}
                                className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                              >
                                <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                  Checks
                                </p>
                                <p className="mt-1 text-sm font-medium text-text">
                                  {selectedValidationSummary.validationsAttempted ?? 0}
                                </p>
                              </button>
                            </div>
                          ) : null}
                          {renderCompareArtifactPreview()}
                          {selectedArtifactContext ? (
                            <div className="rounded-3xl border border-sky-500/20 bg-sky-500/5 p-4">
                              <div className="flex items-start justify-between gap-3">
                                <div>
                                  <p className="text-sm font-medium text-text">Related lineage</p>
                                  <p className="text-xs text-text-muted">
                                    Blackboard rows, run events, and memory evidence sharing this
                                    artifact context.
                                  </p>
                                </div>
                                <span className="rounded-full border border-sky-500/20 bg-sky-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-sky-200">
                                  {relatedBlackboardRows.length +
                                    relatedTimelineEvents.length +
                                    relatedMemoryCandidates.length +
                                    relatedMemoryHits.length}{" "}
                                  links
                                </span>
                              </div>
                              <div className="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                                <button
                                  type="button"
                                  onClick={() => focusBlackboardSection("lineage")}
                                  className="rounded-2xl border border-border bg-surface p-3 text-left transition-colors hover:bg-surface-elevated"
                                >
                                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                    Blackboard
                                  </p>
                                  <p className="mt-1 text-sm font-medium text-text">
                                    {relatedBlackboardRows.length}
                                  </p>
                                </button>
                                <button
                                  type="button"
                                  onClick={() => {
                                    if (selectedArtifactContext.sourceEventId) {
                                      openRelatedEventContext(
                                        selectedArtifactContext.sourceEventId
                                      );
                                    } else {
                                      focusOverviewSection("timeline");
                                    }
                                  }}
                                  className="rounded-2xl border border-border bg-surface p-3 text-left transition-colors hover:bg-surface-elevated"
                                >
                                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                    Timeline
                                  </p>
                                  <p className="mt-1 text-sm font-medium text-text">
                                    {relatedTimelineEvents.length}
                                  </p>
                                </button>
                                <button
                                  type="button"
                                  onClick={() => focusTabSection("memory", "memory_candidates")}
                                  className="rounded-2xl border border-border bg-surface p-3 text-left transition-colors hover:bg-surface-elevated"
                                >
                                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                    Candidates
                                  </p>
                                  <p className="mt-1 text-sm font-medium text-text">
                                    {relatedMemoryCandidates.length}
                                  </p>
                                </button>
                                <button
                                  type="button"
                                  onClick={() => focusTabSection("memory", "memory_hits")}
                                  className="rounded-2xl border border-border bg-surface p-3 text-left transition-colors hover:bg-surface-elevated"
                                >
                                  <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                    Hits
                                  </p>
                                  <p className="mt-1 text-sm font-medium text-text">
                                    {relatedMemoryHits.length}
                                  </p>
                                </button>
                              </div>
                              <div className="mt-3 space-y-3">
                                {relatedBlackboardRows.length > 0 ? (
                                  <div className="rounded-2xl border border-border bg-surface p-3">
                                    <div className="mb-2 flex items-center justify-between gap-3">
                                      <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                        Blackboard
                                      </p>
                                      <span className="text-[11px] text-text-muted">
                                        {relatedBlackboardRows.length}
                                      </span>
                                    </div>
                                    <div className="space-y-2">
                                      {relatedBlackboardRows.map((row, index) => (
                                        <button
                                          key={String(row.id ?? row.source_event_id ?? index)}
                                          type="button"
                                          onClick={() => {
                                            if (blackboardRowSourceEventId(row)) {
                                              openBlackboardContext(
                                                "event",
                                                blackboardRowStepId(row),
                                                blackboardRowSourceEventId(row)
                                              );
                                            } else {
                                              focusBlackboardSection("lineage");
                                            }
                                          }}
                                          className="w-full rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                                        >
                                          <p className="text-sm font-medium text-text">
                                            {blackboardRowText(row)}
                                          </p>
                                          <div className="mt-2 flex flex-wrap gap-2">
                                            {blackboardRowStepId(row) ? (
                                              <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                Step {blackboardRowStepId(row)}
                                              </span>
                                            ) : null}
                                            {blackboardRowSourceEventId(row) ? (
                                              <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                Event {blackboardRowSourceEventId(row)}
                                              </span>
                                            ) : null}
                                          </div>
                                        </button>
                                      ))}
                                    </div>
                                  </div>
                                ) : null}
                                {relatedTimelineEvents.length > 0 ? (
                                  <div className="rounded-2xl border border-border bg-surface p-3">
                                    <div className="mb-2 flex items-center justify-between gap-3">
                                      <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                        Timeline
                                      </p>
                                      <span className="text-[11px] text-text-muted">
                                        {relatedTimelineEvents.length}
                                      </span>
                                    </div>
                                    <div className="space-y-2">
                                      {relatedTimelineEvents.map((event, index) => (
                                        <button
                                          key={runEventId(event, index)}
                                          type="button"
                                          onClick={() =>
                                            openRelatedEventContext(pickText(event.event_id))
                                          }
                                          className="w-full rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                                        >
                                          <div className="flex flex-wrap items-center gap-2">
                                            <span className="rounded-full border border-sky-500/20 bg-sky-500/10 px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-sky-200">
                                              {runEventType(event).replace(/_/g, " ")}
                                            </span>
                                            <span className="text-[11px] text-text-muted">
                                              {formatTimestamp(runEventTimestamp(event))}
                                            </span>
                                          </div>
                                          {runEventText(event) ? (
                                            <p className="mt-2 text-sm text-text">
                                              {runEventText(event)}
                                            </p>
                                          ) : null}
                                        </button>
                                      ))}
                                    </div>
                                  </div>
                                ) : null}
                                {relatedMemoryCandidates.length > 0 ? (
                                  <div className="rounded-2xl border border-border bg-surface p-3">
                                    <div className="mb-2 flex items-center justify-between gap-3">
                                      <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                        Memory candidates
                                      </p>
                                      <span className="text-[11px] text-text-muted">
                                        {relatedMemoryCandidates.length}
                                      </span>
                                    </div>
                                    <div className="space-y-2">
                                      {relatedMemoryCandidates.map((candidate) => (
                                        <button
                                          key={candidate.candidate_id}
                                          type="button"
                                          onClick={() => {
                                            if (candidate.artifact) {
                                              openArtifactRecord(candidate.artifact);
                                            } else {
                                              focusTabSection("memory", "memory_candidates");
                                            }
                                          }}
                                          className="w-full rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                                        >
                                          <div className="flex items-center justify-between gap-3">
                                            <p className="text-sm font-medium text-text">
                                              {candidate.summary || candidate.kind}
                                            </p>
                                            <span className="text-[11px] text-text-muted">
                                              {candidate.created_at_ms
                                                ? formatTimestamp(candidate.created_at_ms)
                                                : "candidate"}
                                            </span>
                                          </div>
                                          <div className="mt-2 flex flex-wrap gap-2">
                                            <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                              {candidate.kind}
                                            </span>
                                            {candidate.artifact?.path ? (
                                              <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                                Artifact-backed
                                              </span>
                                            ) : null}
                                          </div>
                                        </button>
                                      ))}
                                    </div>
                                  </div>
                                ) : null}
                                {relatedMemoryHits.length > 0 ? (
                                  <div className="rounded-2xl border border-border bg-surface p-3">
                                    <div className="mb-2 flex items-center justify-between gap-3">
                                      <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                        Memory hits
                                      </p>
                                      <span className="text-[11px] text-text-muted">
                                        {relatedMemoryHits.length}
                                      </span>
                                    </div>
                                    <div className="space-y-2">
                                      {relatedMemoryHits.map((hit, index) => (
                                        <button
                                          key={String(hit.candidate_id ?? hit.memory_id ?? index)}
                                          type="button"
                                          onClick={() => focusTabSection("memory", "memory_hits")}
                                          className="w-full rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                                        >
                                          <p className="text-sm font-medium text-text">
                                            {pickText(hit.summary) ||
                                              pickText(hit.memory_id) ||
                                              pickText(hit.candidate_id) ||
                                              "Memory hit"}
                                          </p>
                                          <pre className="mt-2 whitespace-pre-wrap break-words text-[11px] text-text-muted">
                                            {renderValue(
                                              hit.summary ?? hit.content ?? hit.payload ?? hit
                                            )}
                                          </pre>
                                        </button>
                                      ))}
                                    </div>
                                  </div>
                                ) : null}
                                {relatedBlackboardRows.length === 0 &&
                                relatedTimelineEvents.length === 0 &&
                                relatedMemoryCandidates.length === 0 &&
                                relatedMemoryHits.length === 0 ? (
                                  <div className="rounded-2xl border border-dashed border-border bg-surface-elevated/30 p-3">
                                    <p className="text-sm text-text-muted">
                                      No related lineage was found for the current artifact context.
                                    </p>
                                  </div>
                                ) : null}
                              </div>
                            </div>
                          ) : null}
                          {renderArtifactPreviewContent("No artifact preview available.")}
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
                          ["Changed files", selectedCodingSignals.changedFiles.length],
                          ["Commands", selectedCodingSignals.verificationCommands.length],
                          ["Patch notes", selectedCodingSignals.patchSummaries.length],
                          ["Failures", selectedCodingSignals.validationFailures.length],
                        ].map(([label, value]) => (
                          <button
                            key={label}
                            type="button"
                            onClick={() => {
                              if (label === "Validation tasks") {
                                focusTabSection("validation", "validation_tasks");
                              } else if (label === "Validation artifacts") {
                                if (latestValidationArtifact) {
                                  setSelectedArtifactPath(latestValidationArtifact.path);
                                }
                                focusTabSection("validation", "validation_artifacts");
                              } else if (label === "Selected checks") {
                                focusTabSection("validation", "validation_inspector");
                              } else {
                                focusTabSection("validation", "validation_tasks");
                              }
                            }}
                            className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                          >
                            <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                              {label}
                            </p>
                            <p className="mt-1 text-lg font-semibold text-text">{String(value)}</p>
                          </button>
                        ))}
                      </div>

                      <div className="grid gap-4 xl:grid-cols-3">
                        <div className="space-y-2">
                          <p className="text-sm font-medium text-text">Changed files</p>
                          {selectedCodingSignals.changedFiles.length === 0 ? (
                            <p className="text-sm text-text-muted">
                              No changed files surfaced from validation context yet.
                            </p>
                          ) : (
                            selectedCodingSignals.changedFiles.map((file) => (
                              <div
                                key={file}
                                className="rounded-xl border border-border bg-surface-elevated/40 px-2.5 py-2 font-mono text-[11px] text-text-muted"
                              >
                                {file}
                              </div>
                            ))
                          )}
                        </div>

                        <div className="space-y-2">
                          <p className="text-sm font-medium text-text">Verification commands</p>
                          {selectedCodingSignals.verificationCommands.length === 0 ? (
                            <p className="text-sm text-text-muted">
                              No verification commands were captured yet.
                            </p>
                          ) : (
                            selectedCodingSignals.verificationCommands.map((command) => (
                              <div
                                key={command}
                                className="rounded-xl border border-border bg-surface-elevated/40 px-2.5 py-2 font-mono text-[11px] text-text-muted"
                              >
                                {command}
                              </div>
                            ))
                          )}
                        </div>

                        <div className="space-y-2">
                          <p className="text-sm font-medium text-text">Patch summaries</p>
                          {selectedCodingSignals.patchSummaries.length === 0 ? (
                            <p className="text-sm text-text-muted">
                              No patch summaries were captured yet.
                            </p>
                          ) : (
                            selectedCodingSignals.patchSummaries.map((summary, index) => (
                              <div
                                key={`${index}-${summary.slice(0, 24)}`}
                                className="rounded-xl border border-border bg-surface-elevated/40 px-2.5 py-2 text-sm text-text-muted"
                              >
                                {summary}
                              </div>
                            ))
                          )}
                        </div>

                        <div className="space-y-2">
                          <p className="text-sm font-medium text-text">Failure details</p>
                          {selectedCodingSignals.validationFailures.length === 0 ? (
                            <p className="text-sm text-text-muted">
                              No explicit failure detail was captured yet.
                            </p>
                          ) : (
                            selectedCodingSignals.validationFailures.map((failure, index) => (
                              <div
                                key={`${index}-${failure.slice(0, 24)}`}
                                className="rounded-xl border border-rose-500/20 bg-rose-500/10 px-2.5 py-2 text-sm text-rose-100"
                              >
                                {failure}
                              </div>
                            ))
                          )}
                        </div>
                      </div>

                      <div ref={validationTasksRef} className="space-y-2">
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
                            const taskSummary = codingTaskSummaryForTask(task);
                            const owner = taskSummary?.owner || taskOwner(task);
                            const verificationCommands = uniqueStrings(
                              [
                                ...(taskSummary?.verification_commands ?? []),
                                ...extractTaskVerificationCommands(task),
                              ],
                              3
                            );
                            const changedFiles = uniqueStrings(
                              [...(taskSummary?.changed_files ?? []), ...taskChangedFiles(task)],
                              3
                            );
                            const diffFiles = uniqueStrings(taskSummary?.diff_files ?? [], 3);
                            const artifactPaths = uniqueStrings(
                              taskSummary?.artifact_paths ?? [],
                              3
                            );
                            const failure = taskSummary?.failure_detail || taskFailureDetail(task);
                            const verificationOutcome = taskSummary?.verification_outcome;
                            const verificationPassed = taskSummary?.verification_passed;
                            const validationsAttempted = taskSummary?.validations_attempted;
                            const latestFailingCommand = taskSummary?.latest_failing_command;
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
                                <div className="mt-2">
                                  <Button
                                    variant="secondary"
                                    size="sm"
                                    onClick={() => {
                                      setSelectedTaskId(taskStableId(task));
                                      setDetailTab("overview");
                                    }}
                                  >
                                    Inspect
                                  </Button>
                                </div>
                                {owner ? (
                                  <div className="mt-2">
                                    <span className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                      Owner {owner}
                                    </span>
                                  </div>
                                ) : null}
                                {verificationCommands.length > 0 ? (
                                  <div className="mt-3 space-y-1">
                                    <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                      Commands
                                    </p>
                                    {verificationCommands.map((command) => (
                                      <div
                                        key={command}
                                        className="rounded-lg border border-border bg-surface px-2 py-1 font-mono text-[10px] text-text-muted"
                                      >
                                        {command}
                                      </div>
                                    ))}
                                  </div>
                                ) : null}
                                {changedFiles.length > 0 ? (
                                  <div className="mt-3 space-y-1">
                                    <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                      Changed files
                                    </p>
                                    {changedFiles.map((file) => (
                                      <div
                                        key={file}
                                        className="rounded-lg border border-border bg-surface px-2 py-1 font-mono text-[10px] text-text-muted"
                                      >
                                        {file}
                                      </div>
                                    ))}
                                  </div>
                                ) : null}
                                {diffFiles.length > 0 ? (
                                  <div className="mt-3 space-y-1">
                                    <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                      Diff files
                                    </p>
                                    {diffFiles.map((file) => (
                                      <div
                                        key={file}
                                        className="rounded-lg border border-border bg-surface px-2 py-1 font-mono text-[10px] text-text-muted"
                                      >
                                        {file}
                                      </div>
                                    ))}
                                  </div>
                                ) : null}
                                {verificationOutcome || verificationPassed !== undefined ? (
                                  <div className="mt-3 grid gap-2 md:grid-cols-3">
                                    <div className="rounded-lg border border-border bg-surface px-2 py-2">
                                      <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                        Outcome
                                      </p>
                                      <p className="mt-1 text-[11px] text-text">
                                        {verificationOutcome || "Unknown"}
                                      </p>
                                    </div>
                                    <div className="rounded-lg border border-border bg-surface px-2 py-2">
                                      <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                        Passed
                                      </p>
                                      <p className="mt-1 text-[11px] text-text">
                                        {verificationPassed === undefined ||
                                        verificationPassed === null
                                          ? "Unknown"
                                          : verificationPassed
                                            ? "Yes"
                                            : "No"}
                                      </p>
                                    </div>
                                    <div className="rounded-lg border border-border bg-surface px-2 py-2">
                                      <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                        Checks
                                      </p>
                                      <p className="mt-1 text-[11px] text-text">
                                        {typeof validationsAttempted === "number"
                                          ? validationsAttempted
                                          : "Unknown"}
                                      </p>
                                    </div>
                                  </div>
                                ) : null}
                                {latestFailingCommand ? (
                                  <div className="mt-3 rounded-xl border border-amber-500/20 bg-amber-500/10 px-3 py-2 font-mono text-[10px] text-amber-100">
                                    {latestFailingCommand}
                                  </div>
                                ) : null}
                                {failure ? (
                                  <div className="mt-3 rounded-xl border border-rose-500/20 bg-rose-500/10 px-3 py-2 text-[11px] text-rose-100">
                                    {failure}
                                  </div>
                                ) : null}
                                {artifactPaths.length > 0 ? (
                                  <div className="mt-3 space-y-1">
                                    <p className="text-[10px] uppercase tracking-[0.16em] text-text-muted">
                                      Artifact paths
                                    </p>
                                    {artifactPaths.map((path) => (
                                      <div
                                        key={path}
                                        className="rounded-lg border border-border bg-surface px-2 py-1 font-mono text-[10px] text-text-muted"
                                      >
                                        {path}
                                      </div>
                                    ))}
                                  </div>
                                ) : null}
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

                      <div ref={validationArtifactsRef} className="space-y-2">
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
                                  <button
                                    type="button"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      openArtifactRecordContext(artifact, "task");
                                    }}
                                    className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                  >
                                    Step {artifact.step_id}
                                  </button>
                                ) : null}
                                {artifact.source_event_id ? (
                                  <button
                                    type="button"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      openArtifactRecordContext(artifact, "event");
                                    }}
                                    className="rounded-full border border-border px-2 py-0.5 text-[10px] uppercase tracking-[0.16em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                  >
                                    Event {artifact.source_event_id}
                                  </button>
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
                    <CardContent ref={validationInspectorRef} className="space-y-3">
                      {selectedArtifactPath ? (
                        <>
                          <button
                            type="button"
                            onClick={() => {
                              focusTabSection("validation", "validation_artifacts");
                            }}
                            className="w-full rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                          >
                            <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                              Selected artifact
                            </p>
                            <p className="mt-2 break-all font-mono text-[11px] text-text-muted">
                              {selectedArtifactPath}
                            </p>
                            {selectedArtifactRecord?.step_id ||
                            selectedArtifactRecord?.source_event_id ? (
                              <div className="mt-3 flex flex-wrap gap-2">
                                {selectedArtifactRecord.step_id ? (
                                  <button
                                    type="button"
                                    onClick={() => openArtifactContext("task")}
                                    className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                  >
                                    Step {selectedArtifactRecord.step_id}
                                  </button>
                                ) : null}
                                {selectedArtifactRecord.source_event_id ? (
                                  <button
                                    type="button"
                                    onClick={() => openArtifactContext("event")}
                                    className="rounded-full border border-border bg-surface px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                                  >
                                    Event {selectedArtifactRecord.source_event_id}
                                  </button>
                                ) : null}
                              </div>
                            ) : null}
                          </button>
                          <div className="grid gap-3 md:grid-cols-3">
                            <button
                              type="button"
                              onClick={() => focusTabSection("validation", "validation_inspector")}
                              className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                            >
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Outcome
                              </p>
                              <p className="mt-1 text-sm font-medium text-text">
                                {selectedValidationSummary?.outcome || "Unknown"}
                              </p>
                            </button>
                            <button
                              type="button"
                              onClick={() => focusTabSection("validation", "validation_artifacts")}
                              className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                            >
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
                            </button>
                            <button
                              type="button"
                              onClick={() => focusTabSection("validation", "validation_tasks")}
                              className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                            >
                              <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                                Checks
                              </p>
                              <p className="mt-1 text-sm font-medium text-text">
                                {selectedValidationSummary?.validationsAttempted ?? 0}
                              </p>
                            </button>
                          </div>
                          {renderArtifactPreviewContent("No validation preview available.")}
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
                        <button
                          type="button"
                          onClick={() => {
                            setMemoryHitFilter("all");
                            focusTabSection("memory", "memory_hits");
                          }}
                          className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                        >
                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                            Hit count
                          </p>
                          <p className="mt-1 text-lg font-semibold text-text">
                            {memoryHits.length}
                          </p>
                        </button>
                        <button
                          type="button"
                          onClick={() => {
                            setMemoryHitFilter("scored");
                            focusTabSection("memory", "memory_hits");
                          }}
                          className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                        >
                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                            Scored hits
                          </p>
                          <p className="mt-1 text-lg font-semibold text-text">
                            {memoryHits.filter((hit) => typeof hit.score === "number").length}
                          </p>
                        </button>
                      </div>
                      {memoryHits.length === 0 ? (
                        <p className="text-sm text-text-muted">No memory hits returned.</p>
                      ) : (
                        <div ref={memoryHitsRef} className="space-y-4">
                          <div className="flex flex-wrap gap-2">
                            {[
                              ["all", "All hits"],
                              ["scored", "Scored only"],
                            ].map(([key, label]) => (
                              <button
                                key={key}
                                type="button"
                                onClick={() => setMemoryHitFilter(key as "all" | "scored")}
                                className={cn(
                                  "rounded-full border px-3 py-1.5 text-xs font-medium transition-colors",
                                  memoryHitFilter === key
                                    ? "border-primary/40 bg-primary/10 text-primary"
                                    : "border-border bg-surface text-text-muted hover:text-text"
                                )}
                              >
                                {label}
                              </button>
                            ))}
                          </div>
                          {filteredMemoryHits.length === 0 ? (
                            <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-dashed border-border bg-surface-elevated/30 p-3">
                              <p className="text-sm text-text-muted">
                                No memory hits match the current filter.
                              </p>
                              <Button variant="secondary" size="sm" onClick={resetMemoryFilters}>
                                Reset filters
                              </Button>
                            </div>
                          ) : null}
                          {filteredMemoryHits.map((hit, index) => {
                            const hitId =
                              pickText(hit.candidate_id) ||
                              pickText(hit.memory_id) ||
                              `hit-${index + 1}`;
                            return (
                              <div
                                key={String(hit.candidate_id ?? hit.memory_id ?? index)}
                                className="rounded-3xl border border-border bg-surface-elevated/40 p-4"
                              >
                                <div className="flex items-center justify-between gap-3">
                                  <div>
                                    <p className="text-sm font-medium text-text">
                                      {memoryKindLabel(hit.kind ?? hit.source ?? "memory_hit")}
                                    </p>
                                    <button
                                      type="button"
                                      onClick={() => void copyMemoryValue(hitId)}
                                      className="mt-1 text-[11px] text-text-muted transition-colors hover:text-text"
                                    >
                                      {copiedMemoryValue === hitId ? "Copied" : hitId}
                                    </button>
                                  </div>
                                  {typeof hit.score === "number" ? (
                                    <button
                                      type="button"
                                      onClick={() => setMemoryHitFilter("scored")}
                                      className="rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2.5 py-1 text-[10px] uppercase tracking-[0.18em] text-emerald-200 transition-colors hover:bg-emerald-500/15"
                                    >
                                      score {hit.score.toFixed(2)}
                                    </button>
                                  ) : null}
                                </div>
                                <div className="mt-3 flex flex-wrap gap-2">
                                  {pickText(hit.kind) ? (
                                    <button
                                      type="button"
                                      onClick={() => void copyMemoryValue(pickText(hit.kind))}
                                      className="rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface hover:text-text"
                                    >
                                      {copiedMemoryValue === pickText(hit.kind)
                                        ? "Copied"
                                        : pickText(hit.kind)}
                                    </button>
                                  ) : null}
                                  {pickText(hit.source) ? (
                                    <button
                                      type="button"
                                      onClick={() => void copyMemoryValue(pickText(hit.source))}
                                      className="rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface hover:text-text"
                                    >
                                      {copiedMemoryValue === pickText(hit.source)
                                        ? "Copied"
                                        : pickText(hit.source)}
                                    </button>
                                  ) : null}
                                  {pickText(hit.memory_id) ? (
                                    <button
                                      type="button"
                                      onClick={() =>
                                        void copyMemoryValue(`memory ${pickText(hit.memory_id)}`)
                                      }
                                      className="rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface hover:text-text"
                                    >
                                      {copiedMemoryValue === `memory ${pickText(hit.memory_id)}`
                                        ? "Copied"
                                        : `memory ${pickText(hit.memory_id)}`}
                                    </button>
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
                            );
                          })}
                        </div>
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
                        <button
                          type="button"
                          onClick={() => {
                            setMemoryCandidateFilter("all");
                            focusTabSection("memory", "memory_candidates");
                          }}
                          className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                        >
                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                            Candidate count
                          </p>
                          <p className="mt-1 text-lg font-semibold text-text">
                            {memoryCandidates.length}
                          </p>
                        </button>
                        <button
                          type="button"
                          onClick={() => {
                            setMemoryCandidateFilter("artifact_backed");
                            focusTabSection("memory", "memory_candidates");
                          }}
                          className="rounded-2xl border border-border bg-surface-elevated/40 p-3 text-left transition-colors hover:bg-surface-elevated"
                        >
                          <p className="text-[11px] uppercase tracking-[0.2em] text-text-muted">
                            Linked artifacts
                          </p>
                          <p className="mt-1 text-lg font-semibold text-text">
                            {memoryCandidates.filter((candidate) => candidate.artifact).length}
                          </p>
                        </button>
                      </div>
                      {memoryCandidates.length === 0 ? (
                        <p className="text-sm text-text-muted">No memory candidates recorded.</p>
                      ) : (
                        <div ref={memoryCandidatesRef} className="space-y-4">
                          <div className="flex flex-wrap gap-2">
                            {[
                              ["all", "All candidates"],
                              ["artifact_backed", "Artifact-backed"],
                            ].map(([key, label]) => (
                              <button
                                key={key}
                                type="button"
                                onClick={() =>
                                  setMemoryCandidateFilter(key as "all" | "artifact_backed")
                                }
                                className={cn(
                                  "rounded-full border px-3 py-1.5 text-xs font-medium transition-colors",
                                  memoryCandidateFilter === key
                                    ? "border-primary/40 bg-primary/10 text-primary"
                                    : "border-border bg-surface text-text-muted hover:text-text"
                                )}
                              >
                                {label}
                              </button>
                            ))}
                          </div>
                          {filteredMemoryCandidates.length === 0 ? (
                            <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-dashed border-border bg-surface-elevated/30 p-3">
                              <p className="text-sm text-text-muted">
                                No memory candidates match the current filter.
                              </p>
                              <Button variant="secondary" size="sm" onClick={resetMemoryFilters}>
                                Reset filters
                              </Button>
                            </div>
                          ) : null}
                          {filteredMemoryCandidates.map((candidate) => (
                            <div
                              key={candidate.candidate_id}
                              className="rounded-3xl border border-border bg-surface-elevated/40 p-4"
                            >
                              <div className="flex items-center justify-between gap-3">
                                <div>
                                  <p className="text-sm font-medium text-text">{candidate.kind}</p>
                                  <button
                                    type="button"
                                    onClick={() => void copyMemoryValue(candidate.candidate_id)}
                                    className="mt-1 text-[11px] text-text-muted transition-colors hover:text-text"
                                  >
                                    {copiedMemoryValue === candidate.candidate_id
                                      ? "Copied"
                                      : candidate.candidate_id}
                                  </button>
                                </div>
                                <span className="text-xs text-text-muted">
                                  {formatTimestamp(candidate.created_at_ms)}
                                </span>
                              </div>
                              <div className="mt-3 flex flex-wrap gap-2">
                                <button
                                  type="button"
                                  onClick={() => void copyMemoryValue(candidate.kind)}
                                  className="rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-emerald-200 transition-colors hover:bg-emerald-500/15"
                                >
                                  {copiedMemoryValue === candidate.kind ? "Copied" : candidate.kind}
                                </button>
                                {candidate.artifact?.artifact_type ? (
                                  <button
                                    type="button"
                                    onClick={() =>
                                      candidate.artifact && openArtifactRecord(candidate.artifact)
                                    }
                                    className="rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-text-muted transition-colors hover:bg-surface hover:text-text"
                                  >
                                    {candidate.artifact.artifact_type}
                                  </button>
                                ) : null}
                              </div>
                              {candidate.artifact ? (
                                <button
                                  type="button"
                                  onClick={() =>
                                    candidate.artifact && openArtifactRecord(candidate.artifact)
                                  }
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
                          ))}
                        </div>
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
