import { useEffect, useMemo } from "react";
import {
  workflowEventSessionId,
  workflowSessionLogEventEntries,
} from "../orchestration/workflowStability";

export function useSessionLogEntries({
  selectedSessionFilterId,
  selectedSessionId,
  sessionMessageCreatedAt,
  sessionMessageId,
  sessionMessageParts,
  sessionMessageText,
  sessionMessageVariant,
  sessionMessages,
  sessionEvents,
  sessionLabel,
  sessionLogRef,
  sessionLogPinnedToBottom,
}: any) {
  const sessionLogEntries = useMemo(() => {
    const messageEntries = sessionMessages.map(({ sessionId, message }: any, index: number) => ({
      id: `message:${sessionId}:${sessionMessageId(message, index)}`,
      kind: "message" as const,
      sessionId,
      at: sessionMessageCreatedAt(message),
      variant: sessionMessageVariant(message),
      label: String(message?.info?.role || "session").trim() || "session",
      body: sessionMessageText(message),
      raw: message,
      parts: sessionMessageParts(message),
      sessionLabel: sessionLabel(sessionId),
    }));
    const liveEntries = workflowSessionLogEventEntries(sessionEvents, selectedSessionId).map(
      (entry) => ({
        ...entry,
        sessionLabel: sessionLabel(workflowEventSessionId(entry.raw, selectedSessionId)),
      })
    );
    const rows = [...messageEntries, ...liveEntries].sort((a, b) => a.at - b.at);
    if (selectedSessionFilterId === "all") return rows;
    return rows.filter((entry) => entry.sessionId === selectedSessionFilterId);
  }, [
    selectedSessionFilterId,
    selectedSessionId,
    sessionMessageCreatedAt,
    sessionMessageId,
    sessionMessageParts,
    sessionMessageText,
    sessionMessageVariant,
    sessionMessages,
    sessionEvents,
    sessionLabel,
  ]);
  useEffect(() => {
    const el = sessionLogRef.current;
    if (!el || !sessionLogPinnedToBottom) return;
    el.scrollTop = el.scrollHeight;
  }, [sessionLogEntries, sessionLogPinnedToBottom]);

  return sessionLogEntries;
}
