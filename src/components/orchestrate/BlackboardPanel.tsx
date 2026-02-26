import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowRight,
  GitBranch,
  Maximize2,
  Minimize2,
  MoveUpRight,
  Route,
  Search,
} from "lucide-react";
import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";
import {
  deriveIndicators,
  extractWhyNextFromEvents,
  filterProjectedNodes,
  isDecisionEventType,
  projectNodes,
  type ProjectionNode,
  type ProjectionNodeKind,
} from "./blackboardProjection";
import {
  applyEsc,
  pauseFollowOnManualNavigation,
  reconcileSelection,
  shouldAutoFocusOnDecision,
  toggleExpand,
  toggleFullscreen,
  type BlackboardPanelMode,
} from "./blackboardPanelState";
import { closeDrawersOnEsc, openDriftDrawerIfNeeded } from "./blackboardUiState";
import type {
  Blackboard,
  RunCheckpointSummary,
  RunEventRecord,
  RunReplaySummary,
  RunStatus,
  Task,
} from "./types";

interface BlackboardPanelProps {
  runId: string | null;
  runStatus?: RunStatus | null;
  tasks: Task[];
  events: RunEventRecord[];
  blackboard: Blackboard | null;
  whyNextStep?: string | null;
  replay?: RunReplaySummary | null;
  checkpoint?: RunCheckpointSummary | null;
  className?: string;
}

type ExpandedView = "spine" | "lineage_rail";

const SEARCHABLE_KINDS: Array<ProjectionNodeKind | "all"> = [
  "all",
  "decision",
  "task_sync",
  "reliability",
  "memory",
  "checkpoint",
];

function nodeKindClass(kind: ProjectionNodeKind): string {
  if (kind === "decision") return "border-cyan-500/40 bg-cyan-500/10 text-cyan-200";
  if (kind === "memory") return "border-emerald-500/40 bg-emerald-500/10 text-emerald-200";
  if (kind === "task_sync") return "border-indigo-500/40 bg-indigo-500/10 text-indigo-200";
  if (kind === "reliability") return "border-red-500/40 bg-red-500/10 text-red-200";
  return "border-amber-500/40 bg-amber-500/10 text-amber-200";
}

function isRecentRelevantEvent(event: RunEventRecord): boolean {
  const normalized = event.type.trim().toLowerCase();
  return (
    isDecisionEventType(normalized) ||
    normalized === "todo_synced" ||
    normalized === "workspace_mismatch" ||
    normalized === "task_started" ||
    normalized === "task_completed" ||
    normalized === "task_trace" ||
    normalized === "run_failed" ||
    normalized.includes("loop") ||
    normalized.includes("escalated")
  );
}

function findNearestNodeBySeq(nodes: ProjectionNode[], seq: number): ProjectionNode | null {
  if (!nodes.length) return null;
  let nearest: ProjectionNode | null = null;
  let nearestDistance = Number.MAX_SAFE_INTEGER;
  for (const node of nodes) {
    if (!node.seq) continue;
    const distance = Math.abs(node.seq - seq);
    if (distance < nearestDistance) {
      nearestDistance = distance;
      nearest = node;
    }
  }
  return nearest ?? nodes[0];
}

export function BlackboardPanel({
  runId,
  runStatus,
  tasks,
  events,
  blackboard,
  whyNextStep,
  replay,
  checkpoint,
  className,
}: BlackboardPanelProps) {
  const [mode, setMode] = useState<BlackboardPanelMode>("docked");
  const [expandedView, setExpandedView] = useState<ExpandedView>("spine");
  const [followLatest, setFollowLatest] = useState(true);
  const [showFollowPausedBadge, setShowFollowPausedBadge] = useState(false);
  const [kindFilter, setKindFilter] = useState<ProjectionNodeKind | "all">("all");
  const [query, setQuery] = useState("");
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [showDriftDrawer, setShowDriftDrawer] = useState(false);
  const [expandNowWhy, setExpandNowWhy] = useState(false);
  const [nodeListScrollTop, setNodeListScrollTop] = useState(0);

  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const fullscreenRef = useRef<HTMLDivElement | null>(null);
  const lastAutoFocusedDecisionSeqRef = useRef(0);

  const derivedWhyNext = whyNextStep ?? extractWhyNextFromEvents(events);
  const indicators = useMemo(
    () => deriveIndicators(runStatus, tasks, events, replay, checkpoint),
    [runStatus, tasks, events, replay, checkpoint]
  );
  const currentTask = useMemo(
    () => tasks.find((task) => task.state === "in_progress") ?? null,
    [tasks]
  );

  const recentEvents = useMemo(
    () => [...events].filter(isRecentRelevantEvent).slice(-5).reverse(),
    [events]
  );

  const lineage = useMemo(
    () =>
      events
        .filter((event) => isDecisionEventType(event.type))
        .slice(-20)
        .reverse(),
    [events]
  );

  const projectedNodes = useMemo(
    () => projectNodes(events, blackboard, checkpoint ?? null),
    [events, blackboard, checkpoint]
  );

  const filteredNodes = useMemo(
    () => filterProjectedNodes(projectedNodes, kindFilter, query),
    [projectedNodes, kindFilter, query]
  );

  const selectedNode = useMemo(() => {
    const resolvedId = reconcileSelection(selectedNodeId, filteredNodes);
    if (!resolvedId) return null;
    return filteredNodes.find((node) => node.id === resolvedId) ?? null;
  }, [filteredNodes, selectedNodeId]);

  const newestDecision = useMemo(
    () => projectedNodes.find((node) => node.kind === "decision") ?? null,
    [projectedNodes]
  );

  const decisionSpine = useMemo(
    () => filteredNodes.filter((node) => node.kind === "decision"),
    [filteredNodes]
  );

  const attachmentsByParent = useMemo(() => {
    const grouped = new Map<string, ProjectionNode[]>();
    for (const node of filteredNodes) {
      if (node.kind === "decision") continue;
      const parent = node.parentId;
      if (!parent) continue;
      const rows = grouped.get(parent) ?? [];
      rows.push(node);
      grouped.set(parent, rows);
    }
    for (const rows of grouped.values()) {
      rows.sort((a, b) => b.seq - a.seq || b.tsMs - a.tsMs);
    }
    return grouped;
  }, [filteredNodes]);

  const listVirtualization = useMemo(() => {
    const rowHeight = 74;
    const viewportRows = 6;
    const start = Math.max(0, Math.floor(nodeListScrollTop / rowHeight) - 2);
    const end = Math.min(filteredNodes.length, start + viewportRows + 6);
    return {
      rowHeight,
      start,
      end,
      topSpacer: start * rowHeight,
      bottomSpacer: Math.max(0, (filteredNodes.length - end) * rowHeight),
      visible: filteredNodes.slice(start, end),
    };
  }, [filteredNodes, nodeListScrollTop]);

  const setNodeSelection = useCallback(
    (nodeId: string, source: "auto" | "manual") => {
      setSelectedNodeId(nodeId);
      if (source === "manual") {
        const nextFollow = pauseFollowOnManualNavigation(followLatest);
        if (nextFollow !== followLatest) {
          setFollowLatest(nextFollow);
          setShowFollowPausedBadge(true);
          window.setTimeout(() => setShowFollowPausedBadge(false), 1800);
        }
      }
    },
    [followLatest]
  );

  const jumpToWorkspaceMismatch = () => {
    const reliabilityNode =
      projectedNodes.find(
        (node) => node.kind === "reliability" && node.eventType === "workspace_mismatch"
      ) ??
      projectedNodes.find((node) => node.kind === "reliability") ??
      null;
    if (!reliabilityNode) return;
    if (mode === "docked") setMode("expanded");
    setNodeSelection(reliabilityNode.id, "auto");
  };

  const jumpToCheckpoint = () => {
    if (!checkpoint) return;
    const nearest = findNearestNodeBySeq(projectedNodes, checkpoint.seq);
    if (!nearest) return;
    setNodeSelection(nearest.id, "auto");
  };

  const copyDebugBundle = async () => {
    if (!runId || !replay) return;
    const payload = {
      run_id: runId,
      replay,
      checkpoint_seq: checkpoint?.seq ?? replay.checkpoint_seq ?? null,
      last_event_seq: events.length > 0 ? events[events.length - 1].seq : null,
      drift: replay.drift,
      selected_node_id: selectedNodeId,
    };
    await window.navigator.clipboard.writeText(JSON.stringify(payload, null, 2));
  };

  useEffect(() => {
    const newestSeq = newestDecision?.seq ?? null;
    if (
      shouldAutoFocusOnDecision(followLatest, newestSeq, lastAutoFocusedDecisionSeqRef.current) &&
      newestDecision
    ) {
      window.requestAnimationFrame(() => {
        setNodeSelection(newestDecision.id, "auto");
      });
      lastAutoFocusedDecisionSeqRef.current = newestDecision.seq;
      return;
    }
    if (newestSeq !== null) {
      lastAutoFocusedDecisionSeqRef.current = Math.max(
        lastAutoFocusedDecisionSeqRef.current,
        newestSeq
      );
    }
  }, [newestDecision, followLatest, setNodeSelection]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!runId) return;
      const target = event.target as HTMLElement | null;
      const isTypingTarget =
        !!target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable);

      if (event.key === "Escape") {
        setMode((prev) => applyEsc(prev));
        setShowDriftDrawer((prev) => closeDrawersOnEsc(prev));
        return;
      }

      if (isTypingTarget) return;

      if (event.key.toLowerCase() === "e") {
        event.preventDefault();
        setMode((prev) => toggleExpand(prev));
      }
      if (event.key.toLowerCase() === "f") {
        event.preventDefault();
        setMode((prev) => toggleFullscreen(prev));
      }
      if (event.key === " ") {
        event.preventDefault();
        setFollowLatest((prev) => !prev);
      }
      if (event.key === "/") {
        event.preventDefault();
        searchInputRef.current?.focus();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [runId]);

  useEffect(() => {
    if (mode !== "fullscreen") return;
    const root = fullscreenRef.current;
    if (!root) return;
    const focusables = root.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    );
    focusables[0]?.focus();

    const trap = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const rows = Array.from(
        root.querySelectorAll<HTMLElement>(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
        )
      ).filter((row) => !row.hasAttribute("disabled"));
      if (!rows.length) return;
      const first = rows[0];
      const last = rows[rows.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      }
    };

    root.addEventListener("keydown", trap);
    return () => root.removeEventListener("keydown", trap);
  }, [mode]);

  const nowBlock = (
    <div className="rounded border border-border/60 bg-surface p-2">
      <div className="mb-1 text-[10px] uppercase tracking-wide text-text-subtle">Now</div>
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className="rounded border border-border px-2 py-0.5 text-text">
          {runStatus ?? "unknown"}
        </span>
        <span className="rounded border border-border px-2 py-0.5 text-text">
          {currentTask ? `step ${currentTask.id}` : "no active step"}
        </span>
      </div>
      <p
        title={derivedWhyNext ?? ""}
        className={cn(
          "mt-2 text-xs text-text whitespace-pre-wrap",
          mode === "docked" && !expandNowWhy ? "line-clamp-2" : ""
        )}
      >
        {derivedWhyNext ?? "No decision rationale recorded yet."}
      </p>
      {mode === "docked" && (derivedWhyNext?.length ?? 0) > 120 ? (
        <button
          type="button"
          aria-label="Toggle why-next-step text"
          onClick={() => setExpandNowWhy((prev) => !prev)}
          className="mt-1 text-[11px] text-primary underline-offset-2 hover:underline"
        >
          {expandNowWhy ? "less" : "more"}
        </button>
      ) : null}
    </div>
  );

  const alertsBlock =
    indicators.showAwaitingApproval ||
    indicators.hasWorkspaceMismatch ||
    indicators.showReplayDrift ? (
      <div className="flex flex-wrap items-center gap-2 text-xs">
        {indicators.showAwaitingApproval ? (
          <span className="rounded border border-amber-500/40 bg-amber-500/10 px-2 py-1 text-amber-200">
            awaiting approval
          </span>
        ) : null}
        {indicators.hasWorkspaceMismatch ? (
          <button
            type="button"
            aria-label="Jump to workspace mismatch"
            onClick={jumpToWorkspaceMismatch}
            className="rounded border border-red-500/40 bg-red-500/10 px-2 py-1 text-red-200"
          >
            workspace mismatch
          </button>
        ) : null}
        {indicators.showReplayDrift ? (
          <button
            type="button"
            aria-label="Open drift details"
            onClick={() => setShowDriftDrawer(openDriftDrawerIfNeeded(indicators.showReplayDrift))}
            className="rounded border border-red-500/40 bg-red-500/10 px-2 py-1 text-red-200"
          >
            drift detected
          </button>
        ) : null}
      </div>
    ) : null;

  const dockedView = (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className="rounded border border-border px-2 py-0.5 text-text">
          steps: {indicators.doneCount}/{tasks.length}
        </span>
        <span className="rounded border border-border px-2 py-0.5 text-yellow-200">
          blocked: {indicators.blockedCount}
        </span>
        <span className="rounded border border-border px-2 py-0.5 text-red-200">
          failed: {indicators.failedCount}
        </span>
        {indicators.checkpointSeq !== null ? (
          <button
            type="button"
            aria-label="Jump to latest checkpoint"
            onClick={jumpToCheckpoint}
            className="rounded border border-cyan-500/40 bg-cyan-500/10 px-2 py-0.5 text-cyan-200"
          >
            checkpoint @{indicators.checkpointSeq}
          </button>
        ) : null}
      </div>
      {nowBlock}
      {alertsBlock}
      <div className="rounded border border-border/60 bg-surface p-2">
        <div className="mb-1 text-[10px] uppercase tracking-wide text-text-subtle">Recent</div>
        {recentEvents.length === 0 ? (
          <p className="text-xs text-text-muted">No relevant events yet.</p>
        ) : (
          <div className="space-y-1">
            {recentEvents.map((event) => (
              <div
                key={event.event_id}
                className="rounded border border-border bg-surface-elevated px-2 py-1 text-xs text-text"
              >
                #{event.seq} {event.type}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );

  const expandedViewContent = (
    <div className="space-y-3">
      <div className="grid gap-2 lg:grid-cols-2">
        {nowBlock}
        <div className="rounded border border-border/60 bg-surface p-2">
          <div className="mb-1 text-[10px] uppercase tracking-wide text-text-subtle">Controls</div>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              aria-label="Toggle follow latest"
              onClick={() => setFollowLatest((prev) => !prev)}
              className={cn(
                "rounded border px-2 py-1 text-[11px]",
                followLatest
                  ? "border-primary/40 bg-primary/10 text-primary"
                  : "border-border text-text-muted"
              )}
            >
              {followLatest ? "Follow on" : "Follow off"}
            </button>
            {showFollowPausedBadge ? (
              <span className="rounded border border-amber-500/40 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-200">
                Follow paused
              </span>
            ) : null}
            <button
              type="button"
              aria-label="Switch to spine view"
              onClick={() => setExpandedView("spine")}
              className={cn(
                "rounded border px-2 py-1 text-[11px]",
                expandedView === "spine"
                  ? "border-primary/40 bg-primary/10 text-primary"
                  : "border-border text-text-muted"
              )}
            >
              Spine
            </button>
            <button
              type="button"
              aria-label="Switch to lineage rail view"
              onClick={() => setExpandedView("lineage_rail")}
              className={cn(
                "rounded border px-2 py-1 text-[11px]",
                expandedView === "lineage_rail"
                  ? "border-primary/40 bg-primary/10 text-primary"
                  : "border-border text-text-muted"
              )}
            >
              Lineage rail
            </button>
            {checkpoint ? (
              <button
                type="button"
                aria-label="Jump to checkpoint marker"
                onClick={jumpToCheckpoint}
                className="rounded border border-cyan-500/40 bg-cyan-500/10 px-2 py-1 text-[11px] text-cyan-200"
              >
                Checkpoint @ seq {checkpoint.seq}
              </button>
            ) : null}
          </div>
          <div className="mt-2 text-[11px] text-text-muted">
            Shortcuts: E expand, F fullscreen, Space follow, / search, Esc exit
          </div>
        </div>
      </div>

      {alertsBlock}

      <div className="rounded border border-border/60 bg-surface p-2">
        <div className="mb-2 flex flex-wrap items-center gap-2">
          {SEARCHABLE_KINDS.map((kind) => (
            <button
              key={kind}
              type="button"
              aria-label={`Filter ${kind}`}
              onClick={() => setKindFilter(kind)}
              className={cn(
                "rounded border px-2 py-1 text-[10px]",
                kindFilter === kind
                  ? "border-primary/40 bg-primary/10 text-primary"
                  : "border-border text-text-muted"
              )}
            >
              {kind}
            </button>
          ))}
          <div className="relative ml-auto w-full max-w-xs">
            <Search className="pointer-events-none absolute left-2 top-1.5 h-3.5 w-3.5 text-text-muted" />
            <input
              ref={searchInputRef}
              aria-label="Search blackboard nodes"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search step, event, why..."
              className="w-full rounded border border-border bg-surface-elevated py-1 pl-7 pr-2 text-xs text-text placeholder:text-text-muted focus:border-primary focus:outline-none"
            />
          </div>
        </div>

        {expandedView === "lineage_rail" ? (
          <div className="max-h-80 space-y-1 overflow-y-auto">
            {lineage.length === 0 ? (
              <p className="text-xs text-text-muted">No decisions yet.</p>
            ) : (
              lineage.map((event) => {
                const linkedNode = projectedNodes.find(
                  (node) => node.sourceEventId === event.event_id
                );
                const why = event.payload?.why_next_step;
                const whyText = typeof why === "string" && why.trim().length > 0 ? why : "<none>";
                return (
                  <button
                    key={event.event_id}
                    type="button"
                    aria-label={`Select decision ${event.seq}`}
                    onClick={() => linkedNode && setNodeSelection(linkedNode.id, "manual")}
                    className={cn(
                      "w-full rounded border p-2 text-left",
                      linkedNode && selectedNodeId === linkedNode.id
                        ? "border-primary/40 bg-primary/10"
                        : "border-border bg-surface-elevated"
                    )}
                  >
                    <div className="flex items-center justify-between gap-2 text-[11px]">
                      <span className="rounded border border-cyan-500/40 bg-cyan-500/10 px-1.5 py-0.5 text-cyan-200">
                        #{event.seq}
                      </span>
                      <span className="text-text">{event.step_id ?? "run"}</span>
                    </div>
                    <div className="mt-1 text-xs text-text-muted line-clamp-1">{whyText}</div>
                  </button>
                );
              })
            )}
          </div>
        ) : (
          <div className="space-y-2">
            {decisionSpine.length === 0 ? (
              <p className="text-xs text-text-muted">
                No decision spine nodes match current filter.
              </p>
            ) : (
              decisionSpine.map((decision, index) => {
                const attachments = attachmentsByParent.get(decision.id) ?? [];
                return (
                  <div
                    key={decision.id}
                    className="rounded border border-border/70 bg-surface-elevated p-2"
                  >
                    <button
                      type="button"
                      aria-label={`Select spine decision ${decision.seq}`}
                      onClick={() => setNodeSelection(decision.id, "manual")}
                      className={cn(
                        "w-full rounded border p-2 text-left",
                        selectedNodeId === decision.id
                          ? "border-primary/40 bg-primary/10"
                          : "border-cyan-500/30 bg-cyan-500/5"
                      )}
                    >
                      <div className="flex items-center justify-between text-xs">
                        <span className="rounded border border-cyan-500/40 bg-cyan-500/10 px-2 py-0.5 text-cyan-200">
                          {index + 1}
                        </span>
                        <span className="text-text-muted">seq #{decision.seq}</span>
                      </div>
                      <div className="mt-1 text-sm text-text">{decision.stepId ?? "run"}</div>
                      <div className="mt-1 text-xs text-text-muted line-clamp-2">
                        {decision.whyNextStep ?? decision.label}
                      </div>
                    </button>
                    {attachments.length > 0 ? (
                      <div className="mt-2 space-y-1 pl-5">
                        {attachments.slice(0, 8).map((node) => (
                          <button
                            key={node.id}
                            type="button"
                            aria-label={`Select attached ${node.kind} node`}
                            onClick={() => setNodeSelection(node.id, "manual")}
                            className={cn(
                              "flex w-full items-start gap-2 rounded border p-2 text-left",
                              selectedNodeId === node.id
                                ? "border-primary/40 bg-primary/10"
                                : "border-border bg-surface"
                            )}
                          >
                            <ArrowRight className="mt-0.5 h-3 w-3 text-text-muted" />
                            <div className="min-w-0">
                              <div className="text-[11px] text-text">
                                <span
                                  className={cn(
                                    "rounded border px-1 py-0.5",
                                    nodeKindClass(node.kind)
                                  )}
                                >
                                  {node.kind}
                                </span>{" "}
                                <span className="text-text-muted">#{node.seq || "-"}</span>
                              </div>
                              <div className="mt-1 truncate text-xs text-text-muted">
                                {node.label}
                              </div>
                            </div>
                          </button>
                        ))}
                      </div>
                    ) : null}
                  </div>
                );
              })
            )}
          </div>
        )}
      </div>

      <div className="grid gap-2 xl:grid-cols-2">
        <div
          className="max-h-80 overflow-y-auto rounded border border-border/60 bg-surface p-2"
          onScroll={(event) => setNodeListScrollTop((event.target as HTMLDivElement).scrollTop)}
        >
          <div className="mb-1 text-[10px] uppercase tracking-wide text-text-subtle">Node List</div>
          <div
            style={{
              paddingTop: listVirtualization.topSpacer,
              paddingBottom: listVirtualization.bottomSpacer,
            }}
          >
            {listVirtualization.visible.map((node) => (
              <button
                key={node.id}
                type="button"
                aria-label={`Select node ${node.id}`}
                onClick={() => setNodeSelection(node.id, "manual")}
                className={cn(
                  "mb-1 w-full rounded border p-2 text-left",
                  selectedNodeId === node.id
                    ? "border-primary/40 bg-primary/10"
                    : "border-border bg-surface-elevated"
                )}
                style={{ minHeight: `${listVirtualization.rowHeight - 8}px` }}
              >
                <div className="flex items-center justify-between gap-2 text-[11px]">
                  <span className={cn("rounded border px-1.5 py-0.5", nodeKindClass(node.kind))}>
                    {node.kind}
                  </span>
                  <span className="text-text-muted">#{node.seq || "-"}</span>
                </div>
                <div className="mt-1 truncate text-xs text-text">{node.label}</div>
                {node.parentId ? (
                  <div className="mt-1 flex items-center gap-1 text-[11px] text-text-muted">
                    <GitBranch className="h-3 w-3" />
                    {node.parentId}
                  </div>
                ) : null}
              </button>
            ))}
          </div>
        </div>
        <div className="rounded border border-border/60 bg-surface p-2">
          <div className="mb-1 text-[10px] uppercase tracking-wide text-text-subtle">Inspector</div>
          {!selectedNode ? (
            <p className="text-xs text-text-muted">Select a projected node.</p>
          ) : (
            <div className="space-y-1 text-xs">
              <div>
                <span className="text-text-muted">kind:</span> {selectedNode.kind}
              </div>
              <div>
                <span className="text-text-muted">label:</span> {selectedNode.label}
              </div>
              <div>
                <span className="text-text-muted">seq:</span> {selectedNode.seq || "-"}
              </div>
              <div>
                <span className="text-text-muted">step:</span> {selectedNode.stepId ?? "-"}
              </div>
              <div>
                <span className="text-text-muted">event:</span> {selectedNode.eventType ?? "-"}
              </div>
              <div>
                <span className="text-text-muted">why:</span> {selectedNode.whyNextStep ?? "n/a"}
              </div>
              <pre className="mt-2 max-h-52 overflow-auto rounded border border-border/50 bg-surface-elevated p-2 text-[11px] text-text-muted whitespace-pre-wrap break-all">
                {JSON.stringify(selectedNode.payload ?? {}, null, 2)}
              </pre>
            </div>
          )}
        </div>
      </div>
    </div>
  );

  const panelBody = (
    <div className={cn("rounded-lg border border-border bg-surface-elevated/40 p-3", className)}>
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div className="text-[10px] uppercase tracking-wide text-text-subtle">Blackboard</div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            aria-label="Toggle follow latest"
            onClick={() => setFollowLatest((prev) => !prev)}
            className={cn(
              "rounded border px-2 py-1 text-[10px]",
              followLatest
                ? "border-primary/40 bg-primary/10 text-primary"
                : "border-border text-text-muted"
            )}
          >
            {followLatest ? "Follow on" : "Follow off"}
          </button>
          {mode === "docked" ? (
            <Button
              size="sm"
              variant="secondary"
              aria-label="Expand blackboard"
              onClick={() => setMode((prev) => toggleExpand(prev))}
            >
              <MoveUpRight className="mr-1 h-3.5 w-3.5" />
              Expand
            </Button>
          ) : (
            <Button
              size="sm"
              variant="secondary"
              aria-label="Dock blackboard"
              onClick={() => setMode((prev) => toggleExpand(prev))}
            >
              <Minimize2 className="mr-1 h-3.5 w-3.5" />
              Dock
            </Button>
          )}
          <Button
            size="sm"
            variant="secondary"
            aria-label="Toggle fullscreen blackboard"
            onClick={() => setMode((prev) => toggleFullscreen(prev))}
          >
            <Maximize2 className="mr-1 h-3.5 w-3.5" />
            Fullscreen
          </Button>
        </div>
      </div>

      {!runId ? (
        <p className="text-xs text-text-muted">Select a run to view blackboard context.</p>
      ) : mode === "docked" ? (
        dockedView
      ) : (
        expandedViewContent
      )}

      {showDriftDrawer && replay ? (
        <div className="fixed inset-y-0 right-0 z-[80] w-full max-w-md border-l border-border bg-surface p-3 shadow-2xl">
          <div className="mb-3 flex items-center justify-between">
            <div className="text-sm font-semibold text-text">Drift Details</div>
            <Button
              size="sm"
              variant="secondary"
              aria-label="Close drift details"
              onClick={() => setShowDriftDrawer(false)}
            >
              Close
            </Button>
          </div>
          <div className="space-y-2 text-xs">
            <div className="rounded border border-border p-2">
              <div>run_id: {runId}</div>
              <div>checkpoint_seq: {checkpoint?.seq ?? replay.checkpoint_seq ?? "-"}</div>
              <div>last_event_seq: {events.length > 0 ? events[events.length - 1].seq : "-"}</div>
            </div>
            <div className="rounded border border-border p-2">
              <div className="font-medium text-text">Flags</div>
              <div className="mt-1 text-text-muted">
                status_mismatch: {String(replay.drift.status_mismatch)}
              </div>
              <div className="text-text-muted">
                why_next_step_mismatch: {String(replay.drift.why_next_step_mismatch)}
              </div>
              <div className="text-text-muted">
                step_count_mismatch: {String(replay.drift.step_count_mismatch)}
              </div>
            </div>
            <Button
              size="sm"
              onClick={() => void copyDebugBundle()}
              aria-label="Copy drift debug bundle"
            >
              Copy debug bundle
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );

  if (mode !== "fullscreen") {
    return panelBody;
  }

  return (
    <div
      ref={fullscreenRef}
      role="dialog"
      aria-modal="true"
      className="fixed inset-4 z-[70] overflow-auto rounded-xl border border-border bg-surface shadow-2xl"
    >
      <div className="sticky top-0 z-10 flex items-center justify-between border-b border-border bg-surface p-2">
        <div className="flex items-center gap-2 text-xs text-text-muted">
          <Route className="h-3.5 w-3.5" />
          Blackboard fullscreen
        </div>
        <Button
          size="sm"
          variant="secondary"
          aria-label="Exit fullscreen"
          onClick={() => setMode("expanded")}
        >
          <Minimize2 className="mr-1 h-3.5 w-3.5" />
          Exit fullscreen
        </Button>
      </div>
      <div className="p-3">{panelBody}</div>
    </div>
  );
}
