import { useEffect, useMemo } from "react";
import { subscribeSse } from "../../services/sse";

type Envelope = {
  kind?: string;
  run_id?: string;
  runId?: string;
  seq?: number;
  ts_ms?: number;
  payload?: any;
};

export function buildCursorToken(
  cursorsByRunId: Record<string, { eventSeq: number; patchSeq: number }>
) {
  const events: Record<string, number> = {};
  const patches: Record<string, number> = {};
  for (const [runId, cursor] of Object.entries(cursorsByRunId || {})) {
    const eventSeq = Number(cursor?.eventSeq || 0);
    const patchSeq = Number(cursor?.patchSeq || 0);
    if (eventSeq > 0) events[runId] = eventSeq;
    if (patchSeq > 0) patches[runId] = patchSeq;
  }
  const payload = { events, patches };
  const jsonText = JSON.stringify(payload);
  if (!jsonText || jsonText === '{"events":{},"patches":{}}') return "";
  return btoa(jsonText).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

export function useOrchestratorEvents({
  workspace,
  runIds,
  cursorToken,
  onEnvelope,
  onError,
}: {
  workspace: string;
  runIds: string[];
  cursorToken?: string;
  onEnvelope: (envelope: Envelope) => void;
  onError?: (error: unknown) => void;
}) {
  const runIdsKey = useMemo(
    () => [...new Set((runIds || []).map((id) => String(id || "").trim()).filter(Boolean))],
    [runIds]
  );
  const url = useMemo(() => {
    const ws = String(workspace || "").trim();
    if (!ws || !runIdsKey.length) return "";
    const params = new URLSearchParams();
    params.set("workspace", ws);
    params.set("runIds", runIdsKey.join(","));
    if (cursorToken) params.set("cursor", cursorToken);
    return `/api/orchestrator/events?${params.toString()}`;
  }, [cursorToken, runIdsKey, workspace]);

  useEffect(() => {
    if (!url) return;
    const unsubscribe = subscribeSse(
      url,
      (event: MessageEvent) => {
        let payload: any = null;
        try {
          payload = JSON.parse(String(event?.data || "{}"));
        } catch {
          return;
        }
        if (!payload || typeof payload !== "object") return;
        onEnvelope(payload as Envelope);
      },
      {
        onError: (err: unknown) => {
          if (onError) onError(err);
        },
      }
    );
    return () => unsubscribe();
  }, [onEnvelope, onError, url]);
}
