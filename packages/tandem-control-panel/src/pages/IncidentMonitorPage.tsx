import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { ConfirmDialog } from "../components/ControlPanelDialogs";
import { AnimatedPage, Badge, DetailDrawer, EmptyState, IdChip, PanelCard } from "../ui/index.tsx";
import {
  QualityGateStrip,
  QueryError,
  SignalLifecyclePanel,
  SignalMetadataGrid,
  StatTile,
} from "./IncidentMonitorPageShared";
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
  labels: "incident-monitor, runtime-failure",
  confidence: "medium",
  riskLevel: "medium",
  expectedDestination: "incident_monitor_issue_draft",
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

function githubRepoIssueUrl(record: AnyRecord): string {
  const explicit = firstString(record, ["github_issue_url", "issue_url"]);
  if (explicit) return explicit;
  const repo = firstString(record, ["repo"]);
  const issueNumber = firstString(record, ["issue_number", "matched_issue_number"]);
  if (/^[^/\s]+\/[^/\s]+$/.test(repo) && /^\d+$/.test(issueNumber)) {
    return `https://github.com/${repo}/issues/${issueNumber}`;
  }
  return "";
}

function githubCommentUrl(record: AnyRecord): string {
  return firstString(record, ["github_comment_url", "comment_url"]);
}

function githubIssueLabel(record: AnyRecord): string {
  const issueNumber = firstString(record, ["issue_number", "matched_issue_number"]);
  return issueNumber ? `Issue #${issueNumber}` : "GitHub issue";
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

export function IncidentMonitorPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [form, setForm] = useState<ReporterForm>(emptyReporterForm);
  const [detail, setDetail] = useState<{ title: string; value: unknown } | null>(null);
  const [reportOpen, setReportOpen] = useState(false);

  const [incidentPage, setIncidentPage] = useState(1);
  const [incidentPageSize, setIncidentPageSize] = useState(DEFAULT_LIST_PAGE_SIZE);
  const [selectedIncidentIds, setSelectedIncidentIds] = useState<string[]>([]);
  const [incidentsCollapsed, setIncidentsCollapsed] = useState(true);
  const [deleteIncidentsConfirm, setDeleteIncidentsConfirm] = useState<{
    ids: string[];
    all: boolean;
  } | null>(null);

  const [draftPage, setDraftPage] = useState(1);
  const [draftPageSize, setDraftPageSize] = useState(DEFAULT_LIST_PAGE_SIZE);
  const [selectedDraftIds, setSelectedDraftIds] = useState<string[]>([]);
  const [draftsCollapsed, setDraftsCollapsed] = useState(true);
  const [deleteDraftsConfirm, setDeleteDraftsConfirm] = useState<{
    ids: string[];
    all: boolean;
  } | null>(null);

  const [postPage, setPostPage] = useState(1);
  const [postPageSize, setPostPageSize] = useState(DEFAULT_LIST_PAGE_SIZE);
  const [postDestinationFilter, setPostDestinationFilter] = useState("");
  const [selectedPostIds, setSelectedPostIds] = useState<string[]>([]);
  const [postsCollapsed, setPostsCollapsed] = useState(true);
  const [deletePostsConfirm, setDeletePostsConfirm] = useState<{
    ids: string[];
    all: boolean;
  } | null>(null);

  const invalidateMonitorQueries = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["incident-monitor", "status"] }),
      queryClient.invalidateQueries({ queryKey: ["incident-monitor", "incidents"] }),
      queryClient.invalidateQueries({ queryKey: ["incident-monitor", "drafts"] }),
      queryClient.invalidateQueries({ queryKey: ["incident-monitor", "posts"] }),
    ]);
  };

  const statusQuery = useQuery({
    queryKey: ["incident-monitor", "status"],
    queryFn: () => client.incidentMonitor.getStatus(),
    refetchInterval: 10000,
  });

  const incidentsQuery = useQuery({
    queryKey: ["incident-monitor", "incidents", LIST_FETCH_LIMIT],
    queryFn: () => client.incidentMonitor.listIncidents({ limit: LIST_FETCH_LIMIT }),
  });

  const draftsQuery = useQuery({
    queryKey: ["incident-monitor", "drafts", LIST_FETCH_LIMIT],
    queryFn: () => client.incidentMonitor.listDrafts({ limit: LIST_FETCH_LIMIT }),
  });

  const postsQuery = useQuery({
    queryKey: ["incident-monitor", "posts", LIST_FETCH_LIMIT, postDestinationFilter],
    queryFn: () =>
      client.incidentMonitor.listPosts({
        limit: LIST_FETCH_LIMIT,
        destinationId: postDestinationFilter || undefined,
      }),
  });

  const statusMutation = useMutation({
    mutationFn: async (action: "recompute" | "pause" | "resume" | "debug") => {
      if (action === "recompute") return client.incidentMonitor.recomputeStatus();
      if (action === "pause") return client.incidentMonitor.pause();
      if (action === "resume") return client.incidentMonitor.resume();
      return client.incidentMonitor.debug();
    },
    onSuccess: async (result, action) => {
      if (action === "debug") {
        setDetail({ title: "Incident Monitor Debug Payload", value: result });
      }
      toast(
        "ok",
        action === "debug" ? "Incident Monitor debug payload loaded." : "Incident Monitor updated."
      );
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const incidentMutation = useMutation({
    mutationFn: async (vars: {
      action: "view" | "replay" | "triage" | "route-preview";
      incident: AnyRecord;
    }) => {
      const incidentId = firstString(vars.incident, ["incident_id", "id"]);
      if (!incidentId) throw new Error("Incident id missing.");
      if (vars.action === "view") return client.incidentMonitor.getIncident(incidentId);
      if (vars.action === "route-preview") {
        return client.incidentMonitor.previewRoute({ incident_id: incidentId });
      }
      if (vars.action === "triage") {
        const draftId = firstString(vars.incident, ["draft_id"]);
        if (!draftId) throw new Error("Incident has no draft id for triage.");
        return client.incidentMonitor.createTriageRun(draftId);
      }
      return client.incidentMonitor.replayIncident(incidentId);
    },
    onSuccess: async (result, vars) => {
      if (vars.action === "view") {
        setDetail({
          title: `Incident ${firstString(vars.incident, ["incident_id", "id"], "detail")}`,
          value: result,
        });
      } else if (vars.action === "route-preview") {
        setDetail({
          title: `Route preview ${firstString(vars.incident, ["incident_id", "id"], "incident")}`,
          value: result,
        });
      }
      toast("ok", vars.action === "view" ? "Incident loaded." : "Incident action queued.");
      await invalidateMonitorQueries();
    },
    onError: (error: any) => toast("err", error?.message || String(error)),
  });

  const draftMutation = useMutation({
    mutationFn: async (vars: {
      action: string;
      draft: AnyRecord;
      reason?: string;
      destinationIds?: string[];
    }) => {
      const draftId = firstString(vars.draft, ["draft_id", "id"]);
      if (!draftId) throw new Error("Draft id missing.");
      if (vars.action === "view") return client.incidentMonitor.getDraft(draftId);
      if (vars.action === "route-preview") {
        return client.incidentMonitor.previewRoute({ draft_id: draftId });
      }
      if (vars.action === "approve")
        return client.incidentMonitor.approveDraft(draftId, vars.reason);
      if (vars.action === "deny") return client.incidentMonitor.denyDraft(draftId, vars.reason);
      if (vars.action === "triage-run") return client.incidentMonitor.createTriageRun(draftId);
      if (vars.action === "triage-summary") {
        return client.incidentMonitor.createTriageSummary(draftId, {
          suggested_title: firstString(vars.draft, ["title", "fingerprint"], "Runtime failure"),
          what_happened: firstString(vars.draft, ["detail", "evidence_digest"], ""),
          notes: vars.reason || "Created from the control panel.",
        });
      }
      if (vars.action === "issue-draft")
        return client.incidentMonitor.createIssueDraft(draftId, {});
      if (vars.action === "publish") {
        return client.incidentMonitor.publishDraft(draftId, {
          ...(vars.destinationIds?.length ? { destination_ids: vars.destinationIds } : {}),
          ...(vars.reason ? { reason: vars.reason } : {}),
        });
      }
      if (vars.action === "recheck") return client.incidentMonitor.recheckMatch(draftId, {});
      throw new Error(`Unknown draft action: ${vars.action}`);
    },
    onSuccess: async (result, vars) => {
      if (vars.action === "view") {
        setDetail({
          title: `Draft ${firstString(vars.draft, ["draft_id", "id"], "detail")}`,
          value: result,
        });
      } else if (vars.action === "route-preview") {
        setDetail({
          title: `Route preview ${firstString(vars.draft, ["draft_id", "id"], "draft")}`,
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
      return client.incidentMonitor.report(payload);
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
      if (vars.all) return client.incidentMonitor.bulkDeleteIncidents({ all: true });
      return client.incidentMonitor.bulkDeleteIncidents({ ids: vars.ids });
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
      if (vars.all) return client.incidentMonitor.bulkDeleteDrafts({ all: true });
      return client.incidentMonitor.bulkDeleteDrafts({ ids: vars.ids });
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
      if (vars.all) return client.incidentMonitor.bulkDeletePosts({ all: true });
      return client.incidentMonitor.bulkDeletePosts({ ids: vars.ids });
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
  const destinations = useMemo(() => {
    const statusDestinations = asArray(status.destinations);
    if (statusDestinations.length) return statusDestinations;
    return asArray(config.destinations);
  }, [config.destinations, status.destinations]);
  const destinationReadiness = useMemo(
    () => asArray(status.destination_readiness),
    [status.destination_readiness]
  );
  const readinessByDestination = useMemo(
    () =>
      new Map(
        destinationReadiness.map((row) => [
          firstString(row, ["destination_id", "destinationId", "id"]),
          row,
        ])
      ),
    [destinationReadiness]
  );
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
    setPostPage(1);
    setSelectedPostIds([]);
  }, [postDestinationFilter]);

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
    destinations.length,
    postDestinationFilter,
    selectedIncidentIds.length,
    selectedDraftIds.length,
    selectedPostIds.length,
    safeIncidentPage,
    safeDraftPage,
    safePostPage,
    incidentPageSize,
    draftPageSize,
    postPageSize,
    incidentsCollapsed,
    draftsCollapsed,
    postsCollapsed,
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
          title="Incident Monitor"
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
                  ]).then(() => toast("ok", "Incident Monitor refreshed."));
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
                <i data-lucide="shield-alert"></i>
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
            {destinations.length ? (
              <div className="flex flex-wrap gap-2">
                {destinations.map((destination, index) => {
                  const destinationId = firstString(
                    destination,
                    ["destination_id", "destinationId", "id"],
                    `destination-${index + 1}`
                  );
                  const destinationName = firstString(destination, ["name"], destinationId);
                  const destinationKind = firstString(destination, ["kind"], "destination");
                  const destinationReady = readinessByDestination.get(destinationId);
                  const ready =
                    destinationReady?.ready === true ||
                    destinationReady?.publish_ready === true ||
                    destinationReady?.publishReady === true;
                  return (
                    <Badge key={destinationId} tone={ready ? "ok" : "warn"}>
                      {destinationName} | {destinationKind}
                    </Badge>
                  );
                })}
              </div>
            ) : null}
          </div>
        </PanelCard>

        <PanelCard
          title="Incidents"
          subtitle={`${incidents.length} recent incident${incidents.length === 1 ? "" : "s"}`}
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge tone={incidents.length ? "info" : "ghost"}>{incidents.length}</Badge>
              <button
                type="button"
                className="tcp-icon-btn"
                title={incidentsCollapsed ? "Expand incidents" : "Collapse incidents"}
                aria-label={incidentsCollapsed ? "Expand incidents" : "Collapse incidents"}
                onClick={() => setIncidentsCollapsed((value) => !value)}
              >
                <i data-lucide={incidentsCollapsed ? "chevron-down" : "chevron-up"}></i>
              </button>
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
                <i data-lucide="list-x"></i>
              </button>
              <label className="flex items-center gap-2 tcp-text-caption uppercase tracking-wide text-slate-500">
                <span>Per page</span>
                <select
                  className="tcp-input h-8 min-w-[4rem] px-2 text-xs leading-none"
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
          {incidentsCollapsed ? null : incidentsQuery.isError ? (
            <QueryError error={incidentsQuery.error} />
          ) : null}
          {incidentsCollapsed ? null : incidentsQuery.isLoading ? (
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
                          <div className="mt-1"><IdChip value={incidentId} /></div>
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
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={incidentMutation.isPending}
                        onClick={() =>
                          incidentMutation.mutate({ action: "route-preview", incident })
                        }
                      >
                        <i data-lucide="route"></i>
                        Route preview
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
              <Badge tone={drafts.length ? "info" : "ghost"}>{drafts.length}</Badge>
              <button
                type="button"
                className="tcp-icon-btn"
                title={draftsCollapsed ? "Expand drafts" : "Collapse drafts"}
                aria-label={draftsCollapsed ? "Expand drafts" : "Collapse drafts"}
                onClick={() => setDraftsCollapsed((value) => !value)}
              >
                <i data-lucide={draftsCollapsed ? "chevron-down" : "chevron-up"}></i>
              </button>
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
                <i data-lucide="list-x"></i>
              </button>
              <label className="flex items-center gap-2 tcp-text-caption uppercase tracking-wide text-slate-500">
                <span>Per page</span>
                <select
                  className="tcp-input h-8 min-w-[4rem] px-2 text-xs leading-none"
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
          {draftsCollapsed ? null : draftsQuery.isError ? (
            <QueryError error={draftsQuery.error} />
          ) : null}
          {draftsCollapsed ? null : draftsQuery.isLoading ? (
            <EmptyState title="Loading drafts" text="Checking generated issue drafts." />
          ) : drafts.length ? (
            <div className="grid gap-3">
              {pagedDrafts.map((draft, index) => {
                const draftId = draftIdOf(draft, draftPageStart + index);
                const matchedIssue = firstString(draft, ["matched_issue_number", "issue_number"]);
                const draftIssueUrl = githubRepoIssueUrl(draft);
                const draftCommentUrl = githubCommentUrl(draft);
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
                          <div className="mt-1"><IdChip value={draftId} /></div>
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
                              window.prompt("Approval reason", "Approved from Incident Monitor.") ||
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
                              window.prompt("Denial reason", "Denied from Incident Monitor.") ||
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
                        onClick={() => draftMutation.mutate({ action: "route-preview", draft })}
                      >
                        <i data-lucide="route"></i>
                        Route preview
                      </button>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        disabled={draftMutation.isPending}
                        onClick={() => {
                          const defaultDestinationIds = destinations
                            .map((destination, destinationIndex) =>
                              firstString(
                                destination,
                                ["destination_id", "destinationId", "id"],
                                `destination-${destinationIndex + 1}`
                              )
                            )
                            .filter(Boolean)
                            .join(", ");
                          const rawDestinationIds = window.prompt(
                            `Destination ids, comma-separated. Leave blank for route/default. Configured: ${
                              defaultDestinationIds || "none"
                            }`,
                            ""
                          );
                          if (rawDestinationIds === null) return;
                          const destinationIds = rawDestinationIds
                            .split(/[\s,]+/)
                            .map((value) => value.trim())
                            .filter(Boolean);
                          const reason =
                            window.prompt("Publish reason", "Published from Incident Monitor.") ||
                            undefined;
                          draftMutation.mutate({
                            action: "publish",
                            draft,
                            destinationIds,
                            reason,
                          });
                        }}
                      >
                        <i data-lucide="shield-alert"></i>
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
                      {draftIssueUrl ? (
                        <a
                          className="tcp-btn h-8 px-3 text-xs"
                          href={draftIssueUrl}
                          target="_blank"
                          rel="noreferrer"
                        >
                          <i data-lucide="external-link"></i>
                          {githubIssueLabel(draft)}
                        </a>
                      ) : null}
                      {draftCommentUrl ? (
                        <a
                          className="tcp-btn h-8 px-3 text-xs"
                          href={draftCommentUrl}
                          target="_blank"
                          rel="noreferrer"
                        >
                          <i data-lucide="message-square"></i>
                          GitHub comment
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
              <Badge tone={posts.length ? "info" : "ghost"}>{posts.length}</Badge>
              <button
                type="button"
                className="tcp-icon-btn"
                title={postsCollapsed ? "Expand posts" : "Collapse posts"}
                aria-label={postsCollapsed ? "Expand posts" : "Collapse posts"}
                onClick={() => setPostsCollapsed((value) => !value)}
              >
                <i data-lucide={postsCollapsed ? "chevron-down" : "chevron-up"}></i>
              </button>
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
                <i data-lucide="list-x"></i>
              </button>
              <label className="flex items-center gap-2 tcp-text-caption uppercase tracking-wide text-slate-500">
                <span>Destination</span>
                <select
                  className="tcp-input h-8 min-w-[10rem] px-2 text-xs leading-none"
                  value={postDestinationFilter}
                  onChange={(event) => setPostDestinationFilter(event.currentTarget.value)}
                >
                  <option value="">All</option>
                  {destinations.map((destination, index) => {
                    const destinationId = firstString(
                      destination,
                      ["destination_id", "destinationId", "id"],
                      `destination-${index + 1}`
                    );
                    return (
                      <option key={destinationId} value={destinationId}>
                        {firstString(destination, ["name"], destinationId)}
                      </option>
                    );
                  })}
                </select>
              </label>
              <label className="flex items-center gap-2 tcp-text-caption uppercase tracking-wide text-slate-500">
                <span>Per page</span>
                <select
                  className="tcp-input h-8 min-w-[4rem] px-2 text-xs leading-none"
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
          {postsCollapsed ? null : postsQuery.isError ? (
            <QueryError error={postsQuery.error} />
          ) : null}
          {postsCollapsed ? null : postsQuery.isLoading ? (
            <EmptyState title="Loading posts" text="Checking GitHub publishing attempts." />
          ) : posts.length ? (
            <div className="grid gap-3">
              {pagedPosts.map((post, index) => {
                const postId = postIdOf(post, postPageStart + index);
                const issueUrl = githubRepoIssueUrl(post);
                const commentUrl = githubCommentUrl(post);
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
                          <div className="mt-1"><IdChip value={postId} /></div>
                        </div>
                      </div>
                      <Badge tone={statusTone(post.status)}>
                        {String(post.status || "unknown")}
                      </Badge>
                    </div>
                    <div className="tcp-subtle mt-2 grid gap-1 text-xs">
                      <div>Repo: {firstString(post, ["repo"], "unset")}</div>
                      <div>
                        Destination:{" "}
                        {firstString(post, ["destination_id", "destinationId"], "legacy-github")}
                      </div>
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
                      {issueUrl ? (
                        <a
                          className="tcp-btn h-8 px-3 text-xs"
                          href={issueUrl}
                          target="_blank"
                          rel="noreferrer"
                        >
                          <i data-lucide="external-link"></i>
                          {githubIssueLabel(post)}
                        </a>
                      ) : null}
                      {commentUrl ? (
                        <a
                          className="tcp-btn h-8 px-3 text-xs"
                          href={commentUrl}
                          target="_blank"
                          rel="noreferrer"
                        >
                          <i data-lucide="message-square"></i>
                          GitHub comment
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
        title={detail?.title || "Incident Monitor Detail"}
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

          <div className="tcp-subtle text-xs font-semibold uppercase tracking-[0.2em]">Basics</div>
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
            <label className="grid gap-1">
              <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Labels</span>
              <input
                className="tcp-input"
                value={form.labels}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, labels: event.currentTarget.value }))
                }
                placeholder="incident-monitor, regression"
              />
            </label>
          </div>

          <div className="tcp-subtle mt-2 text-xs font-semibold uppercase tracking-[0.2em]">
            Classification
          </div>
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
            <label className="grid gap-1">
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
                <option value="incident_monitor_issue_draft">Incident Monitor issue draft</option>
                <option value="triage_only">Triage only</option>
                <option value="research_brief">Research brief</option>
                <option value="workflow_proposal">Workflow proposal</option>
                <option value="product_opportunity">Product opportunity</option>
              </select>
            </label>
          </div>

          <details className="mt-2 grid gap-3">
            <summary className="tcp-subtle cursor-pointer text-xs font-semibold uppercase tracking-[0.2em]">
              Evidence &amp; reproduction (optional)
            </summary>
            <div className="grid gap-3 md:grid-cols-2">
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
                <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                  Related run ID
                </span>
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
          </details>
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
