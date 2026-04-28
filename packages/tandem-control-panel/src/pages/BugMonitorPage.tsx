import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { ConfirmDialog } from "../components/ControlPanelDialogs";
import { AnimatedPage, Badge, DetailDrawer, EmptyState, PanelCard } from "../ui/index.tsx";
import type { AppPageProps } from "./pageTypes";

const LIST_PAGE_SIZE_OPTIONS = [10, 25, 50, 100];
const DEFAULT_LIST_PAGE_SIZE = 10;
const LIST_FETCH_LIMIT = 200;

function clampPage(page: number, totalPages: number) {
  if (!Number.isFinite(page) || page < 1) return 1;
  if (!Number.isFinite(totalPages) || totalPages < 1) return 1;
  return Math.min(page, totalPages);
}

function formatPageWindow(page: number, pageSize: number, total: number) {
  if (!total) return "0 of 0";
  const safePage = clampPage(page, Math.max(1, Math.ceil(total / Math.max(1, pageSize))));
  const safeSize = Math.max(1, pageSize);
  const start = (safePage - 1) * safeSize + 1;
  const end = Math.min(total, safePage * safeSize);
  return `${start}-${end} of ${total}`;
}

type AnyRecord = Record<string, any>;

type ReporterForm = {
  title: string;
  body: string;
  severity: string;
  source: string;
  labels: string;
  confidence: string;
  riskLevel: string;
  expectedDestination: string;
  evidenceRefs: string;
  relatedRunId: string;
  relatedWorkflowId: string;
  relatedFile: string;
  reproductionNotes: string;
  expectedBehavior: string;
  actualBehavior: string;
};

const emptyReporterForm: ReporterForm = {
  title: "",
  body: "",
  severity: "error",
  source: "manual",
  labels: "bug-monitor, runtime-failure",
  confidence: "medium",
  riskLevel: "medium",
  expectedDestination: "bug_monitor_issue_draft",
  evidenceRefs: "",
  relatedRunId: "",
  relatedWorkflowId: "",
  relatedFile: "",
  reproductionNotes: "",
  expectedBehavior: "",
  actualBehavior: "",
};

function asRecord(value: unknown): AnyRecord {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as AnyRecord) : {};
}

function asArray(value: unknown): AnyRecord[] {
  return Array.isArray(value) ? value.filter((item) => item && typeof item === "object") : [];
}

function unwrapStatus(payload: unknown): AnyRecord {
  const root = asRecord(payload);
  return asRecord(root.status || root);
}

function firstString(record: AnyRecord, keys: string[], fallback = ""): string {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value.trim();
    if (typeof value === "number" && Number.isFinite(value)) return String(value);
  }
  return fallback;
}

function nestedRecord(record: AnyRecord, key: string): AnyRecord {
  return asRecord(record[key]);
}

function firstNumber(record: AnyRecord, keys: string[]): number | null {
  for (const key of keys) {
    const value = Number(record[key]);
    if (Number.isFinite(value) && value > 0) return value;
  }
  return null;
}

function formatTime(value: unknown): string {
  const numeric = Number(value);
  if (Number.isFinite(numeric) && numeric > 0) {
    return new Date(numeric).toLocaleString();
  }
  if (typeof value === "string" && value.trim()) {
    const parsed = Date.parse(value);
    if (Number.isFinite(parsed)) return new Date(parsed).toLocaleString();
    return value;
  }
  return "Unavailable";
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function previewText(value: unknown, fallback = "No body yet."): string {
  const text = String(value || "").trim();
  if (!text) return fallback;
  return text.length > 420 ? `${text.slice(0, 420)}...` : text;
}

function statusTone(status: unknown): "ok" | "warn" | "err" | "info" | "ghost" {
  const normalized = String(status || "").toLowerCase();
  if (["posted", "published", "ok", "ready", "draft_ready", "open"].includes(normalized)) {
    return "ok";
  }
  if (["denied", "failed", "error", "blocked"].some((term) => normalized.includes(term))) {
    return "err";
  }
  if (["approval", "pending", "queued", "paused"].some((term) => normalized.includes(term))) {
    return "warn";
  }
  return normalized ? "info" : "ghost";
}

function boolLabel(value: unknown): string {
  return value ? "Yes" : "No";
}

function joinLabels(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function splitLines(value: string): string[] {
  return value
    .split(/\r?\n|,/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function detailContains(record: AnyRecord, pattern: RegExp): boolean {
  return pattern.test(String(record.detail || ""));
}

function valuesFromUnknown(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value
      .map((item) =>
        typeof item === "string" || typeof item === "number" ? String(item).trim() : ""
      )
      .filter(Boolean);
  }
  if (typeof value === "string" && value.trim()) return [value.trim()];
  if (typeof value === "number" && Number.isFinite(value)) return [String(value)];
  return [];
}

function collectValues(record: AnyRecord, keys: string[]): string[] {
  const payload = nestedRecord(record, "event_payload");
  const values: string[] = [];
  for (const key of keys) {
    values.push(...valuesFromUnknown(record[key]));
    values.push(...valuesFromUnknown(payload[key]));
  }
  return Array.from(new Set(values));
}

function shortValue(value: unknown, maxLength = 120): string {
  const text = String(value || "").trim();
  if (!text) return "";
  return text.length > maxLength ? `${text.slice(0, maxLength - 3)}...` : text;
}

function metadataValue(record: AnyRecord, keys: string[], detailPattern?: RegExp): string {
  const payload = nestedRecord(record, "event_payload");
  return (
    firstString(record, keys) ||
    firstString(payload, keys) ||
    (detailPattern && detailContains(record, detailPattern) ? "recorded in detail" : "")
  );
}

function linkHref(value: string): string | undefined {
  return /^https?:\/\//i.test(value) ? value : undefined;
}

function evidenceValues(record: AnyRecord): string[] {
  const evidence = [
    ...collectValues(record, [
      "evidence_refs",
      "artifact_refs",
      "artifactRefs",
      "files_touched",
      "filesTouched",
    ]),
    ...valuesFromUnknown(record.evidence_digest),
    ...valuesFromUnknown(record.response_excerpt),
    ...valuesFromUnknown(record.excerpt),
  ];
  if (
    detailContains(
      record,
      /(^|\n)(artifact_refs|evidence_refs|files_touched|tool_result_excerpt|excerpt):/i
    )
  ) {
    evidence.push("structured evidence in detail");
  }
  return Array.from(new Set(evidence)).filter(Boolean);
}

function lifecycleArtifactValues(record: AnyRecord): string[] {
  const artifacts = [
    ...collectValues(record, [
      "artifact_refs",
      "artifactRefs",
      "evidence_refs",
      "triage_artifacts",
      "research_sources",
      "related_failure_patterns",
    ]),
    ...valuesFromUnknown(record.triage_summary_artifact),
    ...valuesFromUnknown(record.issue_draft_artifact),
    ...valuesFromUnknown(record.duplicate_matches_artifact),
  ];
  for (const key of [
    "triage_summary_artifact",
    "issue_draft_artifact",
    "duplicate_matches_artifact",
  ]) {
    const artifact = asRecord(record[key]);
    artifacts.push(...valuesFromUnknown(artifact.path));
    artifacts.push(...valuesFromUnknown(artifact.artifact_ref));
    artifacts.push(...valuesFromUnknown(artifact.id));
  }
  if (
    detailContains(
      record,
      /(^|\n)(artifact_refs|evidence_refs|research_sources|related_failure_patterns):/i
    )
  ) {
    artifacts.push("structured refs in detail");
  }
  return Array.from(new Set(artifacts)).filter(Boolean);
}

function memoryValues(record: AnyRecord): string[] {
  const values = [
    ...collectValues(record, [
      "memory_id",
      "memory_ids",
      "memory_ref",
      "memory_refs",
      "memory_pattern",
      "memory_patterns",
      "failure_pattern_memory",
      "regression_signal_memory",
      "related_failure_patterns",
    ]),
  ];
  if (
    detailContains(
      record,
      /(^|\n)(memory|failure_pattern|regression_signal|related_failure_patterns):/i
    )
  ) {
    values.push("memory references in detail");
  }
  return Array.from(new Set(values)).filter(Boolean);
}

type LifecycleItem = {
  label: string;
  value: string;
  tone?: "ok" | "warn" | "err" | "info" | "ghost";
  href?: string;
};

function SignalLifecyclePanel({
  record,
  kind,
}: {
  record: AnyRecord;
  kind: "incident" | "draft" | "post";
}) {
  const artifacts = lifecycleArtifactValues(record);
  const memory = memoryValues(record);
  const issueUrl = firstString(record, ["github_issue_url", "issue_url"]);
  const commentUrl = firstString(record, ["github_comment_url", "comment_url"]);
  const publishedUrl = issueUrl || commentUrl;
  const coderReadyGate = asRecord(record.coder_ready_gate);
  const qualityGate = asRecord(record.quality_gate);
  const proposalStatus =
    firstString(record, ["github_status", "proposal_status", "issue_draft_status"]) ||
    (record.issue_body || record.issue_draft_artifact ? "issue draft available" : "");
  const approvalState =
    firstString(record, ["approval_status", "approval_decision", "status"]) ||
    (kind === "draft" ? "awaiting review" : "");
  const intakeId = firstString(record, ["incident_id", "post_id", "id", "fingerprint"]);
  const items: LifecycleItem[] = [
    {
      label: "Signal",
      value: shortValue(
        intakeId || metadataValue(record, ["event_type", "source"], /(^|\n)(event|source):/i)
      ),
      tone: qualityGate.status ? statusTone(qualityGate.status) : "info",
    },
    {
      label: "Draft",
      value: shortValue(firstString(record, ["draft_id"]) || (kind === "draft" ? intakeId : "")),
      tone: firstString(record, ["draft_id"]) || kind === "draft" ? "info" : "ghost",
    },
    {
      label: "Triage",
      value: shortValue(
        firstString(record, ["triage_run_id", "triageRunId", "triage_context_run_id"])
      ),
      tone: firstString(record, ["triage_run_id", "triageRunId", "triage_context_run_id"])
        ? "ok"
        : "ghost",
    },
    {
      label: "Proposal",
      value: shortValue(proposalStatus),
      tone: proposalStatus ? statusTone(proposalStatus) : "ghost",
    },
    {
      label: "Coder-ready",
      value: shortValue(
        firstString(coderReadyGate, ["status"]) || (record.coder_ready === true ? "passed" : "")
      ),
      tone:
        record.coder_ready === true || coderReadyGate.status === "passed"
          ? "ok"
          : coderReadyGate.status
            ? "warn"
            : "ghost",
    },
    {
      label: "Approval",
      value: shortValue(approvalState),
      tone: statusTone(approvalState),
    },
    {
      label: "Published",
      value: shortValue(
        publishedUrl || firstString(record, ["issue_number", "comment_id", "github_post_id"])
      ),
      tone: publishedUrl || record.issue_number ? "ok" : "ghost",
      href: publishedUrl ? linkHref(publishedUrl) : undefined,
    },
    {
      label: "Artifacts",
      value: shortValue(artifacts.slice(0, 3).join(", ")),
      tone: artifacts.length ? "info" : "ghost",
    },
    {
      label: "Memory",
      value: shortValue(memory.slice(0, 3).join(", ")),
      tone: memory.length ? "info" : "ghost",
    },
  ].filter((item) => item.value);

  if (!items.length) return null;

  return (
    <div className="mt-3 rounded-md border border-sky-400/20 bg-sky-400/[0.04] p-3">
      <div className="tcp-subtle mb-2 text-xs uppercase tracking-[0.18em]">Signal lifecycle</div>
      <div className="grid gap-2 md:grid-cols-3">
        {items.map((item) => (
          <div key={item.label} className="min-w-0 rounded border border-white/10 bg-black/10 p-2">
            <div className="mb-1 flex items-center justify-between gap-2">
              <span className="tcp-subtle text-[0.68rem] uppercase tracking-[0.14em]">
                {item.label}
              </span>
              <Badge tone={item.tone || "info"}>
                {item.tone === "ghost" ? "none" : item.tone || "info"}
              </Badge>
            </div>
            {item.href ? (
              <a
                className="break-words text-xs text-sky-200 hover:text-sky-100"
                href={item.href}
                target="_blank"
                rel="noreferrer"
              >
                {item.value}
              </a>
            ) : (
              <div className="break-words text-xs text-white/80">{item.value}</div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function signalQualityGates(record: AnyRecord): { label: string; ok: boolean }[] {
  const backendGate = asRecord(record.quality_gate);
  const backendGates = Array.isArray(backendGate.gates) ? backendGate.gates : [];
  if (backendGates.length) {
    return backendGates.map((gate, index) => {
      const gateRecord = asRecord(gate);
      return {
        label: firstString(gateRecord, ["label", "key"], `Gate ${index + 1}`),
        ok: Boolean(gateRecord.passed ?? gateRecord.ok),
      };
    });
  }
  const eventPayload = nestedRecord(record, "event_payload");
  const status = String(record.status || "").toLowerCase();
  const eventType = metadataValue(
    record,
    ["event_type", "event", "signal_type", "failure_type", "error_kind"],
    /(^|\n)(event|event_type|failure_type|error_kind):/i
  );
  const confidence = metadataValue(
    record,
    ["confidence", "signal_confidence", "root_cause_confidence"],
    /(^|\n)confidence:/i
  );
  const risk = metadataValue(record, ["risk_level", "risk"], /(^|\n)risk_level:/i);
  const destination = metadataValue(
    record,
    [
      "expected_destination",
      "github_status",
      "github_issue_url",
      "github_comment_url",
      "issue_url",
      "comment_url",
    ],
    /(^|\n)expected_destination:/i
  );
  const hasEvidence = evidenceValues(record).length > 0;
  const source = metadataValue(
    record,
    ["source", "component", "event_type"],
    /(^|\n)(source|component|event_type):/i
  );
  const hasDedupe =
    !!record.fingerprint || !!record.duplicate_summary || Array.isArray(record.duplicate_matches);
  const routineNoise =
    /progress|heartbeat|started|retrying|attempt\.started|minor_retry/.test(eventType) ||
    /progress|retrying/.test(status);
  const terminalOrActionable = eventType
    ? !routineNoise
    : /(failed|error|blocked|queued|draft|approval|posted|created|duplicate_suppressed)/.test(
        status
      );
  return [
    { label: "Source known", ok: !!source },
    { label: "Type classified", ok: !!eventType },
    { label: "Confidence recorded", ok: !!confidence },
    { label: "Dedupe/fingerprint checked", ok: hasDedupe },
    { label: "Evidence or artifact refs", ok: hasEvidence },
    { label: "Destination clear", ok: !!destination },
    { label: "Risk known", ok: !!risk },
    { label: "Not routine noise", ok: terminalOrActionable && !routineNoise },
  ];
}

function SignalMetadataGrid({ record }: { record: AnyRecord }) {
  const evidence = evidenceValues(record);
  const qualityGate = asRecord(record.quality_gate);
  const duplicateCount = Array.isArray(record.duplicate_matches)
    ? record.duplicate_matches.length
    : 0;
  const rows = [
    [
      "Gate",
      qualityGate.status
        ? `${String(qualityGate.status)} (${Number(qualityGate.passed_count || 0)}/${Number(qualityGate.total_count || 0)})`
        : "",
    ],
    [
      "Source",
      metadataValue(
        record,
        ["source", "component", "event_type"],
        /(^|\n)(source|component|event_type):/i
      ),
    ],
    [
      "Type",
      metadataValue(
        record,
        ["event_type", "event", "signal_type", "failure_type", "error_kind"],
        /(^|\n)(event|event_type|failure_type|error_kind):/i
      ),
    ],
    [
      "Confidence",
      metadataValue(
        record,
        ["confidence", "signal_confidence", "root_cause_confidence"],
        /(^|\n)confidence:/i
      ),
    ],
    ["Risk", metadataValue(record, ["risk_level", "risk"], /(^|\n)risk_level:/i)],
    [
      "Destination",
      metadataValue(
        record,
        [
          "expected_destination",
          "github_status",
          "github_issue_url",
          "github_comment_url",
          "issue_url",
          "comment_url",
        ],
        /(^|\n)expected_destination:/i
      ),
    ],
    ["Fingerprint", firstString(record, ["fingerprint"])],
    ["Run", firstString(record, ["run_id", "runID"])],
    ["Session", firstString(record, ["session_id", "sessionID"])],
    ["Correlation", firstString(record, ["correlation_id", "correlationID"])],
    ["Evidence", evidence.slice(0, 3).join(", ")],
    [
      "Duplicates",
      duplicateCount
        ? `${duplicateCount} match${duplicateCount === 1 ? "" : "es"}`
        : record.duplicate_summary
          ? "summary available"
          : "",
    ],
  ].filter(([, value]) => value);

  if (!rows.length) return null;

  return (
    <div className="tcp-subtle mt-3 grid gap-1 rounded-md border border-white/10 bg-white/[0.03] p-3 text-xs md:grid-cols-2">
      {rows.map(([label, value]) => (
        <div key={label} className="min-w-0">
          <span className="text-white/60">{label}: </span>
          <span className="break-words text-white/80">{value}</span>
        </div>
      ))}
    </div>
  );
}

function QualityGateStrip({ record }: { record: AnyRecord }) {
  const gates = signalQualityGates(record);
  const backendGate = asRecord(record.quality_gate);
  const isBackendGate = Array.isArray(backendGate.gates) && backendGate.gates.length > 0;
  const passed = gates.filter((gate) => gate.ok).length;
  return (
    <div className="mt-3 border-t border-white/10 pt-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div className="tcp-subtle text-xs uppercase tracking-[0.18em]">
          Signal quality gates{isBackendGate ? "" : " (heuristic)"}
        </div>
        <Badge tone={passed === gates.length ? "ok" : passed >= 5 ? "warn" : "err"}>
          {passed}/{gates.length}
        </Badge>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {gates.map((gate) => (
          <Badge key={gate.label} tone={gate.ok ? "ok" : "ghost"}>
            {gate.ok ? "ok" : "missing"} {gate.label}
          </Badge>
        ))}
      </div>
    </div>
  );
}

function buildReportPayload(form: ReporterForm): AnyRecord {
  const detailParts = [
    form.body.trim(),
    form.reproductionNotes.trim() ? `Reproduction notes:\n${form.reproductionNotes.trim()}` : "",
    form.expectedBehavior.trim() ? `Expected behavior:\n${form.expectedBehavior.trim()}` : "",
    form.actualBehavior.trim() ? `Actual behavior:\n${form.actualBehavior.trim()}` : "",
  ].filter(Boolean);
  const excerpt = detailParts.flatMap((part) =>
    part
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .slice(0, 8)
  );
  return {
    title: form.title.trim(),
    detail: detailParts.join("\n\n"),
    source: form.source.trim() || "manual",
    level: form.severity.trim() || "error",
    confidence: form.confidence.trim() || undefined,
    risk_level: form.riskLevel.trim() || undefined,
    expected_destination: form.expectedDestination.trim() || undefined,
    evidence_refs: splitLines(form.evidenceRefs),
    run_id: form.relatedRunId.trim() || undefined,
    file_name: form.relatedFile.trim() || undefined,
    component: form.relatedWorkflowId.trim() || undefined,
    event: form.relatedWorkflowId.trim() ? "workflow_failure" : "manual_report",
    excerpt,
    labels: joinLabels(form.labels),
    related_workflow_id: form.relatedWorkflowId.trim() || undefined,
    reproduction_notes: form.reproductionNotes.trim() || undefined,
    expected_behavior: form.expectedBehavior.trim() || undefined,
    actual_behavior: form.actualBehavior.trim() || undefined,
  };
}

function StatTile({ label, value, tone = "info" }: { label: string; value: any; tone?: any }) {
  return (
    <div className="tcp-list-item min-h-[5rem]">
      <div className="tcp-subtle text-xs uppercase tracking-[0.18em]">{label}</div>
      <div className="mt-2 flex items-center justify-between gap-2">
        <div className="min-w-0 truncate text-lg font-semibold">{value}</div>
        <Badge tone={tone}>{String(value || "n/a")}</Badge>
      </div>
    </div>
  );
}

function QueryError({ error }: { error: unknown }) {
  const message = error instanceof Error ? error.message : String(error || "Unknown error");
  return (
    <div className="tcp-list-item border-rose-500/40">
      <div className="font-medium text-rose-200">Request failed</div>
      <div className="tcp-subtle mt-1 break-words text-xs">{message}</div>
    </div>
  );
}

export function BugMonitorPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [form, setForm] = useState<ReporterForm>(emptyReporterForm);
  const [detail, setDetail] = useState<{ title: string; value: unknown } | null>(null);
  const [reportOpen, setReportOpen] = useState(false);

  const [incidentPage, setIncidentPage] = useState(1);
  const [incidentPageSize, setIncidentPageSize] = useState(DEFAULT_LIST_PAGE_SIZE);
  const [selectedIncidentIds, setSelectedIncidentIds] = useState<string[]>([]);
  const [deleteIncidentsConfirm, setDeleteIncidentsConfirm] = useState<{
    ids: string[];
    all: boolean;
  } | null>(null);

  const [draftPage, setDraftPage] = useState(1);
  const [draftPageSize, setDraftPageSize] = useState(DEFAULT_LIST_PAGE_SIZE);
  const [selectedDraftIds, setSelectedDraftIds] = useState<string[]>([]);
  const [deleteDraftsConfirm, setDeleteDraftsConfirm] = useState<{
    ids: string[];
    all: boolean;
  } | null>(null);

  const [postPage, setPostPage] = useState(1);
  const [postPageSize, setPostPageSize] = useState(DEFAULT_LIST_PAGE_SIZE);
  const [selectedPostIds, setSelectedPostIds] = useState<string[]>([]);
  const [deletePostsConfirm, setDeletePostsConfirm] = useState<{
    ids: string[];
    all: boolean;
  } | null>(null);

  const invalidateMonitorQueries = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["bug-monitor", "status"] }),
      queryClient.invalidateQueries({ queryKey: ["bug-monitor", "incidents"] }),
      queryClient.invalidateQueries({ queryKey: ["bug-monitor", "drafts"] }),
      queryClient.invalidateQueries({ queryKey: ["bug-monitor", "posts"] }),
    ]);
  };

  const statusQuery = useQuery({
    queryKey: ["bug-monitor", "status"],
    queryFn: () => client.bugMonitor.getStatus(),
    refetchInterval: 10000,
  });

  const incidentsQuery = useQuery({
    queryKey: ["bug-monitor", "incidents", LIST_FETCH_LIMIT],
    queryFn: () => client.bugMonitor.listIncidents({ limit: LIST_FETCH_LIMIT }),
  });

  const draftsQuery = useQuery({
    queryKey: ["bug-monitor", "drafts", LIST_FETCH_LIMIT],
    queryFn: () => client.bugMonitor.listDrafts({ limit: LIST_FETCH_LIMIT }),
  });

  const postsQuery = useQuery({
    queryKey: ["bug-monitor", "posts", LIST_FETCH_LIMIT],
    queryFn: () => client.bugMonitor.listPosts({ limit: LIST_FETCH_LIMIT }),
  });

  const statusMutation = useMutation({
    mutationFn: async (action: "recompute" | "pause" | "resume" | "debug") => {
      if (action === "recompute") return client.bugMonitor.recomputeStatus();
      if (action === "pause") return client.bugMonitor.pause();
      if (action === "resume") return client.bugMonitor.resume();
      return client.bugMonitor.debug();
    },
    onSuccess: async (result, action) => {
      if (action === "debug") {
        setDetail({ title: "Bug Monitor Debug Payload", value: result });
      }
      toast(
        "ok",
        action === "debug" ? "Bug Monitor debug payload loaded." : "Bug Monitor updated."
      );
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const incidentMutation = useMutation({
    mutationFn: async (vars: { action: "view" | "replay" | "triage"; incident: AnyRecord }) => {
      const incidentId = firstString(vars.incident, ["incident_id", "id"]);
      if (!incidentId) throw new Error("Incident id missing.");
      if (vars.action === "view") return client.bugMonitor.getIncident(incidentId);
      if (vars.action === "triage") {
        const draftId = firstString(vars.incident, ["draft_id"]);
        if (!draftId) throw new Error("Incident has no draft id for triage.");
        return client.bugMonitor.createTriageRun(draftId);
      }
      return client.bugMonitor.replayIncident(incidentId);
    },
    onSuccess: async (result, vars) => {
      if (vars.action === "view") {
        setDetail({
          title: `Incident ${firstString(vars.incident, ["incident_id", "id"], "detail")}`,
          value: result,
        });
      }
      toast("ok", vars.action === "view" ? "Incident loaded." : "Incident action queued.");
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const draftMutation = useMutation({
    mutationFn: async (vars: { action: string; draft: AnyRecord; reason?: string }) => {
      const draftId = firstString(vars.draft, ["draft_id", "id"]);
      if (!draftId) throw new Error("Draft id missing.");
      if (vars.action === "view") return client.bugMonitor.getDraft(draftId);
      if (vars.action === "approve") return client.bugMonitor.approveDraft(draftId, vars.reason);
      if (vars.action === "deny") return client.bugMonitor.denyDraft(draftId, vars.reason);
      if (vars.action === "triage-run") return client.bugMonitor.createTriageRun(draftId);
      if (vars.action === "triage-summary") {
        return client.bugMonitor.createTriageSummary(draftId, {
          suggested_title: firstString(vars.draft, ["title", "fingerprint"], "Runtime failure"),
          what_happened: firstString(vars.draft, ["detail", "evidence_digest"], ""),
          notes: vars.reason || "Created from the control panel.",
        });
      }
      if (vars.action === "issue-draft") return client.bugMonitor.createIssueDraft(draftId, {});
      if (vars.action === "publish") return client.bugMonitor.publishDraft(draftId, {});
      if (vars.action === "recheck") return client.bugMonitor.recheckMatch(draftId, {});
      throw new Error(`Unknown draft action: ${vars.action}`);
    },
    onSuccess: async (result, vars) => {
      if (vars.action === "view") {
        setDetail({
          title: `Draft ${firstString(vars.draft, ["draft_id", "id"], "detail")}`,
          value: result,
        });
      }
      toast("ok", vars.action === "view" ? "Draft loaded." : "Draft action completed.");
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const reportMutation = useMutation({
    mutationFn: async () => {
      const payload = buildReportPayload(form);
      if (!String(payload.title || "").trim()) throw new Error("Title is required.");
      if (!String(payload.detail || "").trim()) throw new Error("Summary/body is required.");
      return client.bugMonitor.report(payload);
    },
    onSuccess: async (result) => {
      setDetail({ title: "Manual Report Result", value: result });
      setForm(emptyReporterForm);
      toast("ok", "Manual report submitted.");
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const deleteIncidentsMutation = useMutation({
    mutationFn: async (vars: { ids: string[]; all: boolean }) => {
      if (vars.all) return client.bugMonitor.bulkDeleteIncidents({ all: true });
      return client.bugMonitor.bulkDeleteIncidents({ ids: vars.ids });
    },
    onSuccess: async (result, vars) => {
      const deleted = Number((result as any)?.deleted || vars.ids.length || 0);
      setSelectedIncidentIds([]);
      setDeleteIncidentsConfirm(null);
      toast("ok", `Deleted ${deleted} incident${deleted === 1 ? "" : "s"}.`);
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const deleteDraftsMutation = useMutation({
    mutationFn: async (vars: { ids: string[]; all: boolean }) => {
      if (vars.all) return client.bugMonitor.bulkDeleteDrafts({ all: true });
      return client.bugMonitor.bulkDeleteDrafts({ ids: vars.ids });
    },
    onSuccess: async (result, vars) => {
      const deleted = Number((result as any)?.deleted || vars.ids.length || 0);
      setSelectedDraftIds([]);
      setDeleteDraftsConfirm(null);
      toast("ok", `Deleted ${deleted} draft${deleted === 1 ? "" : "s"}.`);
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const deletePostsMutation = useMutation({
    mutationFn: async (vars: { ids: string[]; all: boolean }) => {
      if (vars.all) return client.bugMonitor.bulkDeletePosts({ all: true });
      return client.bugMonitor.bulkDeletePosts({ ids: vars.ids });
    },
    onSuccess: async (result, vars) => {
      const deleted = Number((result as any)?.deleted || vars.ids.length || 0);
      setSelectedPostIds([]);
      setDeletePostsConfirm(null);
      toast("ok", `Deleted ${deleted} post${deleted === 1 ? "" : "s"}.`);
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const status = unwrapStatus(statusQuery.data);
  const config = asRecord(status.config);
  const readiness = asRecord(status.readiness);
  const runtime = asRecord(status.runtime);
  const incidents = asArray(asRecord(incidentsQuery.data).incidents);
  const drafts = asArray(asRecord(draftsQuery.data).drafts);
  const posts = asArray(asRecord(postsQuery.data).posts);

  const incidentIdOf = (record: AnyRecord, index: number) =>
    firstString(record, ["incident_id", "id"], `incident-${index}`);
  const draftIdOf = (record: AnyRecord, index: number) =>
    firstString(record, ["draft_id", "id"], `draft-${index}`);
  const postIdOf = (record: AnyRecord, index: number) =>
    firstString(record, ["post_id", "id"], `post-${index}`);

  const incidentPageCount = Math.max(
    1,
    Math.ceil(incidents.length / Math.max(1, incidentPageSize))
  );
  const safeIncidentPage = clampPage(incidentPage, incidentPageCount);
  const incidentPageStart = (safeIncidentPage - 1) * Math.max(1, incidentPageSize);
  const pagedIncidents = incidents.slice(incidentPageStart, incidentPageStart + incidentPageSize);
  const incidentPageLabel = formatPageWindow(safeIncidentPage, incidentPageSize, incidents.length);

  const draftPageCount = Math.max(1, Math.ceil(drafts.length / Math.max(1, draftPageSize)));
  const safeDraftPage = clampPage(draftPage, draftPageCount);
  const draftPageStart = (safeDraftPage - 1) * Math.max(1, draftPageSize);
  const pagedDrafts = drafts.slice(draftPageStart, draftPageStart + draftPageSize);
  const draftPageLabel = formatPageWindow(safeDraftPage, draftPageSize, drafts.length);

  const postPageCount = Math.max(1, Math.ceil(posts.length / Math.max(1, postPageSize)));
  const safePostPage = clampPage(postPage, postPageCount);
  const postPageStart = (safePostPage - 1) * Math.max(1, postPageSize);
  const pagedPosts = posts.slice(postPageStart, postPageStart + postPageSize);
  const postPageLabel = formatPageWindow(safePostPage, postPageSize, posts.length);

  useEffect(() => {
    if (incidentPage !== safeIncidentPage) setIncidentPage(safeIncidentPage);
  }, [incidentPage, safeIncidentPage]);
  useEffect(() => {
    if (draftPage !== safeDraftPage) setDraftPage(safeDraftPage);
  }, [draftPage, safeDraftPage]);
  useEffect(() => {
    if (postPage !== safePostPage) setPostPage(safePostPage);
  }, [postPage, safePostPage]);

  useEffect(() => {
    const ids = new Set(incidents.map((row, i) => incidentIdOf(row, i)));
    setSelectedIncidentIds((prev) => prev.filter((id) => ids.has(id)));
  }, [incidents]);
  useEffect(() => {
    const ids = new Set(drafts.map((row, i) => draftIdOf(row, i)));
    setSelectedDraftIds((prev) => prev.filter((id) => ids.has(id)));
  }, [drafts]);
  useEffect(() => {
    const ids = new Set(posts.map((row, i) => postIdOf(row, i)));
    setSelectedPostIds((prev) => prev.filter((id) => ids.has(id)));
  }, [posts]);

  const selectedIncidentSet = useMemo(() => new Set(selectedIncidentIds), [selectedIncidentIds]);
  const selectedDraftSet = useMemo(() => new Set(selectedDraftIds), [selectedDraftIds]);
  const selectedPostSet = useMemo(() => new Set(selectedPostIds), [selectedPostIds]);

  const toggleIncidentSelection = (id: string) => {
    if (!id) return;
    setSelectedIncidentIds((prev) =>
      prev.includes(id) ? prev.filter((entry) => entry !== id) : [...prev, id]
    );
  };
  const toggleDraftSelection = (id: string) => {
    if (!id) return;
    setSelectedDraftIds((prev) =>
      prev.includes(id) ? prev.filter((entry) => entry !== id) : [...prev, id]
    );
  };
  const togglePostSelection = (id: string) => {
    if (!id) return;
    setSelectedPostIds((prev) =>
      prev.includes(id) ? prev.filter((entry) => entry !== id) : [...prev, id]
    );
  };

  const selectAllVisibleIncidents = () => {
    setSelectedIncidentIds((prev) => {
      const next = new Set(prev);
      pagedIncidents.forEach((row, i) => next.add(incidentIdOf(row, incidentPageStart + i)));
      return Array.from(next);
    });
  };
  const selectAllVisibleDrafts = () => {
    setSelectedDraftIds((prev) => {
      const next = new Set(prev);
      pagedDrafts.forEach((row, i) => next.add(draftIdOf(row, draftPageStart + i)));
      return Array.from(next);
    });
  };
  const selectAllVisiblePosts = () => {
    setSelectedPostIds((prev) => {
      const next = new Set(prev);
      pagedPosts.forEach((row, i) => next.add(postIdOf(row, postPageStart + i)));
      return Array.from(next);
    });
  };

  useEffect(() => {
    if (rootRef.current) renderIcons(rootRef.current);
  }, [
    incidents.length,
    drafts.length,
    posts.length,
    selectedIncidentIds.length,
    selectedDraftIds.length,
    selectedPostIds.length,
    safeIncidentPage,
    safeDraftPage,
    safePostPage,
    incidentPageSize,
    draftPageSize,
    postPageSize,
    reportOpen,
    !!detail,
  ]);
  const enabled = !!config.enabled;
  const paused = !!runtime.paused || !!config.paused;
  const ingestReady = !!readiness.ingest_ready;
  const publishReady = !!readiness.publish_ready;
  const monitoringActive = !!runtime.monitoring_active;
  const pendingIncidents = Number(runtime.pending_incidents || 0);
  const lastError =
    firstString(runtime, ["last_runtime_error"]) || firstString(status, ["last_error"]);
  const missingCapabilities = Array.isArray(status.missing_required_capabilities)
    ? status.missing_required_capabilities
    : [];

  const readinessLabel = useMemo(() => {
    if (!enabled) return "Disabled";
    if (paused) return "Paused";
    if (!ingestReady) return "Blocked";
    if (monitoringActive && publishReady) return "Monitoring";
    if (monitoringActive) return "Watching locally";
    return "Ready";
  }, [enabled, ingestReady, monitoringActive, paused, publishReady]);

  return (
    <AnimatedPage className="grid h-full min-h-0 gap-4">
      <div ref={rootRef} className="grid h-full min-h-0 gap-4">
        <PanelCard
          title="Bug Monitor"
          subtitle="Runtime failures, workflow failures, draft issues, and GitHub publishing."
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge tone={enabled && ingestReady ? "ok" : enabled ? "warn" : "ghost"}>
                {readinessLabel}
              </Badge>
              <button
                type="button"
                className="tcp-btn-primary h-8 px-3 text-xs"
                onClick={() => setReportOpen(true)}
              >
                <i data-lucide="file-plus"></i>
                Report issue
              </button>
              <button
                type="button"
                className="tcp-btn h-8 px-3 text-xs"
                disabled={
                  statusQuery.isFetching ||
                  incidentsQuery.isFetching ||
                  draftsQuery.isFetching ||
                  postsQuery.isFetching
                }
                onClick={() => {
                  void Promise.all([
                    statusQuery.refetch(),
                    incidentsQuery.refetch(),
                    draftsQuery.refetch(),
                    postsQuery.refetch(),
                  ]).then(() => toast("ok", "Bug Monitor refreshed."));
                }}
              >
                <i data-lucide="refresh-cw"></i>
                Refresh
              </button>
              <button
                type="button"
                className="tcp-btn h-8 px-3 text-xs"
                disabled={statusMutation.isPending}
                onClick={() => statusMutation.mutate("recompute")}
              >
                <i data-lucide="rotate-cw"></i>
                Recompute
              </button>
              <button
                type="button"
                className="tcp-btn h-8 px-3 text-xs"
                disabled={statusMutation.isPending}
                onClick={() => statusMutation.mutate(paused ? "resume" : "pause")}
              >
                <i data-lucide={paused ? "play" : "pause"}></i>
                {paused ? "Resume" : "Pause"}
              </button>
              <button
                type="button"
                className="tcp-btn h-8 px-3 text-xs"
                disabled={statusMutation.isPending}
                onClick={() => statusMutation.mutate("debug")}
              >
                <i data-lucide="bug-play"></i>
                Debug
              </button>
            </div>
          }
        >
          <div className="grid gap-4">
            {statusQuery.isError ? <QueryError error={statusQuery.error} /> : null}
            {!enabled || !ingestReady || lastError ? (
              <div className="tcp-list-item">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge tone={!enabled || !ingestReady ? "warn" : "err"}>
                    {!enabled ? "Disabled" : !ingestReady ? "Readiness blocked" : "Runtime error"}
                  </Badge>
                  {missingCapabilities.length ? (
                    <span className="tcp-subtle text-xs">
                      Missing: {missingCapabilities.join(", ")}
                    </span>
                  ) : null}
                </div>
                {lastError ? (
                  <div className="tcp-subtle mt-2 break-words text-xs">{lastError}</div>
                ) : null}
              </div>
            ) : null}
            <div className="grid gap-3 md:grid-cols-3 xl:grid-cols-6">
              <StatTile
                label="enabled"
                value={boolLabel(enabled)}
                tone={enabled ? "ok" : "ghost"}
              />
              <StatTile
                label="monitoring_active"
                value={boolLabel(monitoringActive)}
                tone={monitoringActive ? "ok" : "info"}
              />
              <StatTile label="paused" value={boolLabel(paused)} tone={paused ? "warn" : "ok"} />
              <StatTile
                label="pending_incidents"
                value={pendingIncidents}
                tone={pendingIncidents ? "warn" : "ok"}
              />
              <StatTile
                label="ingest_ready"
                value={boolLabel(ingestReady)}
                tone={ingestReady ? "ok" : "warn"}
              />
              <StatTile
                label="publish_ready"
                value={boolLabel(publishReady)}
                tone={publishReady ? "ok" : "info"}
              />
            </div>
          </div>
        </PanelCard>

        <PanelCard
          title="Incidents"
          subtitle={`${incidents.length} recent incident${incidents.length === 1 ? "" : "s"}`}
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge tone="ghost">{incidentPageLabel}</Badge>
              <Badge tone={selectedIncidentIds.length ? "info" : "ghost"}>
                {selectedIncidentIds.length} selected
              </Badge>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Select all on this page"
                aria-label="Select all on this page"
                onClick={selectAllVisibleIncidents}
                disabled={!pagedIncidents.length}
              >
                <i data-lucide="square-check-big"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Clear selection"
                aria-label="Clear selection"
                onClick={() => setSelectedIncidentIds([])}
                disabled={!selectedIncidentIds.length}
              >
                <i data-lucide="x"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn border-rose-500/30 text-rose-100 hover:bg-rose-950/20 disabled:opacity-50"
                title="Delete selected incidents"
                aria-label="Delete selected incidents"
                onClick={() => setDeleteIncidentsConfirm({ ids: selectedIncidentIds, all: false })}
                disabled={!selectedIncidentIds.length || deleteIncidentsMutation.isPending}
              >
                <i data-lucide="trash-2"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn border-rose-500/30 text-rose-100 hover:bg-rose-950/20 disabled:opacity-50"
                title="Delete all incidents"
                aria-label="Delete all incidents"
                onClick={() => setDeleteIncidentsConfirm({ ids: [], all: true })}
                disabled={!incidents.length || deleteIncidentsMutation.isPending}
              >
                <i data-lucide="trash"></i>
              </button>
              <label className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-slate-500">
                <span>Per page</span>
                <select
                  className="tcp-input h-8 px-2 text-xs"
                  value={incidentPageSize}
                  onChange={(event) => {
                    setIncidentPageSize(Number(event.target.value) || DEFAULT_LIST_PAGE_SIZE);
                    setIncidentPage(1);
                  }}
                >
                  {LIST_PAGE_SIZE_OPTIONS.map((value) => (
                    <option key={value} value={value}>
                      {value}
                    </option>
                  ))}
                </select>
              </label>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Previous page"
                aria-label="Previous page"
                onClick={() => setIncidentPage((page) => clampPage(page - 1, incidentPageCount))}
                disabled={safeIncidentPage <= 1}
              >
                <i data-lucide="chevron-left"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Next page"
                aria-label="Next page"
                onClick={() => setIncidentPage((page) => clampPage(page + 1, incidentPageCount))}
                disabled={safeIncidentPage >= incidentPageCount}
              >
                <i data-lucide="chevron-right"></i>
              </button>
            </div>
          }
        >
          {incidentsQuery.isError ? <QueryError error={incidentsQuery.error} /> : null}
          {incidentsQuery.isLoading ? (
            <EmptyState title="Loading incidents" text="Checking the reporter incident queue." />
          ) : incidents.length ? (
            <div className="grid gap-3">
              {pagedIncidents.map((incident, index) => {
                const incidentId = incidentIdOf(incident, incidentPageStart + index);
                const title = firstString(incident, ["title", "summary", "event_type"], incidentId);
                const source = firstString(
                  incident,
                  ["source", "event_type", "component"],
                  "unknown source"
                );
                const severity = firstString(incident, ["severity", "level"]);
                const checked = selectedIncidentSet.has(incidentId);
                return (
                  <div key={incidentId} className="tcp-list-item">
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="flex min-w-0 items-start gap-3">
                        <label className="mt-1 flex h-6 w-6 cursor-pointer items-center justify-center rounded border border-white/15 bg-black/20 hover:border-sky-500/40">
                          <input
                            type="checkbox"
                            className="h-4 w-4 accent-sky-400"
                            checked={checked}
                            onChange={() => toggleIncidentSelection(incidentId)}
                            aria-label={`Select incident ${incidentId}`}
                          />
                        </label>
                        <div className="min-w-0">
                          <div className="truncate font-medium">{title}</div>
                          <div className="tcp-subtle mt-1 break-all text-xs">{incidentId}</div>
                        </div>
                      </div>
                      <div className="flex flex-wrap justify-end gap-2">
                        {severity ? <Badge tone={statusTone(severity)}>{severity}</Badge> : null}
                        <Badge tone={statusTone(incident.status)}>
                          {String(incident.status || "unknown")}
                        </Badge>
                      </div>
                    </div>
                    <div className="tcp-subtle mt-2 grid gap-1 text-xs md:grid-cols-2">
                      <div>Source: {source}</div>
                      <div>
                        Created:{" "}
                        {formatTime(firstNumber(incident, ["created_at_ms", "created_at"]))}
                      </div>
                      <div>
                        Updated:{" "}
                        {formatTime(
                          firstNumber(incident, ["updated_at_ms", "last_seen_at_ms", "updated_at"])
                        )}
                      </div>
                      <div>Occurrences: {Number(incident.occurrence_count || 0)}</div>
                    </div>
                    <SignalMetadataGrid record={incident} />
                    <SignalLifecyclePanel record={incident} kind="incident" />
                    {incident.detail ? (
                      <div className="tcp-subtle mt-2 line-clamp-3 text-xs">
                        {previewText(incident.detail)}
                      </div>
                    ) : null}
                    {incident.last_error ? (
                      <div className="tcp-subtle mt-2 break-words text-xs text-rose-200">
                        {String(incident.last_error)}
                      </div>
                    ) : null}
                    <QualityGateStrip record={incident} />
                    <div className="mt-3 flex flex-wrap gap-2">
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={incidentMutation.isPending}
                        onClick={() => incidentMutation.mutate({ action: "view", incident })}
                      >
                        <i data-lucide="file-search"></i>
                        View
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={incidentMutation.isPending}
                        onClick={() => incidentMutation.mutate({ action: "replay", incident })}
                      >
                        <i data-lucide="rotate-cw"></i>
                        Replay incident
                      </button>
                      {incident.draft_id ? (
                        <button
                          type="button"
                          className="tcp-btn h-8 px-3 text-xs"
                          disabled={incidentMutation.isPending}
                          onClick={() => incidentMutation.mutate({ action: "triage", incident })}
                        >
                          <i data-lucide="sparkles"></i>
                          Create triage run
                        </button>
                      ) : null}
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs border-rose-500/30 text-rose-100 hover:bg-rose-950/20"
                        disabled={deleteIncidentsMutation.isPending}
                        onClick={() => setDeleteIncidentsConfirm({ ids: [incidentId], all: false })}
                      >
                        <i data-lucide="trash-2"></i>
                        Delete
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <EmptyState
              title="No incidents"
              text="No runtime or workflow failures have been captured."
            />
          )}
        </PanelCard>

        {/* Report Issue form is rendered inside the drawer at the end of the page. */}

        <PanelCard
          title="Drafts"
          subtitle={`${drafts.length} recent draft${drafts.length === 1 ? "" : "s"}`}
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge tone="ghost">{draftPageLabel}</Badge>
              <Badge tone={selectedDraftIds.length ? "info" : "ghost"}>
                {selectedDraftIds.length} selected
              </Badge>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Select all on this page"
                aria-label="Select all on this page"
                onClick={selectAllVisibleDrafts}
                disabled={!pagedDrafts.length}
              >
                <i data-lucide="square-check-big"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Clear selection"
                aria-label="Clear selection"
                onClick={() => setSelectedDraftIds([])}
                disabled={!selectedDraftIds.length}
              >
                <i data-lucide="x"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn border-rose-500/30 text-rose-100 hover:bg-rose-950/20 disabled:opacity-50"
                title="Delete selected drafts"
                aria-label="Delete selected drafts"
                onClick={() => setDeleteDraftsConfirm({ ids: selectedDraftIds, all: false })}
                disabled={!selectedDraftIds.length || deleteDraftsMutation.isPending}
              >
                <i data-lucide="trash-2"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn border-rose-500/30 text-rose-100 hover:bg-rose-950/20 disabled:opacity-50"
                title="Delete all drafts"
                aria-label="Delete all drafts"
                onClick={() => setDeleteDraftsConfirm({ ids: [], all: true })}
                disabled={!drafts.length || deleteDraftsMutation.isPending}
              >
                <i data-lucide="trash"></i>
              </button>
              <label className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-slate-500">
                <span>Per page</span>
                <select
                  className="tcp-input h-8 px-2 text-xs"
                  value={draftPageSize}
                  onChange={(event) => {
                    setDraftPageSize(Number(event.target.value) || DEFAULT_LIST_PAGE_SIZE);
                    setDraftPage(1);
                  }}
                >
                  {LIST_PAGE_SIZE_OPTIONS.map((value) => (
                    <option key={value} value={value}>
                      {value}
                    </option>
                  ))}
                </select>
              </label>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Previous page"
                aria-label="Previous page"
                onClick={() => setDraftPage((page) => clampPage(page - 1, draftPageCount))}
                disabled={safeDraftPage <= 1}
              >
                <i data-lucide="chevron-left"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Next page"
                aria-label="Next page"
                onClick={() => setDraftPage((page) => clampPage(page + 1, draftPageCount))}
                disabled={safeDraftPage >= draftPageCount}
              >
                <i data-lucide="chevron-right"></i>
              </button>
            </div>
          }
        >
          {draftsQuery.isError ? <QueryError error={draftsQuery.error} /> : null}
          {draftsQuery.isLoading ? (
            <EmptyState title="Loading drafts" text="Checking generated issue drafts." />
          ) : drafts.length ? (
            <div className="grid gap-3">
              {pagedDrafts.map((draft, index) => {
                const draftId = draftIdOf(draft, draftPageStart + index);
                const matchedIssue = firstString(draft, ["matched_issue_number", "issue_number"]);
                const confidence = firstString(draft, ["confidence", "match_confidence"]);
                const checked = selectedDraftSet.has(draftId);
                return (
                  <div key={draftId} className="tcp-list-item">
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="flex min-w-0 items-start gap-3">
                        <label className="mt-1 flex h-6 w-6 cursor-pointer items-center justify-center rounded border border-white/15 bg-black/20 hover:border-sky-500/40">
                          <input
                            type="checkbox"
                            className="h-4 w-4 accent-sky-400"
                            checked={checked}
                            onChange={() => toggleDraftSelection(draftId)}
                            aria-label={`Select draft ${draftId}`}
                          />
                        </label>
                        <div className="min-w-0">
                          <div className="truncate font-medium">
                            {firstString(draft, ["title", "fingerprint"], draftId)}
                          </div>
                          <div className="tcp-subtle mt-1 break-all text-xs">{draftId}</div>
                        </div>
                      </div>
                      <Badge tone={statusTone(draft.status)}>
                        {String(draft.status || "unknown")}
                      </Badge>
                    </div>
                    <div className="tcp-subtle mt-2 grid gap-1 text-xs md:grid-cols-3">
                      <div>Repo: {firstString(draft, ["repo"], "unset")}</div>
                      <div>
                        Created: {formatTime(firstNumber(draft, ["created_at_ms", "created_at"]))}
                      </div>
                      <div>
                        Match:{" "}
                        {matchedIssue
                          ? `#${matchedIssue}${draft.matched_issue_state ? ` (${draft.matched_issue_state})` : ""}`
                          : "none"}
                        {confidence ? ` · ${confidence}` : ""}
                      </div>
                    </div>
                    <SignalMetadataGrid record={draft} />
                    <SignalLifecyclePanel record={draft} kind="draft" />
                    <div className="tcp-subtle mt-2 whitespace-pre-wrap text-xs">
                      {previewText(draft.detail || draft.evidence_digest)}
                    </div>
                    {draft.last_post_error ? (
                      <div className="tcp-subtle mt-2 break-words text-xs text-rose-200">
                        {String(draft.last_post_error)}
                      </div>
                    ) : null}
                    <QualityGateStrip record={draft} />
                    <div className="mt-3 flex flex-wrap gap-2">
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() => draftMutation.mutate({ action: "view", draft })}
                      >
                        <i data-lucide="file-search"></i>
                        View
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() =>
                          draftMutation.mutate({
                            action: "approve",
                            draft,
                            reason:
                              window.prompt("Approval reason", "Approved from Bug Monitor.") ||
                              undefined,
                          })
                        }
                      >
                        <i data-lucide="check"></i>
                        Approve
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() =>
                          draftMutation.mutate({
                            action: "deny",
                            draft,
                            reason:
                              window.prompt("Denial reason", "Denied from Bug Monitor.") ||
                              undefined,
                          })
                        }
                      >
                        <i data-lucide="x"></i>
                        Deny
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() => draftMutation.mutate({ action: "triage-run", draft })}
                      >
                        <i data-lucide="sparkles"></i>
                        Triage run
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() => draftMutation.mutate({ action: "triage-summary", draft })}
                      >
                        <i data-lucide="clipboard-list"></i>
                        Triage summary
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() => draftMutation.mutate({ action: "issue-draft", draft })}
                      >
                        <i data-lucide="square-pen"></i>
                        Issue draft
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() => draftMutation.mutate({ action: "publish", draft })}
                      >
                        <i data-lucide="bug-play"></i>
                        Publish
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() => draftMutation.mutate({ action: "recheck", draft })}
                      >
                        <i data-lucide="refresh-cw"></i>
                        Recheck match
                      </button>
                      {draft.github_issue_url ? (
                        <a
                          className="tcp-btn h-8 px-3 text-xs"
                          href={String(draft.github_issue_url)}
                          target="_blank"
                          rel="noreferrer"
                        >
                          <i data-lucide="external-link"></i>
                          GitHub
                        </a>
                      ) : null}
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs border-rose-500/30 text-rose-100 hover:bg-rose-950/20"
                        disabled={deleteDraftsMutation.isPending}
                        onClick={() => setDeleteDraftsConfirm({ ids: [draftId], all: false })}
                      >
                        <i data-lucide="trash-2"></i>
                        Delete
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <EmptyState title="No drafts" text="No generated issue drafts are waiting." />
          )}
        </PanelCard>

        <PanelCard
          title="Posts / Published Issues"
          subtitle={`${posts.length} recent post${posts.length === 1 ? "" : "s"}`}
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge tone="ghost">{postPageLabel}</Badge>
              <Badge tone={selectedPostIds.length ? "info" : "ghost"}>
                {selectedPostIds.length} selected
              </Badge>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Select all on this page"
                aria-label="Select all on this page"
                onClick={selectAllVisiblePosts}
                disabled={!pagedPosts.length}
              >
                <i data-lucide="square-check-big"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Clear selection"
                aria-label="Clear selection"
                onClick={() => setSelectedPostIds([])}
                disabled={!selectedPostIds.length}
              >
                <i data-lucide="x"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn border-rose-500/30 text-rose-100 hover:bg-rose-950/20 disabled:opacity-50"
                title="Delete selected posts"
                aria-label="Delete selected posts"
                onClick={() => setDeletePostsConfirm({ ids: selectedPostIds, all: false })}
                disabled={!selectedPostIds.length || deletePostsMutation.isPending}
              >
                <i data-lucide="trash-2"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn border-rose-500/30 text-rose-100 hover:bg-rose-950/20 disabled:opacity-50"
                title="Delete all posts"
                aria-label="Delete all posts"
                onClick={() => setDeletePostsConfirm({ ids: [], all: true })}
                disabled={!posts.length || deletePostsMutation.isPending}
              >
                <i data-lucide="trash"></i>
              </button>
              <label className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-slate-500">
                <span>Per page</span>
                <select
                  className="tcp-input h-8 px-2 text-xs"
                  value={postPageSize}
                  onChange={(event) => {
                    setPostPageSize(Number(event.target.value) || DEFAULT_LIST_PAGE_SIZE);
                    setPostPage(1);
                  }}
                >
                  {LIST_PAGE_SIZE_OPTIONS.map((value) => (
                    <option key={value} value={value}>
                      {value}
                    </option>
                  ))}
                </select>
              </label>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Previous page"
                aria-label="Previous page"
                onClick={() => setPostPage((page) => clampPage(page - 1, postPageCount))}
                disabled={safePostPage <= 1}
              >
                <i data-lucide="chevron-left"></i>
              </button>
              <button
                type="button"
                className="tcp-icon-btn"
                title="Next page"
                aria-label="Next page"
                onClick={() => setPostPage((page) => clampPage(page + 1, postPageCount))}
                disabled={safePostPage >= postPageCount}
              >
                <i data-lucide="chevron-right"></i>
              </button>
            </div>
          }
        >
          {postsQuery.isError ? <QueryError error={postsQuery.error} /> : null}
          {postsQuery.isLoading ? (
            <EmptyState title="Loading posts" text="Checking GitHub publishing attempts." />
          ) : posts.length ? (
            <div className="grid gap-3">
              {pagedPosts.map((post, index) => {
                const postId = postIdOf(post, postPageStart + index);
                const url = firstString(post, [
                  "issue_url",
                  "github_issue_url",
                  "comment_url",
                  "github_comment_url",
                ]);
                const checked = selectedPostSet.has(postId);
                return (
                  <div key={postId} className="tcp-list-item">
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="flex min-w-0 items-start gap-3">
                        <label className="mt-1 flex h-6 w-6 cursor-pointer items-center justify-center rounded border border-white/15 bg-black/20 hover:border-sky-500/40">
                          <input
                            type="checkbox"
                            className="h-4 w-4 accent-sky-400"
                            checked={checked}
                            onChange={() => togglePostSelection(postId)}
                            aria-label={`Select post ${postId}`}
                          />
                        </label>
                        <div className="min-w-0">
                          <div className="truncate font-medium">
                            {firstString(post, ["title", "operation"], "GitHub post")}
                          </div>
                          <div className="tcp-subtle mt-1 break-all text-xs">{postId}</div>
                        </div>
                      </div>
                      <Badge tone={statusTone(post.status)}>
                        {String(post.status || "unknown")}
                      </Badge>
                    </div>
                    <div className="tcp-subtle mt-2 grid gap-1 text-xs">
                      <div>Repo: {firstString(post, ["repo"], "unset")}</div>
                      <div>
                        Posted:{" "}
                        {formatTime(
                          firstNumber(post, [
                            "posted_at_ms",
                            "github_posted_at_ms",
                            "created_at_ms",
                            "updated_at_ms",
                          ])
                        )}
                      </div>
                      {post.issue_number ? <div>Issue: #{String(post.issue_number)}</div> : null}
                    </div>
                    <SignalMetadataGrid record={post} />
                    <SignalLifecyclePanel record={post} kind="post" />
                    {post.error ? (
                      <div className="tcp-subtle mt-2 break-words text-xs text-rose-200">
                        {String(post.error)}
                      </div>
                    ) : null}
                    <div className="mt-3 flex flex-wrap gap-2">
                      {url ? (
                        <a
                          className="tcp-btn h-8 px-3 text-xs"
                          href={url}
                          target="_blank"
                          rel="noreferrer"
                        >
                          <i data-lucide="external-link"></i>
                          Open GitHub
                        </a>
                      ) : null}
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs border-rose-500/30 text-rose-100 hover:bg-rose-950/20"
                        disabled={deletePostsMutation.isPending}
                        onClick={() => setDeletePostsConfirm({ ids: [postId], all: false })}
                      >
                        <i data-lucide="trash-2"></i>
                        Delete
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <EmptyState
              title="No published issues"
              text="No GitHub publishing attempts have been recorded."
            />
          )}
        </PanelCard>
      </div>

      <DetailDrawer
        open={!!detail}
        title={detail?.title || "Bug Monitor Detail"}
        onClose={() => setDetail(null)}
      >
        <pre className="tcp-code whitespace-pre-wrap break-words text-xs">
          {detail ? formatJson(detail.value) : ""}
        </pre>
      </DetailDrawer>

      <DetailDrawer open={reportOpen} title="Report Issue" onClose={() => setReportOpen(false)}>
        <form
          className="grid gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            reportMutation.mutate(undefined, {
              onSuccess: () => setReportOpen(false),
            });
          }}
        >
          <p className="tcp-subtle text-xs">Manual issue intake for runtime findings.</p>
          <label className="grid gap-1">
            <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Title</span>
            <input
              className="tcp-input"
              value={form.title}
              onChange={(event) =>
                setForm((prev) => ({ ...prev, title: event.currentTarget.value }))
              }
              placeholder="Failure title"
            />
          </label>
          <label className="grid gap-1">
            <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Summary / body</span>
            <textarea
              className="tcp-input min-h-28 resize-y"
              value={form.body}
              onChange={(event) =>
                setForm((prev) => ({ ...prev, body: event.currentTarget.value }))
              }
              placeholder="What happened?"
            />
          </label>
          <div className="grid gap-3 md:grid-cols-2">
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Severity</span>
              <select
                className="tcp-input"
                value={form.severity}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, severity: event.currentTarget.value }))
                }
              >
                <option value="error">Error</option>
                <option value="warn">Warning</option>
                <option value="info">Info</option>
                <option value="critical">Critical</option>
              </select>
            </label>
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Source</span>
              <input
                className="tcp-input"
                value={form.source}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, source: event.currentTarget.value }))
                }
                placeholder="manual, runtime, workflow"
              />
            </label>
            <label className="grid gap-1 md:col-span-2">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Labels</span>
              <input
                className="tcp-input"
                value={form.labels}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, labels: event.currentTarget.value }))
                }
                placeholder="bug-monitor, regression"
              />
            </label>
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Confidence</span>
              <select
                className="tcp-input"
                value={form.confidence}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, confidence: event.currentTarget.value }))
                }
              >
                <option value="high">High</option>
                <option value="medium">Medium</option>
                <option value="low">Low</option>
              </select>
            </label>
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Risk level</span>
              <select
                className="tcp-input"
                value={form.riskLevel}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, riskLevel: event.currentTarget.value }))
                }
              >
                <option value="low">Low</option>
                <option value="medium">Medium</option>
                <option value="high">High</option>
              </select>
            </label>
            <label className="grid gap-1 md:col-span-2">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                Expected destination
              </span>
              <select
                className="tcp-input"
                value={form.expectedDestination}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, expectedDestination: event.currentTarget.value }))
                }
              >
                <option value="bug_monitor_issue_draft">Bug Monitor issue draft</option>
                <option value="triage_only">Triage only</option>
                <option value="research_brief">Research brief</option>
                <option value="workflow_proposal">Workflow proposal</option>
                <option value="product_opportunity">Product opportunity</option>
              </select>
            </label>
            <label className="grid gap-1 md:col-span-2">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Evidence refs</span>
              <textarea
                className="tcp-input min-h-20 resize-y"
                value={form.evidenceRefs}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, evidenceRefs: event.currentTarget.value }))
                }
                placeholder="One artifact, log, file, URL, or context ref per line"
              />
            </label>
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Related run ID</span>
              <input
                className="tcp-input"
                value={form.relatedRunId}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, relatedRunId: event.currentTarget.value }))
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                Related workflow ID
              </span>
              <input
                className="tcp-input"
                value={form.relatedWorkflowId}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, relatedWorkflowId: event.currentTarget.value }))
                }
              />
            </label>
            <label className="grid gap-1 md:col-span-2">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                Related file / path
              </span>
              <input
                className="tcp-input"
                value={form.relatedFile}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, relatedFile: event.currentTarget.value }))
                }
              />
            </label>
            <label className="grid gap-1 md:col-span-2">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                Reproduction notes
              </span>
              <textarea
                className="tcp-input min-h-20 resize-y"
                value={form.reproductionNotes}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, reproductionNotes: event.currentTarget.value }))
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                Expected behavior
              </span>
              <textarea
                className="tcp-input min-h-20 resize-y"
                value={form.expectedBehavior}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, expectedBehavior: event.currentTarget.value }))
                }
              />
            </label>
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                Actual behavior
              </span>
              <textarea
                className="tcp-input min-h-20 resize-y"
                value={form.actualBehavior}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, actualBehavior: event.currentTarget.value }))
                }
              />
            </label>
          </div>
          <div className="flex flex-wrap justify-end gap-2">
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => setForm(emptyReporterForm)}
              disabled={reportMutation.isPending}
            >
              <i data-lucide="eraser"></i>
              Clear
            </button>
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => setReportOpen(false)}
              disabled={reportMutation.isPending}
            >
              <i data-lucide="x"></i>
              Cancel
            </button>
            <button
              type="submit"
              className="tcp-btn-primary h-8 px-3 text-xs"
              disabled={reportMutation.isPending}
            >
              <i data-lucide="send"></i>
              {reportMutation.isPending ? "Submitting..." : "Submit report"}
            </button>
          </div>
        </form>
      </DetailDrawer>

      <ConfirmDialog
        open={!!deleteIncidentsConfirm}
        title={deleteIncidentsConfirm?.all ? "Delete all incidents" : "Delete selected incidents"}
        message={
          <span>
            {deleteIncidentsConfirm?.all
              ? `This will permanently remove all ${incidents.length} incident(s).`
              : `This will permanently remove ${deleteIncidentsConfirm?.ids.length || 0} selected incident(s).`}
          </span>
        }
        confirmLabel={deleteIncidentsMutation.isPending ? "Deleting..." : "Delete"}
        confirmIcon="trash-2"
        confirmTone="danger"
        confirmDisabled={deleteIncidentsMutation.isPending}
        onCancel={() => setDeleteIncidentsConfirm(null)}
        onConfirm={() => {
          if (!deleteIncidentsConfirm) return;
          deleteIncidentsMutation.mutate({
            ids: deleteIncidentsConfirm.ids,
            all: deleteIncidentsConfirm.all,
          });
        }}
      />

      <ConfirmDialog
        open={!!deleteDraftsConfirm}
        title={deleteDraftsConfirm?.all ? "Delete all drafts" : "Delete selected drafts"}
        message={
          <span>
            {deleteDraftsConfirm?.all
              ? `This will permanently remove all ${drafts.length} draft(s).`
              : `This will permanently remove ${deleteDraftsConfirm?.ids.length || 0} selected draft(s).`}
          </span>
        }
        confirmLabel={deleteDraftsMutation.isPending ? "Deleting..." : "Delete"}
        confirmIcon="trash-2"
        confirmTone="danger"
        confirmDisabled={deleteDraftsMutation.isPending}
        onCancel={() => setDeleteDraftsConfirm(null)}
        onConfirm={() => {
          if (!deleteDraftsConfirm) return;
          deleteDraftsMutation.mutate({
            ids: deleteDraftsConfirm.ids,
            all: deleteDraftsConfirm.all,
          });
        }}
      />

      <ConfirmDialog
        open={!!deletePostsConfirm}
        title={deletePostsConfirm?.all ? "Delete all posts" : "Delete selected posts"}
        message={
          <span>
            {deletePostsConfirm?.all
              ? `This will permanently remove all ${posts.length} post(s).`
              : `This will permanently remove ${deletePostsConfirm?.ids.length || 0} selected post(s).`}
          </span>
        }
        confirmLabel={deletePostsMutation.isPending ? "Deleting..." : "Delete"}
        confirmIcon="trash-2"
        confirmTone="danger"
        confirmDisabled={deletePostsMutation.isPending}
        onCancel={() => setDeletePostsConfirm(null)}
        onConfirm={() => {
          if (!deletePostsConfirm) return;
          deletePostsMutation.mutate({
            ids: deletePostsConfirm.ids,
            all: deletePostsConfirm.all,
          });
        }}
      />
    </AnimatedPage>
  );
}
