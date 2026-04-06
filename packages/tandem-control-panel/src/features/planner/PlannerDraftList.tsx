import { useEffect, useMemo, useState } from "react";
import { Badge, PanelCard } from "../../ui/index.tsx";
import { PlannerMetricGrid, PlannerSubsection } from "./plannerPrimitives";
import { listNamedPlannerDrafts, listPlannerDraftHistory } from "./plannerDraftStorage";
import { renderIcons } from "../../app/icons.js";

function safeString(value: unknown) {
  return String(value || "").trim();
}

function formatRelativeTime(updatedAtMs: number) {
  if (!updatedAtMs) return "unknown";
  const diffMs = Date.now() - updatedAtMs;
  const minutes = Math.max(1, Math.round(diffMs / 60000));
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return `${days}d ago`;
}

export function PlannerDraftList({
  storagePrefix,
  historyKey,
  draftName,
  onDraftNameChange,
  onSaveCurrentDraft,
  onOpenDraft,
  onDeleteDraft,
  onOpenHistory,
  onDeleteHistory,
}: {
  storagePrefix: string;
  historyKey: string;
  draftName: string;
  onDraftNameChange: (value: string) => void;
  onSaveCurrentDraft: () => void;
  onOpenDraft: (storageKey: string) => void;
  onDeleteDraft: (storageKey: string) => void;
  onOpenHistory: (entryId: string) => void;
  onDeleteHistory: (entryId: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [showAutosaves, setShowAutosaves] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  });

  const drafts = listNamedPlannerDrafts(storagePrefix);
  const history = listPlannerDraftHistory(historyKey);
  const normalizedQuery = safeString(query).toLowerCase();
  const filteredDrafts = useMemo(() => {
    if (!normalizedQuery) return drafts;
    return drafts.filter((draft) =>
      [
        draft.name,
        draft.planId,
        draft.planRevision,
        draft.briefGoal,
        draft.targetSurface,
        draft.planningHorizon,
      ]
        .join(" ")
        .toLowerCase()
        .includes(normalizedQuery)
    );
  }, [drafts, normalizedQuery]);
  const filteredHistory = useMemo(() => {
    if (!normalizedQuery) return history;
    return history.filter((entry) =>
      [
        entry.name,
        entry.planId,
        entry.planRevision,
        entry.briefGoal,
        entry.targetSurface,
        entry.planningHorizon,
      ]
        .join(" ")
        .toLowerCase()
        .includes(normalizedQuery)
    );
  }, [history, normalizedQuery]);

  return (
    <PanelCard
      title="Saved intents"
      subtitle="Open, inspect, and reopen saved planning work from local storage."
    >
      <div className="grid gap-3 text-sm">
        <div className="flex flex-wrap gap-2">
          <Badge tone={drafts.length ? "ok" : "warn"}>
            {drafts.length
              ? `${drafts.length} saved draft${drafts.length === 1 ? "" : "s"}`
              : "no saved drafts"}
          </Badge>
          <Badge tone={history.length ? "info" : "warn"}>
            {history.length ? `${history.length} autosave snapshots` : "no autosave snapshots"}
          </Badge>
        </div>

        <input
          className="tcp-input"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Filter saved drafts and autosaves"
        />

        <PlannerSubsection title="Save current draft">
          <div className="grid gap-2">
            <input
              className="tcp-input"
              value={draftName}
              onChange={(event) => onDraftNameChange(event.target.value)}
              placeholder="Name this intent"
            />
            <button type="button" className="tcp-btn-primary" onClick={onSaveCurrentDraft}>
              Save named intent
            </button>
          </div>
        </PlannerSubsection>

        {filteredDrafts.length ? (
          <div className="grid gap-2">
            {filteredDrafts.slice(0, 8).map((draft) => (
              <PlannerSubsection key={draft.storageKey} title={draft.name}>
                <div className="mb-2 flex flex-wrap gap-2">
                  <Badge tone="ok">{draft.targetSurface || "planner"}</Badge>
                  <Badge tone="info">{draft.planningHorizon || "unspecified horizon"}</Badge>
                  <Badge tone={draft.hasOverlapMatch ? "warn" : "ok"}>
                    {draft.hasOverlapMatch ? "overlap match" : "no overlap"}
                  </Badge>
                </div>
                <PlannerMetricGrid
                  metrics={[
                    {
                      label: "Updated",
                      value: draft.updatedAtMs
                        ? new Date(draft.updatedAtMs).toLocaleString()
                        : "unknown",
                      detail: formatRelativeTime(draft.updatedAtMs),
                    },
                    {
                      label: "Plan",
                      value: draft.planId || "pending",
                    },
                    {
                      label: "Revision",
                      value: draft.planRevision || "pending",
                    },
                    {
                      label: "Brief",
                      value: (
                        <span
                          className="block text-xs line-clamp-3"
                          title={draft.briefGoal || "no saved goal"}
                        >
                          {draft.briefGoal || "no saved goal"}
                        </span>
                      ),
                    },
                    {
                      label: "Conversation",
                      value: `${draft.messageCount} turn${draft.messageCount === 1 ? "" : "s"}`,
                    },
                    {
                      label: "Validation",
                      value: `${draft.blockerCount} blocker${draft.blockerCount === 1 ? "" : "s"}`,
                      detail: `${draft.warningCount} warning${draft.warningCount === 1 ? "" : "s"}`,
                    },
                  ]}
                  columns="grid-cols-1 sm:grid-cols-2"
                />
                <div className="mt-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="tcp-btn"
                    onClick={() => onOpenDraft(draft.storageKey)}
                  >
                    <i data-lucide="folder-open" className="mr-1 h-3 w-3"></i>
                    Reopen draft
                  </button>

                  {confirmDelete === draft.storageKey ? (
                    <div className="flex items-center gap-2 rounded border border-red-500/20 bg-red-500/10 px-2 py-1">
                      <span className="text-xs text-red-400">Confirm delete?</span>
                      <button
                        type="button"
                        className="tcp-btn bg-red-500/20 text-red-300 border-red-500/50 hover:bg-red-500/30"
                        onClick={() => {
                          onDeleteDraft(draft.storageKey);
                          setConfirmDelete(null);
                        }}
                      >
                        <i data-lucide="trash-2" className="mr-1 h-3 w-3"></i>
                        Confirm
                      </button>
                      <button
                        type="button"
                        className="tcp-btn"
                        onClick={() => setConfirmDelete(null)}
                      >
                        Cancel
                      </button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      className="tcp-btn"
                      onClick={() => setConfirmDelete(draft.storageKey)}
                    >
                      <i data-lucide="trash-2" className="mr-1 h-3 w-3"></i>
                      Delete
                    </button>
                  )}
                </div>
              </PlannerSubsection>
            ))}
          </div>
        ) : (
          <div className="tcp-subtle text-xs">
            Save a planner draft and it will appear here for quick reopening.
          </div>
        )}

        <PlannerSubsection title="Recent autosaves">
          {!showAutosaves ? (
            <button
              type="button"
              className="tcp-btn w-full justify-start text-xs"
              onClick={() => setShowAutosaves(true)}
            >
              <i data-lucide="chevron-down" className="mr-1 h-3 w-3"></i>
              Show {history.length} autosaved snapshots
            </button>
          ) : filteredHistory.length ? (
            <div className="grid gap-2">
              {filteredHistory.slice(0, 8).map((entry) => (
                <div
                  key={entry.entryId}
                  className="rounded-lg border border-white/10 bg-black/20 p-3"
                >
                  <div className="mb-2 flex flex-wrap gap-2">
                    <Badge tone="info">{entry.targetSurface || "planner"}</Badge>
                    <Badge tone="info">{entry.planningHorizon || "unspecified horizon"}</Badge>
                    <Badge tone={entry.hasOverlapMatch ? "warn" : "ok"}>
                      {entry.hasOverlapMatch ? "overlap match" : "no overlap"}
                    </Badge>
                  </div>
                  <PlannerMetricGrid
                    metrics={[
                      {
                        label: "Updated",
                        value: entry.updatedAtMs
                          ? new Date(entry.updatedAtMs).toLocaleString()
                          : "unknown",
                        detail: formatRelativeTime(entry.updatedAtMs),
                      },
                      {
                        label: "Plan",
                        value: entry.planId || "pending",
                      },
                      {
                        label: "Revision",
                        value: entry.planRevision || "pending",
                      },
                      {
                        label: "Brief",
                        value: (
                          <span
                            className="block text-xs line-clamp-3"
                            title={entry.briefGoal || "no saved goal"}
                          >
                            {entry.briefGoal || "no saved goal"}
                          </span>
                        ),
                      },
                      {
                        label: "Conversation",
                        value: `${entry.messageCount} turn${entry.messageCount === 1 ? "" : "s"}`,
                      },
                      {
                        label: "Validation",
                        value: `${entry.blockerCount} blocker${entry.blockerCount === 1 ? "" : "s"}`,
                        detail: `${entry.warningCount} warning${entry.warningCount === 1 ? "" : "s"}`,
                      },
                    ]}
                    columns="grid-cols-1 sm:grid-cols-2"
                  />
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      type="button"
                      className="tcp-btn"
                      onClick={() => onOpenHistory(entry.entryId)}
                    >
                      <i data-lucide="folder-open" className="mr-1 h-3 w-3"></i>
                      Reopen autosave
                    </button>

                    {confirmDelete === entry.entryId ? (
                      <div className="flex items-center gap-2 rounded border border-red-500/20 bg-red-500/10 px-2 py-1">
                        <span className="text-xs text-red-400">Confirm delete?</span>
                        <button
                          type="button"
                          className="tcp-btn bg-red-500/20 text-red-300 border-red-500/50 hover:bg-red-500/30"
                          onClick={() => {
                            onDeleteHistory(entry.entryId);
                            setConfirmDelete(null);
                          }}
                        >
                          <i data-lucide="trash-2" className="mr-1 h-3 w-3"></i>
                          Confirm
                        </button>
                        <button
                          type="button"
                          className="tcp-btn"
                          onClick={() => setConfirmDelete(null)}
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <button
                        type="button"
                        className="tcp-btn"
                        onClick={() => setConfirmDelete(entry.entryId)}
                      >
                        <i data-lucide="trash-2" className="mr-1 h-3 w-3"></i>
                        Delete
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="tcp-subtle text-xs">
              {normalizedQuery
                ? "No autosaves match the current filter."
                : "Recent autosaved planner states will appear here as the plan evolves."}
            </div>
          )}
        </PlannerSubsection>
      </div>
    </PanelCard>
  );
}
