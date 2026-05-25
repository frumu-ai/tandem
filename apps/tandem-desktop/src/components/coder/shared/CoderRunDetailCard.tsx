import { useEffect, useMemo, useState } from "react";
import { AlertCircle, ExternalLink } from "lucide-react";
import { Button, Card, CardContent } from "@/components/ui";
import type {
  AutomationV2RunRecord,
  Blackboard,
  BlackboardArtifactRef,
  BlackboardPatchRecord,
  CoderRunRecord,
  OrchestratorRunRecord,
  SessionMessage,
} from "@/lib/tauri";
import { readFileText } from "@/lib/tauri";
import { CoderRunActionToolbar } from "./CoderRunActionToolbar";
import { CoderRunStatusBadge } from "./CoderRunStatusBadge";
import { CoderRunProgress } from "./CoderRunProgress";
import {
  extractRunBlockers,
  extractSessionIdsFromRun,
  relativeTimeFromMs,
  runAwaitingGate,
  runProgress,
  runSortTimestamp,
  runSummary,
  type DerivedCoderRun,
  type SessionPreview,
} from "./coderRunUtils";

type CoderRunDetailCardProps = {
  selectedCoderRun: DerivedCoderRun | null;
  selectedCoderRunRecord: CoderRunRecord | null;
  selectedRunDetail: AutomationV2RunRecord | null;
  selectedContextRunId: string | null;
  selectedSessionPreview: SessionPreview | null;
  sessionMessagesBySession: Record<string, SessionMessage[]>;
  selectedContextRun: OrchestratorRunRecord | null;
  selectedContextBlackboard: Blackboard | null;
  selectedContextPatches: BlackboardPatchRecord[];
  selectedContextError: string | null;
  busyKey: string | null;
  onRefreshDetail: (runId: string) => void;
  onRunAction: (runId: string, action: "pause" | "resume" | "cancel" | "recover") => void;
  onGateDecision: (runId: string, decision: "approve" | "rework" | "cancel") => void;
  onOpenAutomationRun?: (runId: string) => void;
  onOpenContextRun?: (runId: string) => void;
};

type DetailTab = "overview" | "transcripts" | "context" | "artifacts" | "memory";

type MemoryProjection = {
  loading: boolean;
  error: string | null;
  hits: Array<Record<string, unknown>>;
  candidates: Array<Record<string, unknown>>;
  sources: BlackboardArtifactRef[];
};

type ArtifactPreviewState = {
  loading: boolean;
  error: string | null;
  content: string;
};

function isMemoryArtifact(artifact: BlackboardArtifactRef) {
  return (
    artifact.artifact_type === "coder_memory_hits" ||
    artifact.artifact_type === "coder_memory_candidate" ||
    artifact.artifact_type === "coder_memory_promotion"
  );
}

function valueText(value: unknown) {
  return typeof value === "string" ? value : "";
}

function artifactMatchesContext(
  artifact: BlackboardArtifactRef,
  target: { stepId?: string | null; sourceEventId?: string | null }
) {
  return Boolean(
    (target.stepId && artifact.step_id === target.stepId) ||
    (target.sourceEventId && artifact.source_event_id === target.sourceEventId)
  );
}

function DetailStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
      <div className="text-[10px] uppercase tracking-wide text-text-subtle">{label}</div>
      <div className="mt-1 break-all text-xs text-text">{value}</div>
    </div>
  );
}

export function CoderRunDetailCard({
  selectedCoderRun,
  selectedCoderRunRecord,
  selectedRunDetail,
  selectedContextRunId,
  selectedSessionPreview,
  sessionMessagesBySession,
  selectedContextRun,
  selectedContextBlackboard,
  selectedContextPatches,
  selectedContextError,
  busyKey,
  onRefreshDetail,
  onRunAction,
  onGateDecision,
  onOpenAutomationRun,
  onOpenContextRun,
}: CoderRunDetailCardProps) {
  const [detailTab, setDetailTab] = useState<DetailTab>("overview");
  const [memoryProjection, setMemoryProjection] = useState<MemoryProjection>({
    loading: false,
    error: null,
    hits: [],
    candidates: [],
    sources: [],
  });
  const [selectedArtifactPath, setSelectedArtifactPath] = useState<string | null>(null);
  const [artifactPreview, setArtifactPreview] = useState<ArtifactPreviewState>({
    loading: false,
    error: null,
    content: "",
  });

  const transcriptSessions = useMemo(
    () => Object.entries(sessionMessagesBySession),
    [sessionMessagesBySession]
  );
  const blackboardArtifacts = useMemo(
    () => selectedContextBlackboard?.artifacts || [],
    [selectedContextBlackboard?.artifacts]
  );
  const effectiveSelectedArtifactPath =
    selectedArtifactPath &&
    blackboardArtifacts.some((artifact) => artifact.path === selectedArtifactPath)
      ? selectedArtifactPath
      : blackboardArtifacts[0]?.path || null;
  const selectedArtifact =
    blackboardArtifacts.find((artifact) => artifact.path === effectiveSelectedArtifactPath) ||
    blackboardArtifacts[0] ||
    null;
  const awaitingGate = useMemo(() => runAwaitingGate(selectedRunDetail), [selectedRunDetail]);
  const runBlockers = useMemo(() => extractRunBlockers(selectedRunDetail), [selectedRunDetail]);

  useEffect(() => {
    let cancelled = false;
    const loadArtifactPreview = async () => {
      if (!selectedArtifact?.path) {
        setArtifactPreview({ loading: false, error: null, content: "" });
        return;
      }
      setArtifactPreview({ loading: true, error: null, content: "" });
      try {
        const content = await readFileText(selectedArtifact.path, 512 * 1024, 200_000);
        if (cancelled) return;
        setArtifactPreview({ loading: false, error: null, content });
      } catch (error) {
        if (cancelled) return;
        setArtifactPreview({
          loading: false,
          error: error instanceof Error ? error.message : String(error),
          content: "",
        });
      }
    };
    void loadArtifactPreview();
    return () => {
      cancelled = true;
    };
  }, [selectedArtifact?.path]);

  useEffect(() => {
    let cancelled = false;
    const loadMemoryProjection = async () => {
      const sources = (selectedContextBlackboard?.artifacts || []).filter(isMemoryArtifact);
      if (sources.length === 0) {
        setMemoryProjection({
          loading: false,
          error: null,
          hits: [],
          candidates: [],
          sources: [],
        });
        return;
      }
      setMemoryProjection({
        loading: true,
        error: null,
        hits: [],
        candidates: [],
        sources,
      });
      try {
        const payloads = await Promise.all(
          sources.map(async (artifact) => {
            const raw = await readFileText(artifact.path, 512 * 1024, 200_000);
            return {
              artifact,
              payload: JSON.parse(raw) as Record<string, unknown>,
            };
          })
        );
        if (cancelled) return;
        const hits = payloads.flatMap(({ artifact, payload }) => {
          if (artifact.artifact_type !== "coder_memory_hits") return [];
          const rows = Array.isArray(payload.hits) ? payload.hits : [];
          return rows
            .filter(
              (row): row is Record<string, unknown> => typeof row === "object" && row !== null
            )
            .map((row) => ({
              ...row,
              artifact_path: artifact.path,
            }));
        });
        const candidates = payloads
          .filter(({ artifact }) => artifact.artifact_type === "coder_memory_candidate")
          .map(({ artifact, payload }) => ({
            ...payload,
            artifact_path: artifact.path,
          }));
        setMemoryProjection({
          loading: false,
          error: null,
          hits,
          candidates,
          sources,
        });
      } catch (error) {
        if (cancelled) return;
        setMemoryProjection({
          loading: false,
          error: error instanceof Error ? error.message : String(error),
          hits: [],
          candidates: [],
          sources,
        });
      }
    };
    void loadMemoryProjection();
    return () => {
      cancelled = true;
    };
  }, [selectedContextBlackboard?.artifacts, selectedRunDetail?.run_id]);

  const detailTabs: Array<{ key: DetailTab; label: string; count?: number }> = [
    { key: "overview", label: "Overview" },
    { key: "transcripts", label: "Transcripts", count: transcriptSessions.length },
    { key: "context", label: "Context", count: selectedContextRun?.tasks.length ?? 0 },
    {
      key: "artifacts",
      label: "Artifacts",
      count: selectedContextBlackboard?.artifacts.length ?? 0,
    },
    {
      key: "memory",
      label: "Memory",
      count: memoryProjection.hits.length + memoryProjection.candidates.length,
    },
  ];

  const progress = useMemo(() => runProgress(selectedRunDetail), [selectedRunDetail]);
  const githubRef = selectedCoderRunRecord?.github_project_ref || null;

  return (
    <Card className="p-4">
      <CardContent className="space-y-4">
        {selectedCoderRun && selectedRunDetail ? (
          <>
            <div className="space-y-2">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="text-lg font-semibold text-text">
                    {selectedCoderRun.automation.name || "Untitled coder run"}
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-text-subtle">
                    <span className="capitalize">
                      {selectedCoderRun.coderMetadata.workflow_kind.replace(/_/g, " ")}
                    </span>
                    <span aria-hidden>·</span>
                    <span>Updated {relativeTimeFromMs(runSortTimestamp(selectedRunDetail))}</span>
                    {progress.total > 0 ? (
                      <>
                        <span aria-hidden>·</span>
                        <span>
                          {progress.completed}/{progress.total} steps
                        </span>
                      </>
                    ) : null}
                  </div>
                </div>
                <CoderRunStatusBadge run={selectedRunDetail} size="md" />
              </div>
              <CoderRunProgress run={selectedRunDetail} />
            </div>

            {awaitingGate ? (
              <div className="rounded-xl border border-amber-300/50 bg-amber-300/10 p-4">
                <div className="flex items-start gap-2 text-amber-100">
                  <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0" aria-hidden />
                  <div className="min-w-0 flex-1">
                    <div className="text-sm font-semibold">
                      Waiting on you:{" "}
                      {String(awaitingGate.title || awaitingGate.node_id || "operator decision")}
                    </div>
                    {String(awaitingGate.instructions || "").trim() ? (
                      <div className="mt-1 text-xs text-amber-100/80">
                        {String(awaitingGate.instructions)}
                      </div>
                    ) : null}
                  </div>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    variant="primary"
                    onClick={() => onGateDecision(selectedRunDetail.run_id, "approve")}
                    disabled={busyKey === `gate:approve:${selectedRunDetail.run_id}`}
                  >
                    Approve & continue
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => onGateDecision(selectedRunDetail.run_id, "rework")}
                    disabled={busyKey === `gate:rework:${selectedRunDetail.run_id}`}
                  >
                    Request rework
                  </Button>
                </div>
              </div>
            ) : null}

            {runBlockers.length ? (
              <div className="space-y-2">
                {runBlockers.slice(0, 3).map((blocker) => (
                  <div
                    key={blocker.key}
                    className="rounded-lg border border-red-500/30 bg-red-500/10 p-3"
                  >
                    <div className="text-sm font-medium text-red-100">{blocker.title}</div>
                    <div className="mt-1 text-xs text-red-200">{blocker.reason}</div>
                  </div>
                ))}
              </div>
            ) : runSummary(selectedRunDetail) ? (
              <div className="rounded-lg border border-border bg-surface-elevated/30 p-3 text-sm text-text-muted">
                {runSummary(selectedRunDetail)}
              </div>
            ) : null}

            <CoderRunActionToolbar
              run={selectedRunDetail}
              busyKey={busyKey}
              onRefresh={() => onRefreshDetail(selectedRunDetail.run_id)}
              onRunAction={onRunAction}
              onGateDecision={onGateDecision}
            />

            <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-subtle">
              {githubRef ? (
                <a
                  href={githubRef.issue_url || "#"}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1 rounded-full border border-border px-2.5 py-1 transition-colors hover:bg-surface hover:text-text"
                >
                  <ExternalLink className="h-3 w-3" aria-hidden />
                  {githubRef.owner} #{githubRef.project_number} · issue #{githubRef.issue_number}
                </a>
              ) : null}
              {selectedRunDetail.run_id && onOpenAutomationRun ? (
                <button
                  type="button"
                  onClick={() => onOpenAutomationRun(selectedRunDetail.run_id)}
                  className="inline-flex items-center gap-1 rounded-full border border-border px-2.5 py-1 transition-colors hover:bg-surface hover:text-text"
                >
                  <ExternalLink className="h-3 w-3" aria-hidden />
                  Agent Automation
                </button>
              ) : null}
              {selectedContextRunId && onOpenContextRun ? (
                <button
                  type="button"
                  onClick={() => onOpenContextRun(selectedContextRunId)}
                  className="inline-flex items-center gap-1 rounded-full border border-border px-2.5 py-1 transition-colors hover:bg-surface hover:text-text"
                >
                  <ExternalLink className="h-3 w-3" aria-hidden />
                  Command Center
                </button>
              ) : null}
              <span className="ml-auto inline-flex items-center gap-3 text-text-subtle">
                <span>Run {selectedRunDetail.run_id.slice(0, 8)}</span>
                {extractSessionIdsFromRun(selectedRunDetail).length > 0 ? (
                  <span>
                    {extractSessionIdsFromRun(selectedRunDetail).length} session
                    {extractSessionIdsFromRun(selectedRunDetail).length === 1 ? "" : "s"}
                  </span>
                ) : null}
              </span>
            </div>

            <div className="flex flex-wrap gap-2">
              {detailTabs.map((tab) => (
                <button
                  key={tab.key}
                  type="button"
                  onClick={() => setDetailTab(tab.key)}
                  className={`rounded-full border px-3 py-1.5 text-xs font-medium transition-colors ${
                    detailTab === tab.key
                      ? "border-primary/40 bg-primary/10 text-primary"
                      : "border-border bg-surface text-text-muted hover:text-text"
                  }`}
                >
                  {tab.label}
                  {typeof tab.count === "number" ? ` (${tab.count})` : ""}
                </button>
              ))}
            </div>

            {detailTab === "overview" ? (
              <div className="grid gap-3 lg:grid-cols-2">
                <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                  <div className="text-sm font-semibold text-text">Run Identifiers</div>
                  <div className="mt-2 space-y-1.5 text-xs text-text-muted">
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-text-subtle">Workspace</span>
                      <span className="truncate font-mono text-[11px] text-text">
                        {selectedCoderRun.automation.workspace_root || "Not set"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-text-subtle">Automation</span>
                      <span className="truncate font-mono text-[11px] text-text">
                        {selectedCoderRun.automation.automation_id || "Unknown"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-text-subtle">Context run</span>
                      <span className="truncate font-mono text-[11px] text-text">
                        {selectedContextRunId || "—"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-text-subtle">Coder record</span>
                      <span className="truncate font-mono text-[11px] text-text">
                        {selectedCoderRunRecord?.coder_run_id || "—"}
                      </span>
                    </div>
                  </div>
                </div>
                <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                  <div className="text-sm font-semibold text-text">Transcript Snapshot</div>
                  {selectedSessionPreview ? (
                    <div className="mt-2 space-y-2 text-xs text-text-muted">
                      <div>Session: {selectedSessionPreview.sessionId}</div>
                      <div>Messages: {selectedSessionPreview.messageCount}</div>
                      <div>
                        {selectedSessionPreview.latestText || "Latest message has no text payload."}
                      </div>
                    </div>
                  ) : (
                    <div className="mt-2 text-xs text-text-muted">
                      No session transcript is linked to the selected run yet.
                    </div>
                  )}
                </div>
              </div>
            ) : null}

            {detailTab === "transcripts" ? (
              <div className="space-y-3">
                {transcriptSessions.length > 0 ? (
                  transcriptSessions.map(([sessionId, messages]) => (
                    <div
                      key={sessionId}
                      className="rounded-xl border border-border bg-surface-elevated/20 p-3"
                    >
                      <div className="text-sm font-semibold text-text">{sessionId}</div>
                      <div className="mt-1 text-xs text-text-muted">{messages.length} messages</div>
                      <div className="mt-3 max-h-72 space-y-2 overflow-y-auto">
                        {messages.slice(-8).map((message) => (
                          <div
                            key={message.info.id}
                            className="rounded-lg border border-border bg-surface/60 p-2"
                          >
                            <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                              {message.info.role}
                            </div>
                            <div className="mt-1 whitespace-pre-wrap text-xs text-text-muted">
                              {(message.parts || [])
                                .map((part) =>
                                  typeof part === "object" && part !== null
                                    ? String(
                                        (part as Record<string, unknown>).text ||
                                          (part as Record<string, unknown>).content ||
                                          ""
                                      )
                                    : ""
                                )
                                .join("\n")
                                .trim() || "No text payload"}
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  ))
                ) : (
                  <div className="rounded-lg border border-dashed border-border bg-surface-elevated/20 px-4 py-8 text-center text-sm text-text-muted">
                    No linked session transcripts are available for this run.
                  </div>
                )}
              </div>
            ) : null}

            {detailTab === "context" ? (
              <div className="space-y-3">
                {selectedContextError ? (
                  <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                    {selectedContextError}
                  </div>
                ) : null}
                <div className="grid gap-3 lg:grid-cols-2">
                  <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                    <div className="text-sm font-semibold text-text">Why Next Step</div>
                    <div className="mt-2 whitespace-pre-wrap text-xs text-text-muted">
                      {selectedContextRun?.why_next_step || "No current step rationale available."}
                    </div>
                  </div>
                  <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                    <div className="text-sm font-semibold text-text">Blackboard Summary</div>
                    <div className="mt-2 space-y-2 text-xs text-text-muted">
                      <div>Revision: {selectedContextBlackboard?.revision ?? 0}</div>
                      <div>Facts: {selectedContextBlackboard?.facts.length ?? 0}</div>
                      <div>Decisions: {selectedContextBlackboard?.decisions.length ?? 0}</div>
                      <div>
                        Open Questions: {selectedContextBlackboard?.open_questions.length ?? 0}
                      </div>
                      <div>Patches: {selectedContextPatches.length}</div>
                    </div>
                  </div>
                </div>
                <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                  <div className="text-sm font-semibold text-text">Tasks</div>
                  {selectedContextRun?.tasks?.length ? (
                    <div className="mt-3 space-y-2">
                      {selectedContextRun.tasks.map((task) => (
                        <div
                          key={task.id}
                          className="rounded-lg border border-border bg-surface/60 p-2 text-xs text-text-muted"
                        >
                          <div className="font-medium text-text">{task.title}</div>
                          <div className="mt-1">
                            {task.state}
                            {task.assigned_role ? ` • ${task.assigned_role}` : ""}
                          </div>
                          {blackboardArtifacts.some((artifact) =>
                            artifactMatchesContext(artifact, { stepId: task.id })
                          ) ? (
                            <button
                              type="button"
                              onClick={() => {
                                const match = blackboardArtifacts.find((artifact) =>
                                  artifactMatchesContext(artifact, { stepId: task.id })
                                );
                                if (!match) return;
                                setSelectedArtifactPath(match.path);
                                setDetailTab("artifacts");
                              }}
                              className="mt-2 rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-wide text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                            >
                              Open related artifact
                            </button>
                          ) : null}
                          {task.runtime_detail ? (
                            <div className="mt-1">{task.runtime_detail}</div>
                          ) : null}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="mt-2 text-xs text-text-muted">
                      No context task projection is available for this run.
                    </div>
                  )}
                </div>
                <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                  <div className="text-sm font-semibold text-text">Recent Blackboard Patches</div>
                  {selectedContextPatches.length > 0 ? (
                    <div className="mt-3 space-y-2">
                      {selectedContextPatches.slice(0, 10).map((patch) => (
                        <div
                          key={patch.patch_id}
                          className="rounded-lg border border-border bg-surface/60 p-2 text-xs text-text-muted"
                        >
                          <div className="font-medium text-text">
                            {patch.op} • seq {patch.seq}
                          </div>
                          {(() => {
                            const payloadPath = valueText(
                              (patch.payload as Record<string, unknown>)?.path
                            );
                            const match = blackboardArtifacts.find(
                              (artifact) =>
                                artifact.path === payloadPath ||
                                valueText(
                                  (patch.payload as Record<string, unknown>)?.artifact_path
                                ) === artifact.path
                            );
                            if (!match) return null;
                            return (
                              <button
                                type="button"
                                onClick={() => {
                                  setSelectedArtifactPath(match.path);
                                  setDetailTab("artifacts");
                                }}
                                className="mt-2 rounded-full border border-border px-2 py-1 text-[10px] uppercase tracking-wide text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                              >
                                Open artifact
                              </button>
                            );
                          })()}
                          <div className="mt-1 whitespace-pre-wrap break-words">
                            {JSON.stringify(patch.payload, null, 2)}
                          </div>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="mt-2 text-xs text-text-muted">
                      No recent blackboard patches are available.
                    </div>
                  )}
                </div>
              </div>
            ) : null}

            {detailTab === "artifacts" ? (
              <div className="space-y-3">
                {blackboardArtifacts.length ? (
                  <div className="grid gap-3 xl:grid-cols-[320px_minmax(0,1fr)]">
                    <div className="space-y-2">
                      {blackboardArtifacts.map((artifact) => {
                        const selected = artifact.path === selectedArtifact?.path;
                        return (
                          <button
                            key={artifact.id}
                            type="button"
                            onClick={() => setSelectedArtifactPath(artifact.path)}
                            className={`w-full rounded-xl border p-3 text-left transition-colors ${
                              selected
                                ? "border-primary bg-primary/10"
                                : "border-border bg-surface-elevated/20 hover:bg-surface-elevated/40"
                            }`}
                          >
                            <div className="text-sm font-semibold text-text">
                              {artifact.artifact_type}
                            </div>
                            <div className="mt-1 break-all text-xs text-text-muted">
                              {artifact.path}
                            </div>
                            <div className="mt-2 text-[11px] text-text-subtle">
                              {artifact.step_id ? `step ${artifact.step_id}` : "no step linkage"}
                              {artifact.source_event_id
                                ? ` • event ${artifact.source_event_id}`
                                : ""}
                            </div>
                          </button>
                        );
                      })}
                    </div>
                    <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                      <div className="flex flex-wrap items-center justify-between gap-3">
                        <div>
                          <div className="text-sm font-semibold text-text">
                            {selectedArtifact?.artifact_type || "Artifact preview"}
                          </div>
                          <div className="mt-1 break-all text-xs text-text-muted">
                            {selectedArtifact?.path || "Select an artifact"}
                          </div>
                        </div>
                        {selectedArtifact?.step_id || selectedArtifact?.source_event_id ? (
                          <div className="text-[11px] text-text-subtle">
                            {selectedArtifact?.step_id ? `step ${selectedArtifact.step_id}` : ""}
                            {selectedArtifact?.step_id && selectedArtifact?.source_event_id
                              ? " • "
                              : ""}
                            {selectedArtifact?.source_event_id
                              ? `event ${selectedArtifact.source_event_id}`
                              : ""}
                          </div>
                        ) : null}
                      </div>
                      {artifactPreview.loading ? (
                        <div className="mt-3 text-sm text-text-muted">
                          Loading artifact preview...
                        </div>
                      ) : artifactPreview.error ? (
                        <div className="mt-3 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                          {artifactPreview.error}
                        </div>
                      ) : (
                        <pre className="mt-3 max-h-[560px] overflow-auto rounded-2xl border border-border bg-surface/60 p-3 text-[11px] text-text-muted">
                          {artifactPreview.content || "Artifact file is empty."}
                        </pre>
                      )}
                    </div>
                  </div>
                ) : (
                  <div className="rounded-lg border border-dashed border-border bg-surface-elevated/20 px-4 py-8 text-center text-sm text-text-muted">
                    No linked context artifacts are available yet for this run.
                  </div>
                )}
              </div>
            ) : null}

            {detailTab === "memory" ? (
              <div className="space-y-3">
                {memoryProjection.error ? (
                  <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                    {memoryProjection.error}
                  </div>
                ) : null}
                {memoryProjection.loading ? (
                  <div className="rounded-lg border border-border bg-surface px-4 py-8 text-center text-sm text-text-muted">
                    Loading linked memory artifacts...
                  </div>
                ) : memoryProjection.hits.length === 0 &&
                  memoryProjection.candidates.length === 0 ? (
                  <div className="rounded-lg border border-dashed border-border bg-surface-elevated/20 px-4 py-8 text-center text-sm text-text-muted">
                    No linked memory hits or candidates have been written for this run yet.
                  </div>
                ) : (
                  <>
                    <div className="grid gap-3 md:grid-cols-3">
                      <DetailStat
                        label="Hit Records"
                        value={String(memoryProjection.hits.length)}
                      />
                      <DetailStat
                        label="Candidates"
                        value={String(memoryProjection.candidates.length)}
                      />
                      <DetailStat
                        label="Memory Artifacts"
                        value={String(memoryProjection.sources.length)}
                      />
                    </div>

                    {memoryProjection.hits.length > 0 ? (
                      <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                        <div className="text-sm font-semibold text-text">Memory Hits</div>
                        <div className="mt-3 space-y-2">
                          {memoryProjection.hits.slice(0, 8).map((hit, index) => (
                            <div
                              key={`${valueText(hit.memory_id) || valueText(hit.run_id) || "hit"}-${index}`}
                              className="rounded-lg border border-border bg-surface/60 p-2 text-xs text-text-muted"
                            >
                              <div className="font-medium text-text">
                                {valueText(hit.subject) ||
                                  valueText(hit.kind) ||
                                  valueText(hit.memory_id) ||
                                  "Memory hit"}
                              </div>
                              <div className="mt-1">
                                {valueText(hit.summary) ||
                                  valueText(hit.content) ||
                                  valueText(hit.source) ||
                                  "No summary available"}
                              </div>
                              <div className="mt-1 text-[11px] text-text-subtle">
                                {valueText(hit.memory_id)
                                  ? `memory ${valueText(hit.memory_id)} • `
                                  : ""}
                                {valueText(hit.run_id) ? `run ${valueText(hit.run_id)} • ` : ""}
                                {typeof hit.score === "number" ? `score ${hit.score}` : ""}
                              </div>
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : null}

                    {memoryProjection.candidates.length > 0 ? (
                      <div className="rounded-xl border border-border bg-surface-elevated/20 p-3">
                        <div className="text-sm font-semibold text-text">Memory Candidates</div>
                        <div className="mt-3 space-y-2">
                          {memoryProjection.candidates.slice(0, 8).map((candidate, index) => (
                            <div
                              key={`${valueText(candidate.candidate_id) || "candidate"}-${index}`}
                              className="rounded-lg border border-border bg-surface/60 p-2 text-xs text-text-muted"
                            >
                              <div className="font-medium text-text">
                                {valueText(candidate.kind) || "candidate"}
                              </div>
                              <div className="mt-1">
                                {valueText(candidate.summary) ||
                                  valueText(
                                    typeof candidate.payload === "object" && candidate.payload
                                      ? (candidate.payload as Record<string, unknown>).summary
                                      : ""
                                  ) ||
                                  "No summary available"}
                              </div>
                              <div className="mt-1 text-[11px] text-text-subtle">
                                {valueText(candidate.candidate_id)}
                              </div>
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : null}
                  </>
                )}
              </div>
            ) : null}
          </>
        ) : (
          <div className="flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-border bg-surface-elevated/20 px-4 py-16 text-center">
            <div className="text-sm font-medium text-text">No run selected</div>
            <div className="text-xs text-text-muted">
              Pick a run from the list to see live status, transcripts, artifacts, and operator
              controls.
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
