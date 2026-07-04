import { Badge, PanelCard } from "../ui/index.tsx";
import { toArray } from "./CodingWorkflowsHelpers";

type RepoContext = {
  source?: string;
  fallback_used?: boolean;
  artifact_path?: string;
  path_scope?: string;
  index_source?: string;
  index_status?: string;
  index_error?: string;
  error?: string;
};

type PartialDiffArtifact = {
  worker_id?: string;
  subtask_id?: string;
  patch_path?: string;
};

type DiagnosticBadge = { label: string; tone: "ok" | "warn" | "err" | "info" };

function repoGraphWasUsed(repoContext: RepoContext | null): boolean {
  return (
    !!repoContext &&
    repoContext.source === "repo.context_bundle" &&
    repoContext.fallback_used !== true
  );
}

/**
 * TAN-276 (Codex follow-up): a run's preserved partial diffs aren't
 * guaranteed to be surfaced as a top-level `partial_diff_artifacts` field —
 * ACA carries them on individual events' payloads instead, either as a
 * `partial_diff_artifacts` list (an aggregate/summary event) or a single
 * `partial_diff_artifact` path (a per-worker timeout/retry event). Collect
 * from both the top-level field (defensive, in case it's ever populated
 * directly) and every event's payload, de-duplicating by patch path.
 */
function collectPartialDiffArtifacts(runDetail: any): PartialDiffArtifact[] {
  const artifacts: PartialDiffArtifact[] = [];
  const seen = new Set<string>();

  const pushArtifact = (candidate: PartialDiffArtifact) => {
    const patchPath = candidate.patch_path;
    if (!patchPath) return;
    const key = `${candidate.worker_id || ""}|${candidate.subtask_id || ""}|${patchPath}`;
    if (seen.has(key)) return;
    seen.add(key);
    artifacts.push(candidate);
  };

  toArray(runDetail, "partial_diff_artifacts").forEach((artifact: any) => {
    if (artifact && typeof artifact === "object") {
      pushArtifact({
        worker_id: artifact.worker_id,
        subtask_id: artifact.subtask_id,
        patch_path: artifact.patch_path,
      });
    }
  });

  for (const event of toArray(runDetail, "events")) {
    const payload = event && typeof event === "object" ? event.payload : null;
    if (!payload || typeof payload !== "object") continue;
    const workerId = payload.worker_id;
    const subtaskId = payload.subtask_id;
    if (typeof payload.partial_diff_artifact === "string" && payload.partial_diff_artifact.trim()) {
      pushArtifact({
        worker_id: workerId,
        subtask_id: subtaskId,
        patch_path: payload.partial_diff_artifact,
      });
    }
    toArray(payload, "partial_diff_artifacts").forEach((artifact: any) => {
      if (artifact && typeof artifact === "object") {
        pushArtifact({
          worker_id: artifact.worker_id || workerId,
          subtask_id: artifact.subtask_id || subtaskId,
          patch_path: artifact.patch_path,
        });
      }
    });
  }

  return artifacts;
}

/**
 * TAN-276: badges for a single run event's worker-retry / prompt-timeout /
 * partial-diff-recovery outcome, so those facts are visible in the timeline
 * without opening the raw run JSON. ACA's compact event payload (see
 * `_compact_event_payload` in tandem-agents' `api/main.py`) is the source of
 * these fields.
 */
export function runEventDiagnosticBadges(event: any): DiagnosticBadge[] {
  const payload = event && typeof event === "object" ? event.payload || {} : {};
  const badges: DiagnosticBadge[] = [];
  if (payload.will_retry === true) {
    badges.push({ label: "Will retry", tone: "warn" });
  } else if (payload.will_retry === false && (payload.failure_reason || payload.blocker_kind)) {
    badges.push({ label: "No retry", tone: "err" });
  }
  if (payload.failure_reason) {
    badges.push({ label: `Failure: ${String(payload.failure_reason)}`, tone: "err" });
  }
  if (payload.blocker_kind) {
    badges.push({ label: `Blocked: ${String(payload.blocker_kind)}`, tone: "warn" });
  }
  if (payload.partial_diff_state) {
    const state = String(payload.partial_diff_state);
    badges.push({
      label: `Partial diff: ${state}`,
      tone: state === "accepted" ? "ok" : "warn",
    });
  }
  return badges;
}

/**
 * TAN-276: surfaces ACA's repo-graph usage and partial-diff artifacts for a
 * coder run, using fields already present on the proxied `/api/aca/runs/{id}`
 * payload (`repo_context`, plus `partial_diff_artifacts`/`partial_diff_artifact`
 * carried on individual events) that the run detail view previously fetched
 * but never rendered.
 */
export function CodingWorkflowsRunDiagnosticsPanel({ runDetail }: { runDetail: any }) {
  const repoContext: RepoContext | null =
    runDetail?.repo_context && typeof runDetail.repo_context === "object"
      ? runDetail.repo_context
      : null;
  const partialDiffArtifacts = collectPartialDiffArtifacts(runDetail);

  if (!repoContext && !partialDiffArtifacts.length) return null;

  const usedGraph = repoGraphWasUsed(repoContext);

  return (
    <div className="grid gap-3 lg:grid-cols-2">
      {repoContext ? (
        <PanelCard title="Repo graph">
          <div className="grid gap-2 text-xs leading-5">
            <div className="flex justify-between gap-3">
              <span className="tcp-subtle">Source</span>
              <Badge tone={usedGraph ? "ok" : "warn"}>
                {usedGraph
                  ? "repo.context_bundle"
                  : repoContext.fallback_used
                    ? "Fallback"
                    : String(repoContext.source || "unknown")}
              </Badge>
            </div>
            {repoContext.path_scope ? (
              <div className="flex justify-between gap-3">
                <span className="tcp-subtle">Path scope</span>
                <code className="max-w-[70%] truncate text-right text-slate-200">
                  {repoContext.path_scope}
                </code>
              </div>
            ) : null}
            {repoContext.index_status ? (
              <div className="flex justify-between gap-3">
                <span className="tcp-subtle">Index status</span>
                <span className="text-right text-slate-200">{repoContext.index_status}</span>
              </div>
            ) : null}
            {repoContext.index_error || repoContext.error ? (
              <div className="flex justify-between gap-3">
                <span className="tcp-subtle">Error</span>
                <span className="max-w-[70%] text-right text-red-300">
                  {repoContext.index_error || repoContext.error}
                </span>
              </div>
            ) : null}
            {repoContext.artifact_path ? (
              <div className="flex justify-between gap-3">
                <span className="tcp-subtle">Artifact</span>
                <code className="max-w-[70%] truncate text-right text-slate-200">
                  {repoContext.artifact_path}
                </code>
              </div>
            ) : null}
          </div>
        </PanelCard>
      ) : null}
      {partialDiffArtifacts.length ? (
        <PanelCard title="Partial diff artifacts">
          <div className="grid gap-2">
            {partialDiffArtifacts.map((artifact, index) => (
              <div
                key={`${artifact.worker_id || "worker"}-${artifact.subtask_id || index}`}
                className="rounded-lg border border-white/10 bg-black/20 px-3 py-2 text-xs"
              >
                <div className="font-semibold text-slate-100">
                  {artifact.worker_id || "worker"}
                  {artifact.subtask_id ? ` · ${artifact.subtask_id}` : ""}
                </div>
                {artifact.patch_path ? (
                  <code className="mt-1 block truncate text-slate-200">
                    {artifact.patch_path}
                  </code>
                ) : null}
              </div>
            ))}
          </div>
        </PanelCard>
      ) : null}
    </div>
  );
}
