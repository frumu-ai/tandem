type AddLogFn = (event: { type: "system"; content: string }) => void;

type FinalizeFn = (status: string) => void | Promise<void>;

const asStatus = (value: unknown): string | null => {
  return typeof value === "string" && value.length > 0 ? value : null;
};

const isTerminalStatus = (status: string | null): boolean => {
  if (!status) return false;
  const normalized = status.toLowerCase();
  return (
    normalized === "completed" ||
    normalized === "failed" ||
    normalized === "error" ||
    normalized === "cancelled" ||
    normalized === "canceled" ||
    normalized === "timeout" ||
    normalized === "timed_out"
  );
};

export const handleCommonRunEvent = (
  data: unknown,
  addLog: AddLogFn,
  finalizeRun: FinalizeFn
): boolean => {
  const evt = (data || {}) as { type?: string; properties?: Record<string, unknown> };
  const type = evt.type;
  const props = evt.properties || {};
  const status = asStatus(props.status);

  if (type === "server.connected") {
    addLog({
      type: "system",
      content: "Connected to live engine stream.",
    });
    return true;
  }

  if (type === "engine.lifecycle.ready") {
    addLog({
      type: "system",
      content: "Engine stream is ready.",
    });
    return true;
  }

  if (type === "session.run.started") {
    const runId =
      (props.runID as string | undefined) ||
      (props.runId as string | undefined) ||
      (props.run_id as string | undefined) ||
      "";
    addLog({
      type: "system",
      content: runId ? `Run started (${runId.substring(0, 8)}).` : "Run started.",
    });
    return true;
  }

  if (type === "session.error") {
    const err = (props.error || {}) as { message?: string };
    addLog({
      type: "system",
      content: `Engine error: ${err.message || "Unknown error"}`,
    });
    void finalizeRun("failed");
    return true;
  }

  if (type === "session.status" && status) {
    addLog({
      type: "system",
      content: `Session status: ${status}`,
    });
    if (isTerminalStatus(status)) {
      void finalizeRun(status);
    }
    return true;
  }

  if (
    type === "session.run.completed" ||
    type === "session.run.failed" ||
    type === "session.run.cancelled" ||
    type === "session.run.canceled"
  ) {
    const inferred =
      type === "session.run.completed"
        ? "completed"
        : type === "session.run.failed"
          ? "failed"
          : "cancelled";
    void finalizeRun(status || inferred);
    return true;
  }

  if (
    (type === "run.status.updated" || type === "session.run.finished") &&
    isTerminalStatus(status)
  ) {
    void finalizeRun(status || "completed");
    return true;
  }

  return false;
};
