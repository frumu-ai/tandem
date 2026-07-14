import { useCallback, useEffect, useRef, useState } from "react";
import type { TandemClient, WorkflowPlannerSessionRecord } from "@frumu/tandem-client";
import { toChatWorkflowArtifact, type ChatWorkflowArtifact } from "./workflowArtifact";

type ChatWorkflowArtifactState = {
  artifact: ChatWorkflowArtifact | null;
  session: WorkflowPlannerSessionRecord | null;
  loading: boolean;
  error: string;
};

const EMPTY_STATE: ChatWorkflowArtifactState = {
  artifact: null,
  session: null,
  loading: false,
  error: "",
};

export function useChatWorkflowArtifact(
  client: TandemClient,
  chatSessionId: string,
  refreshSignal: string
) {
  const requestRef = useRef(0);
  const [state, setState] = useState<ChatWorkflowArtifactState>(EMPTY_STATE);

  const refresh = useCallback(async () => {
    const sessionId = chatSessionId.trim();
    const requestId = ++requestRef.current;
    if (!sessionId) {
      setState(EMPTY_STATE);
      return null;
    }
    setState((current) => ({ ...current, loading: !current.artifact, error: "" }));
    try {
      const listed = await client.workflowPlannerSessions.list({
        linkedChatSessionId: sessionId,
      });
      const latest = [...(listed.sessions ?? [])].sort(
        (left, right) =>
          (right.last_referenced_at_ms ?? right.updated_at_ms) -
          (left.last_referenced_at_ms ?? left.updated_at_ms)
      )[0];
      if (!latest) {
        if (requestId === requestRef.current) setState(EMPTY_STATE);
        return null;
      }
      const response = await client.workflowPlannerSessions.get(latest.session_id);
      if (requestId !== requestRef.current) return null;
      const session = response.session;
      const artifact = toChatWorkflowArtifact(session);
      setState({ artifact, session, loading: false, error: "" });
      return session;
    } catch (error) {
      if (requestId !== requestRef.current) return null;
      setState((current) => ({
        ...current,
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      }));
      return null;
    }
  }, [chatSessionId, client.workflowPlannerSessions]);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshSignal]);

  useEffect(() => {
    if (state.artifact?.operationStatus !== "running") return;
    const poll = window.setInterval(() => void refresh(), 1500);
    return () => window.clearInterval(poll);
  }, [refresh, state.artifact?.operationStatus]);

  return { ...state, refresh };
}
