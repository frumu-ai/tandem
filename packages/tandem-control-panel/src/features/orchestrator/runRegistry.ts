import { useEffect, useMemo, useReducer } from "react";

type AnyRun = Record<string, any>;

type RunRegistryState = {
  runsById: Record<string, AnyRun>;
  orderedRunIds: string[];
  selectedRunId: string;
  cursorsByRunId: Record<string, { eventSeq: number; patchSeq: number }>;
};

type SyncRunsAction = {
  type: "sync_runs";
  runs: AnyRun[];
  preferredRunId?: string;
};

type SelectRunAction = {
  type: "select_run";
  runId: string;
};

type ClearSelectionAction = {
  type: "clear_selection";
};

type AdvanceCursorAction = {
  type: "advance_cursor";
  runId: string;
  kind: string;
  seq: number;
};

type RunRegistryAction =
  | SyncRunsAction
  | SelectRunAction
  | ClearSelectionAction
  | AdvanceCursorAction;

function runIdFromRecord(run: AnyRun, index = 0) {
  return String(run?.run_id || run?.runId || `run-${index}`).trim();
}

function runTimestamp(run: AnyRun) {
  const updated = Number(run?.updated_at_ms || run?.updatedAtMs || 0);
  const created = Number(run?.created_at_ms || run?.createdAtMs || 0);
  const value = Number.isFinite(updated) && updated > 0 ? updated : created;
  return Number.isFinite(value) && value > 0 ? value : 0;
}

function chooseFallbackRunId(runs: AnyRun[], preferredRunId = "") {
  const preferred = String(preferredRunId || "").trim();
  const ordered = [...(Array.isArray(runs) ? runs : [])].sort(
    (a, b) => runTimestamp(b) - runTimestamp(a)
  );
  if (preferred) {
    const preferredRun = ordered.find((run, index) => runIdFromRecord(run, index) === preferred);
    if (preferredRun) {
      const preferredStatus = String(preferredRun?.status || "")
        .trim()
        .toLowerCase();
      if (!["completed", "failed", "cancelled"].includes(preferredStatus)) {
        return preferred;
      }
    }
  }
  const active = ordered.find((run) => {
    const status = String(run?.status || "")
      .trim()
      .toLowerCase();
    return !["completed", "failed", "cancelled"].includes(status);
  });
  const fallback = active || ordered[0];
  return fallback ? runIdFromRecord(fallback) : "";
}

function reduceRunRegistry(state: RunRegistryState, action: RunRegistryAction): RunRegistryState {
  if (action.type === "sync_runs") {
    const byId: Record<string, AnyRun> = {};
    const ids: string[] = [];
    for (let index = 0; index < action.runs.length; index += 1) {
      const run = action.runs[index];
      const id = runIdFromRecord(run, index);
      if (!id) continue;
      byId[id] = run;
      ids.push(id);
    }
    let nextSelected = state.selectedRunId;
    if (!nextSelected) {
      nextSelected = chooseFallbackRunId(action.runs, action.preferredRunId);
    }
    return {
      runsById: byId,
      orderedRunIds: ids,
      selectedRunId: nextSelected,
      cursorsByRunId: state.cursorsByRunId,
    };
  }

  if (action.type === "select_run") {
    return {
      ...state,
      selectedRunId: String(action.runId || "").trim(),
    };
  }

  if (action.type === "clear_selection") {
    return {
      ...state,
      selectedRunId: "",
    };
  }

  if (action.type === "advance_cursor") {
    const runId = String(action.runId || "").trim();
    if (!runId) return state;
    const seq = Number(action.seq || 0);
    if (!Number.isFinite(seq) || seq <= 0) return state;
    const kind = String(action.kind || "")
      .trim()
      .toLowerCase();
    const current = state.cursorsByRunId[runId] || { eventSeq: 0, patchSeq: 0 };
    const next = {
      eventSeq: kind === "context_run_event" ? Math.max(current.eventSeq, seq) : current.eventSeq,
      patchSeq: kind === "blackboard_patch" ? Math.max(current.patchSeq, seq) : current.patchSeq,
    };
    if (next.eventSeq === current.eventSeq && next.patchSeq === current.patchSeq) return state;
    return {
      ...state,
      cursorsByRunId: {
        ...state.cursorsByRunId,
        [runId]: next,
      },
    };
  }

  return state;
}

export function useRunRegistry(runs: AnyRun[], preferredRunId = "") {
  const [state, dispatch] = useReducer(reduceRunRegistry, {
    runsById: {},
    orderedRunIds: [],
    selectedRunId: "",
    cursorsByRunId: {},
  });

  useEffect(() => {
    dispatch({
      type: "sync_runs",
      runs: Array.isArray(runs) ? runs : [],
      preferredRunId,
    });
  }, [runs, preferredRunId]);

  const orderedRuns = useMemo(
    () => state.orderedRunIds.map((id) => state.runsById[id]).filter(Boolean),
    [state.orderedRunIds, state.runsById]
  );

  return {
    runsById: state.runsById,
    orderedRunIds: state.orderedRunIds,
    orderedRuns,
    selectedRunId: state.selectedRunId,
    cursorsByRunId: state.cursorsByRunId,
    setSelectedRunId: (runId: string) => dispatch({ type: "select_run", runId }),
    clearSelectedRunId: () => dispatch({ type: "clear_selection" }),
    advanceCursor: (runId: string, kind: string, seq: number) =>
      dispatch({ type: "advance_cursor", runId, kind, seq }),
  };
}
