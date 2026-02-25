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
  reconnectMaxAttempts?: number;
  reconnectBaseDelayMs?: number;
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

  let activeRunId = runId;
  let source: EventSource | null = null;
  let finalized = false;
  let sawRunEvent = false;
  let streamedAssistantText = "";
  const textSnapshots = new Map<string, string>();
  const runTimeoutMs = options?.runTimeoutMs;
  const reconnectMaxAttempts = Math.max(1, options?.reconnectMaxAttempts ?? 8);
  const reconnectBaseDelayMs = Math.max(500, options?.reconnectBaseDelayMs ?? 1200);
  let reconnectAttempts = 0;
  let reconnectTimer: number | null = null;
  let runTimeout: number | null = null;

  const hydrateFinalAssistantText = async () => {
    try {
      const messages = await api.getSessionMessages(sessionId);
      let latestAssistantText = "";
      for (let i = messages.length - 1; i >= 0; i--) {
        const msg = messages[i];
        if (msg.info?.role !== "assistant") continue;
        const text = (msg.parts || [])
          .filter((p) => p.type === "text" && p.text)
          .map((p) => p.text)
          .join("\n")
          .trim();
        if (text) {
          latestAssistantText = text;
          break;
        }
      }
      if (!latestAssistantText) return;

      if (!streamedAssistantText) {
        handlers.addTextDelta(latestAssistantText);
        handlers.addSystemLog("Recovered final assistant output from session history.");
        return;
      }

      if (latestAssistantText.startsWith(streamedAssistantText)) {
        const suffix = latestAssistantText.slice(streamedAssistantText.length);
        if (suffix) {
          handlers.addTextDelta(suffix);
          handlers.addSystemLog(
            "Recovered missing trailing assistant output from session history."
          );
        }
        return;
      }

      if (latestAssistantText !== streamedAssistantText) {
        handlers.addTextDelta(`\n${latestAssistantText}`);
        handlers.addSystemLog(
          "Assistant output differed from live stream; synced from session history."
        );
      }
    } catch {
      // best-effort hydration; leave stream finalize path intact
    }
  };

  const finalize = (status: string) => {
    if (finalized) return;
    finalized = true;
    if (reconnectTimer) {
      window.clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    window.clearTimeout(watchdog);
    if (runTimeout) {
      window.clearTimeout(runTimeout);
      runTimeout = null;
    }
    window.clearInterval(runStatePoll);
    void (async () => {
      await hydrateFinalAssistantText();
      handlers.onFinalize(status);
      if (source) {
        source.close();
      }
      if (eventSourceRef.current === source && source) {
        eventSourceRef.current = null;
      }
    })();
  };

  const getRunId = (active: unknown): string => {
    const state = (active || {}) as { runID?: unknown; runId?: unknown; run_id?: unknown };
    return String(state.runID || state.runId || state.run_id || "").trim();
  };

  const scheduleReconnect = async (reason: string) => {
    if (finalized) return;
    reconnectAttempts += 1;

    try {
      const runState = await api.getActiveRun(sessionId);
      const active = runState?.active || null;
      const nextRunId = getRunId(active);
      if (!nextRunId) {
        handlers.addSystemLog(
          `Stream disconnected (${reason}) and no active run remains. Finalizing.`
        );
        finalize("inactive");
        return;
      }
      if (nextRunId !== activeRunId) {
        activeRunId = nextRunId;
        handlers.addSystemLog(`Detected updated active run (${nextRunId.substring(0, 8)}).`);
      }
    } catch {
      // If run-state query fails, continue reconnect attempts.
    }

    if (reconnectAttempts > reconnectMaxAttempts) {
      handlers.addSystemLog("Live stream reconnection limit reached. Finalizing run stream.");
      finalize("stream_error");
      return;
    }

    const delay = Math.min(8000, reconnectBaseDelayMs * reconnectAttempts);
    handlers.addSystemLog(
      `Stream interrupted. Reconnecting (${reconnectAttempts}/${reconnectMaxAttempts}) in ${Math.round(delay / 100) / 10}s...`
    );
    reconnectTimer = window.setTimeout(() => {
      reconnectTimer = null;
      if (!finalized) {
        openStream();
      }
    }, delay);
  };

  const resetRunTimeout = () => {
    if (typeof runTimeoutMs !== "number" || runTimeoutMs <= 0 || finalized) return;
    if (runTimeout) {
      window.clearTimeout(runTimeout);
    }
    runTimeout = window.setTimeout(async () => {
      if (finalized) return;
      try {
        const runState = await api.getActiveRun(sessionId);
        if (runState?.active) {
          handlers.addSystemLog(
            `Run exceeded ${runTimeoutMs}ms but is still active. Extending timeout window.`
          );
          resetRunTimeout();
          return;
        }
      } catch {
        // If active run check fails, fall through to timeout finalize.
      }
      handlers.addSystemLog(`Run timeout reached (${runTimeoutMs}ms).`);
      finalize("timeout");
    }, runTimeoutMs);
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

  resetRunTimeout();

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

  const handleMessage = (evt: MessageEvent<string>) => {
    try {
      resetRunTimeout();
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

      if (data.type === "permission.asked") {
        const tool = String(data?.properties?.tool || data?.properties?.permission || "tool");
        const requestId = String(data?.properties?.requestID || "").trim();
        handlers.addSystemLog(
          requestId
            ? `Permission requested for ${tool} (${requestId.substring(0, 8)}).`
            : `Permission requested for ${tool}.`
        );
        return;
      }

      if (data.type === "permission.replied") {
        const reply = String(data?.properties?.reply || "unknown");
        const requestId = String(data?.properties?.requestID || "").trim();
        handlers.addSystemLog(
          requestId
            ? `Permission reply: ${reply} (${requestId.substring(0, 8)}).`
            : `Permission reply: ${reply}.`
        );
        return;
      }

      if (data.type === "tool.loop_guard.triggered") {
        const tool = String(data?.properties?.tool || "tool");
        const reason = String(data?.properties?.reason || "guard");
        handlers.addSystemLog(`Tool loop guard triggered for ${tool}: ${reason}.`);
        return;
      }

      if (data.type === "tool.args.recovered") {
        const tool = String(data?.properties?.tool || "tool");
        handlers.addSystemLog(`Recovered tool arguments for ${tool}.`);
        return;
      }

      if (data.type !== "message.part.updated") return;
      const part = data?.properties?.part;
      if (!part) return;

      if (part.type === "tool" || part.type === "tool-invocation" || part.type === "tool-result") {
        const rawState = part?.state;
        const status =
          typeof rawState === "string"
            ? rawState
            : typeof rawState?.status === "string"
              ? rawState.status
              : undefined;
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
          const rawResult =
            part?.result ??
            part?.error ??
            (typeof rawState === "object" ? rawState?.result : undefined) ??
            (typeof rawState === "object" ? rawState?.output : undefined) ??
            "";
          const result =
            typeof rawResult === "string" ? rawResult : JSON.stringify(rawResult || {});
          handlers.onToolEnd({ tool: String(part.tool || "tool"), result });
        }
        return;
      }

      const delta = data?.properties?.delta;
      if (part.type === "text") {
        if (typeof delta === "string" && delta.length > 0) {
          streamedAssistantText += delta;
          handlers.addTextDelta(delta);
          return;
        }
        const fullText = typeof part?.text === "string" ? part.text : "";
        if (fullText.length > 0) {
          const key = String(
            part?.id ||
              part?.partID ||
              data?.properties?.messageID ||
              data?.properties?.id ||
              "text-part"
          );
          const prev = textSnapshots.get(key) || "";
          if (fullText.startsWith(prev) && fullText.length > prev.length) {
            const inferredDelta = fullText.slice(prev.length);
            streamedAssistantText += inferredDelta;
            handlers.addTextDelta(inferredDelta);
          } else if (!prev && !streamedAssistantText) {
            streamedAssistantText += fullText;
            handlers.addTextDelta(fullText);
          }
          textSnapshots.set(key, fullText);
        }
      }
    } catch {
      handlers.addSystemLog("Failed to parse stream event payload.");
    }
  };

  const openStream = () => {
    if (finalized) return;
    if (source) {
      source.close();
    }
    const currentSource = new EventSource(api.getEventStreamUrl(sessionId, activeRunId));
    source = currentSource;
    eventSourceRef.current = currentSource;

    currentSource.onopen = () => {
      reconnectAttempts = 0;
      resetRunTimeout();
    };
    currentSource.onmessage = handleMessage;
    currentSource.onerror = () => {
      currentSource.close();
      if (source === currentSource) {
        source = null;
      }
      if (eventSourceRef.current === currentSource) {
        eventSourceRef.current = null;
      }
      void scheduleReconnect("sse_error");
    };
  };

  openStream();
};
