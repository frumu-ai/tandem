import { api } from "../api";
import { handleCommonRunEvent } from "./liveEventDebug";

interface ToolStartEvent {
  tool: string;
}

interface ToolEndEvent {
  tool: string;
  result: string;
}

export interface PortalRunStreamHandlers {
  addSystemLog: (content: string) => void;
  addTextDelta: (delta: string) => void;
  onToolStart: (event: ToolStartEvent) => void;
  onToolEnd: (event: ToolEndEvent) => void;
  onFinalize: (status: string) => void;
}

export interface PortalRunStreamOptions {
  runTimeoutMs?: number;
}

export const attachPortalRunStream = (
  eventSourceRef: { current: EventSource | null },
  sessionId: string,
  runId: string,
  handlers: PortalRunStreamHandlers,
  options?: PortalRunStreamOptions
): void => {
  if (eventSourceRef.current) {
    eventSourceRef.current.close();
  }

  const source = new EventSource(api.getEventStreamUrl(sessionId, runId));
  eventSourceRef.current = source;
  let finalized = false;
  let sawRunEvent = false;
  const runTimeoutMs = options?.runTimeoutMs;

  const finalize = (status: string) => {
    if (finalized) return;
    finalized = true;
    window.clearTimeout(watchdog);
    if (runTimeout) {
      window.clearTimeout(runTimeout);
    }
    window.clearInterval(runStatePoll);
    handlers.onFinalize(status);
    source.close();
    if (eventSourceRef.current === source) {
      eventSourceRef.current = null;
    }
  };

  const watchdog = window.setTimeout(async () => {
    if (finalized || sawRunEvent) return;
    try {
      const runState = await api.getActiveRun(sessionId);
      if (!runState?.active) {
        handlers.addSystemLog(
          "Run ended before live events arrived. Check provider key/model and engine logs."
        );
        finalize("inactive_no_events");
      } else {
        handlers.addSystemLog(
          "Run is active but no live deltas yet. Waiting for provider/tool output..."
        );
      }
    } catch {
      handlers.addSystemLog("No live events yet and failed to query run state.");
    }
  }, 4000);

  const runTimeout =
    typeof runTimeoutMs === "number" && runTimeoutMs > 0
      ? window.setTimeout(() => {
          handlers.addSystemLog(`Run timeout reached (${runTimeoutMs}ms).`);
          finalize("timeout");
        }, runTimeoutMs)
      : null;

  const runStatePoll = window.setInterval(async () => {
    if (finalized) return;
    try {
      const runState = await api.getActiveRun(sessionId);
      if (!runState?.active) {
        handlers.addSystemLog(
          "Run became inactive without a terminal stream event. Finalizing from poll."
        );
        finalize("inactive");
      }
    } catch {
      // keep attached
    }
  }, 5000);

  source.onmessage = (evt) => {
    try {
      const data = JSON.parse(evt.data);
      if (data.type !== "server.connected" && data.type !== "engine.lifecycle.ready") {
        sawRunEvent = true;
      }

      if (
        handleCommonRunEvent(
          data,
          (event) => handlers.addSystemLog(event.content),
          (status) => finalize(status)
        )
      ) {
        return;
      }

      if (data.type !== "message.part.updated") return;
      const part = data?.properties?.part;
      if (!part) return;

      if (part.type === "tool") {
        const status = part?.state?.status;
        if (status === "running" || status === "in_progress" || status === "pending") {
          handlers.onToolStart({ tool: String(part.tool || "tool") });
          return;
        }
        if (
          status === "completed" ||
          status === "failed" ||
          status === "error" ||
          status === "cancelled" ||
          status === "canceled" ||
          status === "denied"
        ) {
          const rawResult = part?.state?.result ?? part?.state?.output ?? part?.result ?? "";
          const result =
            typeof rawResult === "string" ? rawResult : JSON.stringify(rawResult || {});
          handlers.onToolEnd({ tool: String(part.tool || "tool"), result });
        }
        return;
      }

      const delta = data?.properties?.delta;
      if (part.type === "text" && typeof delta === "string" && delta.length > 0) {
        handlers.addTextDelta(delta);
      }
    } catch {
      handlers.addSystemLog("Failed to parse stream event payload.");
    }
  };

  source.onerror = () => {
    handlers.addSystemLog("Stream disconnected.");
    finalize("stream_error");
  };
};
