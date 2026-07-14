import { useCallback, useEffect, useRef, useState } from "react";
import type {
  TandemClient,
  WorkflowPlannerSessionListItem,
  WorkflowPlannerSessionRecord,
} from "@frumu/tandem-client";
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

function selectActiveSession(
  sessions: WorkflowPlannerSessionListItem[]
): WorkflowPlannerSessionListItem | undefined {
  if (sessions.length === 1) return sessions[0];

  const latestReference = sessions.reduce<number | undefined>((latest, session) => {
    const referencedAt = session.last_referenced_at_ms;
    if (referencedAt == null) return latest;
    return latest == null ? referencedAt : Math.max(latest, referencedAt);
  }, undefined);
  if (latestReference != null) {
    const referenced = sessions.filter(
      (session) => session.last_referenced_at_ms === latestReference
    );
    if (referenced.length === 1) return referenced[0];
  }
  return undefined;
}

export function useChatWorkflowArtifact(
  client: TandemClient,
  chatSessionId: string,
  refreshSignal: string
) {
  const requestRef = useRef(0);
  const activeChatSessionRef = useRef("");
  const [state, setState] = useState<ChatWorkflowArtifactState>(EMPTY_STATE);

  const refresh = useCallback(async () => {
    const sessionId = chatSessionId.trim();
    const requestId = ++requestRef.current;
    const sessionChanged = activeChatSessionRef.current !== sessionId;
    activeChatSessionRef.current = sessionId;
    if (!sessionId) {
      setState(EMPTY_STATE);
      return null;
    }
    setState((current) =>
      sessionChanged
        ? { ...EMPTY_STATE, loading: true }
        : { ...current, loading: !current.artifact, error: "" }
    );
    try {
      const listed = await client.workflowPlannerSessions.list({
        linkedChatSessionId: sessionId,
      });
      const latest = selectActiveSession(listed.sessions ?? []);
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
