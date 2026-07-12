import type { TandemClient } from "@frumu/tandem-client";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
  goalOperationsReducer,
  initialGoalOperationsState,
} from "./model";
import type { GoalProjection, ProjectionConnection } from "./types";

type ProjectionRuntime = TandemClient["statefulRuntime"] & {
  getGoalProjection(
    goalId: string,
    options?: { cursor?: number; limit?: number }
  ): Promise<GoalProjection>;
};

const POLL_INTERVAL_MS = 3_000;
const RETRY_MAX_MS = 20_000;

export function useGoalProjection(client: TandemClient, goalId: string) {
  const [state, dispatch] = useReducer(goalOperationsReducer, undefined, initialGoalOperationsState);
  const [connection, setConnection] = useState<ProjectionConnection>("connecting");
  const [error, setError] = useState("");
  const [replayError, setReplayError] = useState("");
  const [replayProjection, setReplayProjection] = useState<GoalProjection | null>(null);
  const cursorRef = useRef<number | null>(null);
  const failureCount = useRef(0);
  const replayRequestRef = useRef(0);
  const streamConnectedRef = useRef(false);

  useEffect(() => {
    cursorRef.current = state.cursor;
  }, [state.cursor]);

  const refresh = useCallback(
    async (replace = false) => {
      if (!goalId) return;
      const runtime = client.statefulRuntime as ProjectionRuntime;
      try {
        const projection = await runtime.getGoalProjection(goalId, {
          limit: 120,
        });
        dispatch({ type: "projection", projection, replace });
        failureCount.current = 0;
        setError("");
        setConnection(streamConnectedRef.current ? "live" : "polling");
      } catch (reason) {
        failureCount.current += 1;
        setConnection(navigator.onLine ? "polling" : "offline");
        setError(reason instanceof Error ? reason.message : "Goal projection is unavailable.");
        throw reason;
      }
    },
    [client, goalId]
  );

  useEffect(() => {
    if (!goalId) return undefined;
    let active = true;
    let timer = 0;
    const poll = async (replace = false) => {
      if (!active) return;
      try {
        await refresh(replace);
      } catch {
        // The status banner carries the error while bounded backoff reconnects.
      }
      if (!active) return;
      const delay = failureCount.current
        ? Math.min(RETRY_MAX_MS, POLL_INTERVAL_MS * 2 ** failureCount.current)
        : streamConnectedRef.current ? 15_000 : POLL_INTERVAL_MS;
      timer = window.setTimeout(() => void poll(false), delay);
    };
    const reconnect = () => {
      window.clearTimeout(timer);
      setConnection("connecting");
      void poll(true);
    };
    const visibility = () => {
      if (document.visibilityState === "visible") reconnect();
      else setConnection("polling");
    };
    const offline = () => setConnection("offline");
    window.addEventListener("online", reconnect);
    window.addEventListener("offline", offline);
    document.addEventListener("visibilitychange", visibility);
    void poll(true);
    return () => {
      active = false;
      window.clearTimeout(timer);
      window.removeEventListener("online", reconnect);
      window.removeEventListener("offline", offline);
      document.removeEventListener("visibilitychange", visibility);
    };
  }, [goalId, refresh]);

  useEffect(() => {
    if (!goalId) return undefined;
    let active = true;
    let refreshTimer = 0;
    let reconnectTimer = 0;
    const controller = new AbortController();

    const scheduleProjectionRefresh = () => {
      window.clearTimeout(refreshTimer);
      refreshTimer = window.setTimeout(() => {
        if (active) void refresh(false).catch(() => undefined);
      }, 100);
    };

    const consume = async () => {
      while (active) {
        try {
          const stream = client.statefulRuntime.events(goalId, {
            ...(cursorRef.current === null ? {} : { cursor: cursorRef.current }),
            signal: controller.signal,
            reconnect: true,
            maxReconnectAttempts: 8,
            onSequenceGap: () => {
              streamConnectedRef.current = false;
              setConnection("polling");
              void refresh(true).catch(() => undefined);
            },
          });
          for await (const _event of stream) {
            if (!active) return;
            streamConnectedRef.current = true;
            setConnection("live");
            scheduleProjectionRefresh();
          }
        } catch (reason) {
          if (!active || controller.signal.aborted) return;
          setError(reason instanceof Error ? reason.message : "Live updates disconnected.");
        }
        streamConnectedRef.current = false;
        setConnection(navigator.onLine ? "polling" : "offline");
        await new Promise<void>((resolve) => {
          reconnectTimer = window.setTimeout(resolve, POLL_INTERVAL_MS);
        });
      }
    };

    void consume();
    return () => {
      active = false;
      streamConnectedRef.current = false;
      controller.abort();
      window.clearTimeout(refreshTimer);
      window.clearTimeout(reconnectTimer);
    };
  }, [client, goalId, refresh]);

  const loadReplayProjection = useCallback(async (index: number) => {
    const cursor = state.timeline[index]?.cursor;
    setReplayProjection(null);
    setReplayError("");
    if (!goalId || cursor === undefined) return;
    const request = replayRequestRef.current + 1;
    replayRequestRef.current = request;
    try {
      const runtime = client.statefulRuntime as ProjectionRuntime;
      const historical = await runtime.getGoalProjection(goalId, { cursor, limit: 120 });
      if (replayRequestRef.current === request) {
        setReplayProjection(historical);
        setReplayError("");
      }
    } catch (reason) {
      if (replayRequestRef.current === request) {
        setReplayError(reason instanceof Error ? reason.message : "Historical projection is unavailable.");
      }
    }
  }, [client, goalId, state.timeline]);

  const setMode = useCallback((mode: "live" | "replay") => {
    dispatch({ type: "mode", mode });
    if (mode === "live") {
      replayRequestRef.current += 1;
      setReplayProjection(null);
      setReplayError("");
    } else {
      void loadReplayProjection(state.replayIndex);
    }
  }, [loadReplayProjection, state.replayIndex]);

  const scrub = useCallback((index: number) => {
    dispatch({ type: "scrub", index });
    void loadReplayProjection(index);
  }, [loadReplayProjection]);

  return {
    state,
    projection: state.mode === "replay"
      ? replayProjection
      : state.projection,
    connection,
    error: state.mode === "replay" ? replayError : error,
    dispatch,
    setMode,
    scrub,
    refresh: () => refresh(true),
  };
}
