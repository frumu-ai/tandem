import { useCallback, useEffect, useRef } from "react";
import { useEngineStream } from "../stream/useEngineStream";
import {
  workflowEventAt,
  workflowEventRunId,
  workflowEventSessionId,
  workflowEventType,
} from "../orchestration/workflowStability";

export function useAutomationRunStreams({
  selectedRunId,
  selectedSessionId,
  selectedContextRunId,
  isWorkflowRun,
  runInspectorActive,
  timestampOrNull,
  appendRunEvent,
  appendSessionEvent,
  queryClient,
}: any) {
  const contextInvalidationRafRef = useRef<number | null>(null);
  const pendingContextInvalidations = useRef<{
    run: boolean;
    blackboard: boolean;
    events: boolean;
    patches: boolean;
  }>({
    run: false,
    blackboard: false,
    events: false,
    patches: false,
  });
  const flushContextInvalidationsRef = useRef<() => void>(() => {});
  flushContextInvalidationsRef.current = () => {
    contextInvalidationRafRef.current = null;
    const pending = pendingContextInvalidations.current;
    pendingContextInvalidations.current = {
      run: false,
      blackboard: false,
      events: false,
      patches: false,
    };
    const tasks: Array<Promise<unknown>> = [];
    if (pending.run && selectedContextRunId) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", "context", selectedContextRunId],
        })
      );
    }
    if (pending.blackboard && selectedContextRunId) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", "context", selectedContextRunId, "blackboard"],
        })
      );
    }
    if (pending.events && selectedContextRunId) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", "context", selectedContextRunId, "events"],
        })
      );
    }
    if (pending.patches && selectedContextRunId) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", "context", selectedContextRunId, "patches"],
        })
      );
    }
    if (pending.run && selectedRunId) {
      tasks.push(
        queryClient.invalidateQueries({
          queryKey: ["automations", "run", selectedRunId],
        })
      );
    }
    if (tasks.length) void Promise.all(tasks);
  };
  const queueContextInvalidation = useCallback(
    (kinds: Partial<{ run: boolean; blackboard: boolean; events: boolean; patches: boolean }>) => {
      const pending = pendingContextInvalidations.current;
      if (kinds.run) pending.run = true;
      if (kinds.blackboard) pending.blackboard = true;
      if (kinds.events) pending.events = true;
      if (kinds.patches) pending.patches = true;
      if (contextInvalidationRafRef.current == null) {
        contextInvalidationRafRef.current = requestAnimationFrame(() =>
          flushContextInvalidationsRef.current()
        );
      }
    },
    []
  );
  useEffect(
    () => () => {
      if (contextInvalidationRafRef.current != null) {
        cancelAnimationFrame(contextInvalidationRafRef.current);
        contextInvalidationRafRef.current = null;
      }
    },
    []
  );

  useEngineStream(
    selectedRunId
      ? isWorkflowRun
        ? `/api/engine/automations/v2/events?run_id=${encodeURIComponent(selectedRunId)}`
        : `/api/engine/automations/events?run_id=${encodeURIComponent(selectedRunId)}`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload || payload.status === "ready") return;
        const runId = workflowEventRunId(payload);
        if (!runId || runId !== selectedRunId) return;
        const type = workflowEventType(payload);
        const at = workflowEventAt(payload);
        const id = `automations:${runId}:${type}:${at}:${Math.random().toString(16).slice(2, 8)}`;
        appendRunEvent({ id, source: "automations", at, event: payload });
        if (type) {
          const kind = String(type).toLowerCase();
          if (kind.includes("artifact") || kind.includes("blackboard") || kind.includes("patch")) {
            queueContextInvalidation({ blackboard: true, patches: true });
          }
          if (kind.includes("task") || kind.includes("node") || kind.endsWith(".updated")) {
            queueContextInvalidation({ run: true });
          }
        }
      } catch {
        return;
      }
    },
    { enabled: runInspectorActive }
  );
  useEngineStream(
    selectedContextRunId
      ? `/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/events/stream?tail=50`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload || payload.status === "ready") return;
        const id = `context:${String(payload?.seq || "")}:${String(payload?.event_type || "")}`;
        const at =
          timestampOrNull(
            payload?.created_at_ms || payload?.timestamp_ms || payload?.timestampMs
          ) || Date.now();
        appendRunEvent({ id, source: "context", at, event: payload });
        const kind = String(payload?.event_type || "").toLowerCase();
        // Context-stream events drive blackboard/patch invalidation. Always
        // refresh the events query (it's small) and conditionally refresh the
        // heavier blackboard payload only when something blackboard-shaped fires.
        const fields: Partial<{
          run: boolean;
          blackboard: boolean;
          events: boolean;
          patches: boolean;
        }> = { events: true };
        if (
          kind.includes("blackboard") ||
          kind.includes("patch") ||
          kind.includes("artifact") ||
          kind.includes("task")
        ) {
          fields.blackboard = true;
          fields.patches = true;
        }
        if (kind.includes("run") || kind.includes("status") || kind.includes("phase")) {
          fields.run = true;
        }
        queueContextInvalidation(fields);
      } catch {
        return;
      }
    },
    { enabled: runInspectorActive && !!selectedContextRunId }
  );
  useEngineStream(
    selectedRunId && selectedSessionId
      ? `/api/engine/event?sessionID=${encodeURIComponent(selectedSessionId)}&runID=${encodeURIComponent(selectedRunId)}`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload) return;
        const type = workflowEventType(payload);
        const at = workflowEventAt(payload);
        const id = [
          type || "event",
          String(payload?.properties?.sessionID || payload?.sessionID || selectedSessionId || ""),
          String(payload?.properties?.runID || payload?.runID || selectedRunId || ""),
          String(payload?.properties?.messageID || payload?.messageID || ""),
          String(
            payload?.properties?.part?.id || payload?.properties?.seq || payload?.timestamp_ms || at
          ),
        ].join(":");
        appendSessionEvent({ id, at, event: payload });
      } catch {
        return;
      }
    },
    { enabled: runInspectorActive && !!selectedSessionId }
  );
  useEngineStream(
    selectedRunId ? "/api/global/event" : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        const runId = workflowEventRunId(payload);
        if (!runId || runId !== selectedRunId) return;
        const type = workflowEventType(payload);
        if (!type || type === "server.connected" || type === "engine.lifecycle.ready") return;
        const at = workflowEventAt(payload);
        const id = `global:${runId}:${type}:${at}:${Math.random().toString(16).slice(2, 8)}`;
        appendRunEvent({ id, source: "global", at, event: payload });
      } catch {
        return;
      }
    },
    { enabled: runInspectorActive }
  );
}
