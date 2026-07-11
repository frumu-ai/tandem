import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { renderMarkdownSafe } from "../lib/markdown";
import { ChatInterfacePanel } from "../components/ChatInterfacePanel";
import { DetailDrawer } from "../ui/index.tsx";
import type { AppPageProps } from "./pageTypes";
import {
  type ChatSession,
  loadSessions,
  loadStoredSessionId,
  saveStoredSessionId,
} from "../features/chat/session";
import { normalizeMessages, type ChatMessage } from "../features/chat/messages";
import { openFilesExplorer } from "../features/files/explorerHandoff";
import {
  extractRunId,
  extractToolCallId,
  extractToolName,
  isPendingPermissionStatus,
  normalizePermissionRequest,
  summarizeToolPayload,
  TERMINAL_FAILURE_EVENTS,
  TERMINAL_SUCCESS_EVENTS,
  TOOL_END_EVENTS,
  TOOL_PROGRESS_EVENTS,
  TOOL_START_EVENTS,
  type PermissionRequest,
  type ToolActivityItem,
  type ToolActivityStatus,
} from "../features/chat/streaming";
import { subscribeSse } from "../services/sse.js";
import {
  collectRunTimelineFromMessages,
  collectToolActivityFromMessages,
  formatBytes,
  inferMime,
  isRunSignalEvent,
  joinRootAndRel,
  loadAutoApprovePreference,
  loadToolAllowlistPreference,
  sameSession,
  saveAutoApprovePreference,
  saveToolAllowlistPreference,
  seedAutomationPlanner,
  seedWorkflowPlanner,
  setupCardFromResponse,
  shouldShowSetupCard,
  timelineSummaryForEvent,
  timelineTitleForEvent,
  timelineToneForEvent,
  toolStatusFromPayload,
  toolStatusClass,
  toTextError,
  type ConfirmDeleteState,
  type RunTimelineItem,
  type SetupCard,
  type SetupUnderstandResponse,
  type UploadFile,
  type UploadProgressRow,
} from "../features/chat/chatPageHelpers";
import { Icon } from "../ui/Icon";

const CHAT_UPLOAD_DIR = "uploads";

function countAssistantReplies(rows: ChatMessage[]): number {
  return rows.filter((row) => row.role === "assistant" && row.text.trim().length > 0).length;
}

function appendUniqueAssistantMessage(
  rows: ChatMessage[],
  runId: string,
  assistantName: string,
  text: string
): ChatMessage[] {
  const content = text.trim();
  if (!content) return rows;
  const last = rows[rows.length - 1];
  if (last?.role === "assistant" && last.text.trim() === content) return rows;
  return [
    ...rows,
    {
      id: `local-assistant-${runId || Date.now()}`,
      role: "assistant",
      displayRole: assistantName || "Assistant",
      text: content,
      markdown: true,
    },
  ];
}

export function ChatPage({ client, api, toast, providerStatus, identity, navigate }: AppPageProps) {
  const queryClient = useQueryClient();
  const reducedMotion = !!useReducedMotion();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const messagesRef = useRef<HTMLDivElement | null>(null);
  const runAbortRef = useRef<AbortController | null>(null);
  const mountedRef = useRef(true);
  const noEventTimerRef = useRef<number | null>(null);
  const maxStreamTimerRef = useRef<number | null>(null);

  const [sessions, setSessions] = useState<ChatSession[]>([]);
  const [sessionsOpen, setSessionsOpen] = useState(false);
  const [railDrawerOpen, setRailDrawerOpen] = useState(false);
  const [selectedSessionId, setSelectedSessionId] = useState(loadStoredSessionId());
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [uploads, setUploads] = useState<UploadFile[]>([]);
  const [uploadRows, setUploadRows] = useState<UploadProgressRow[]>([]);
  const [sending, setSending] = useState(false);
  const [streamingText, setStreamingText] = useState("");
  const [showThinking, setShowThinking] = useState(false);
  const [toolActivity, setToolActivity] = useState<ToolActivityItem[]>([]);
  const [toolEventSeen, setToolEventSeen] = useState<Set<string>>(new Set());
  const [runTimeline, setRunTimeline] = useState<RunTimelineItem[]>([]);
  const [runTimelineSeen, setRunTimelineSeen] = useState<Set<string>>(new Set());
  const [permissions, setPermissions] = useState<PermissionRequest[]>([]);
  const [permissionBusy, setPermissionBusy] = useState<Set<string>>(new Set());
  const [autoApprove, setAutoApprove] = useState(loadAutoApprovePreference());
  const [autoApproveInFlight, setAutoApproveInFlight] = useState(false);
  const [availableTools, setAvailableTools] = useState<string[]>([]);
  const [selectedTools, setSelectedTools] = useState<string[]>(loadToolAllowlistPreference());
  const [deleteConfirm, setDeleteConfirm] = useState<ConfirmDeleteState>(null);
  const [setupCard, setSetupCard] = useState<SetupCard | null>(null);
  const [workflowNudgeDismissedFor, setWorkflowNudgeDismissedFor] = useState("");

  const sessionTitle = useMemo(() => {
    const hit = sessions.find((x) => x.id === selectedSessionId);
    return hit?.title || "Chat";
  }, [selectedSessionId, sessions]);
  const selectedToolSet = useMemo(() => new Set(selectedTools), [selectedTools]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      runAbortRef.current?.abort();
      if (noEventTimerRef.current) window.clearTimeout(noEventTimerRef.current);
      if (maxStreamTimerRef.current) window.clearTimeout(maxStreamTimerRef.current);
    };
  }, []);

  useEffect(() => {
    saveStoredSessionId(selectedSessionId);
  }, [selectedSessionId]);

  useEffect(() => {
    const area = inputRef.current;
    if (!area) return;
    area.style.height = "0px";
    area.style.height = `${Math.min(area.scrollHeight, 180)}px`;
  }, [prompt]);

  useEffect(() => {
    const host = messagesRef.current;
    if (!host) return;
    host.scrollTop = host.scrollHeight;
  }, [messages, streamingText, showThinking]);

  const refreshSessions = useCallback(async () => {
    const rows = await loadSessions(client, api);
    if (!mountedRef.current) return;
    setSessions(rows);
    setSelectedSessionId((prev) => {
      if (prev && rows.some((row) => row.id === prev)) return prev;
      const saved = loadStoredSessionId();
      if (saved && rows.some((row) => row.id === saved)) return saved;
      return rows[0]?.id || "";
    });
  }, [api, client]);

  const resolveModelRoute = useCallback(async () => {
    const knownProvider = String(providerStatus.defaultProvider || "").trim();
    const knownModel = String(providerStatus.defaultModel || "").trim();
    if (knownProvider && knownModel) {
      return { providerID: knownProvider, modelID: knownModel };
    }
    try {
      const cfg = await client.providers.config();
      const providerID = String(cfg?.default || "").trim();
      const modelID = String(cfg?.providers?.[providerID]?.defaultModel || "").trim();
      if (providerID && modelID) return { providerID, modelID };
    } catch {
      // use known fallback
    }
    return null;
  }, [client.providers, providerStatus.defaultModel, providerStatus.defaultProvider]);

  const createSession = useCallback(async () => {
    const modelRoute = await resolveModelRoute();
    const payload: Record<string, any> = { title: `Chat ${new Date().toLocaleTimeString()}` };
    if (modelRoute) {
      payload.provider = modelRoute.providerID;
      payload.model = modelRoute.modelID;
    }
    const created = await client.sessions.create(payload);
    const sessionId = String(created || "").trim();
    if (!sessionId) throw new Error("Failed to create session.");
    setSelectedSessionId(sessionId);
    await refreshSessions();
    return sessionId;
  }, [client.sessions, refreshSessions, resolveModelRoute]);

  const normalizeAndSetMessages = useCallback(
    (payload: any) => {
      const rows = normalizeMessages(payload, identity.botName || "Assistant");
      setMessages(rows);
      return rows;
    },
    [identity.botName]
  );

  const loadMessagesForSession = useCallback(
    async (sessionId: string) => {
      const targetSessionId = String(sessionId || "").trim();
      if (!targetSessionId) {
        setMessages([]);
        return [];
      }
      const rows = await client.sessions.messages(targetSessionId).catch(() => ({ messages: [] }));
      if (!mountedRef.current) return [];
      const normalized = normalizeAndSetMessages(rows);
      const history = collectToolActivityFromMessages(rows);
      if (history.length) {
        setToolActivity((prev) => {
          const seen = new Set(prev.map((item) => item.id));
          const merged = [...history.filter((item) => !seen.has(item.id)), ...prev];
          return merged.sort((a, b) => b.at - a.at).slice(0, 80);
        });
      }
      const messageTimeline = collectRunTimelineFromMessages(rows);
      if (messageTimeline.length) {
        setRunTimeline((prev) => {
          const seen = new Set(prev.map((item) => item.id));
          const merged = [...messageTimeline.filter((item) => !seen.has(item.id)), ...prev];
          return merged.sort((a, b) => b.at - a.at).slice(0, 80);
        });
      }
      return normalized;
    },
    [client.sessions, normalizeAndSetMessages]
  );

  const refreshMessages = useCallback(async () => {
    if (!selectedSessionId) {
      setMessages([]);
      return;
    }
    setMessagesLoading(true);
    try {
      await loadMessagesForSession(selectedSessionId);
    } finally {
      if (mountedRef.current) setMessagesLoading(false);
    }
  }, [loadMessagesForSession, selectedSessionId]);

  const resetToolTracking = useCallback(() => {
    setToolActivity([]);
    setToolEventSeen(new Set());
  }, []);

  const resetRunTimeline = useCallback(() => {
    setRunTimeline([]);
    setRunTimelineSeen(new Set());
  }, []);

  const recordToolActivity = useCallback(
    (
      toolName: string,
      status: ToolActivityStatus,
      eventKey = "",
      meta: Partial<ToolActivityItem> = {}
    ) => {
      const tool = String(toolName || "").trim();
      if (!tool) return;

      let accepted = true;
      if (eventKey) {
        setToolEventSeen((prev) => {
          if (prev.has(eventKey)) {
            accepted = false;
            return prev;
          }
          const next = new Set(prev);
          next.add(eventKey);
          if (next.size > 1000) return new Set([eventKey]);
          return next;
        });
        if (!accepted) return;
      }

      setToolActivity((prev) => {
        const next = [
          {
            id: `${tool}:${status}:${Date.now()}:${Math.random().toString(36).slice(2, 8)}`,
            tool,
            status,
            at: Date.now(),
            ...meta,
          },
          ...prev,
        ];
        return next.slice(0, 80);
      });
    },
    []
  );

  const recordRunTimeline = useCallback((rawType: string, rawProps: any, fallbackRunId = "") => {
    const type = String(rawType || "").trim();
    if (!type || type === "server.connected" || type === "engine.lifecycle.ready") return;
    const props = rawProps && typeof rawProps === "object" ? rawProps : {};
    const runId = String(extractRunId({ properties: props }, fallbackRunId) || fallbackRunId);
    const eventKey = [
      type,
      runId,
      extractToolCallId(props),
      String(props?.status || props?.state || "").trim(),
      String(props?.error || "").trim(),
    ]
      .filter(Boolean)
      .join(":");
    let accepted = true;
    setRunTimelineSeen((prev) => {
      if (prev.has(eventKey)) {
        accepted = false;
        return prev;
      }
      const next = new Set(prev);
      next.add(eventKey);
      if (next.size > 400) return new Set([eventKey]);
      return next;
    });
    if (!accepted) return;

    setRunTimeline((prev) =>
      [
        {
          id: `${eventKey}:${Date.now()}`,
          type,
          title: timelineTitleForEvent(type),
          summary: timelineSummaryForEvent(type, props),
          tone: timelineToneForEvent(type, props),
          at:
            Number(props?.at || props?.createdAtMs || props?.startedAtMs || props?.finishedAtMs) ||
            Date.now(),
          runId,
        },
        ...prev,
      ].slice(0, 80)
    );
  }, []);

  const recordToolEvent = useCallback(
    (eventType: string, props: any, fallbackRunId = "") => {
      const tool = extractToolName(props);
      if (!tool) return;
      const callId = extractToolCallId(props);
      const runId = String(extractRunId({ properties: props }, fallbackRunId) || fallbackRunId);
      const lowerType = String(eventType || "").toLowerCase();
      const status =
        lowerType.includes("fail") || lowerType.includes("error")
          ? "failed"
          : lowerType.includes("complete") ||
              lowerType.includes("succeed") ||
              lowerType.includes("result") ||
              lowerType.includes("end")
            ? "completed"
            : toolStatusFromPayload(props);
      recordToolActivity(
        tool,
        status,
        `${callId || runId || Date.now()}:${tool}:${eventType}:${status}`,
        {
          callId,
          runId,
          source: eventType,
          summary: summarizeToolPayload(props),
          detail: summarizeToolPayload(props),
        }
      );
    },
    [recordToolActivity]
  );

  const upsertPermissionRequest = useCallback((req: PermissionRequest) => {
    if (!isPendingPermissionStatus(req?.status || "")) return;
    setPermissions((prev) => {
      const idx = prev.findIndex((item) => item.id === req.id);
      if (idx >= 0) {
        const next = [...prev];
        next[idx] = { ...next[idx], ...req };
        return next;
      }
      return [req, ...prev];
    });
  }, []);

  const removePermissionRequest = useCallback((requestId: string) => {
    setPermissions((prev) => prev.filter((item) => item.id !== requestId));
  }, []);

  const refreshPermissionRequests = useCallback(async () => {
    const snapshot = await client.permissions.list().catch(() => ({ requests: [] }));
    const rows = Array.isArray(snapshot?.requests) ? snapshot.requests : [];
    const normalized = rows
      .map((raw: any) => normalizePermissionRequest(raw))
      .filter((row: PermissionRequest | null): row is PermissionRequest => !!row)
      .filter((row) => sameSession(row.sessionId, selectedSessionId))
      .filter((row) => isPendingPermissionStatus(row.status));

    if (!mountedRef.current) return;
    setPermissions(normalized.slice(0, 80));
  }, [client.permissions, selectedSessionId]);

  const replyPermission = useCallback(
    async (requestId: string, replyMode: "once" | "always" | "deny", quiet = false) => {
      if (!requestId) return;
      const busy = permissionBusy.has(requestId);
      if (busy) return;

      setPermissionBusy((prev) => {
        const next = new Set(prev);
        next.add(requestId);
        return next;
      });

      try {
        await client.permissions.reply(requestId, replyMode);
        removePermissionRequest(requestId);
        if (!quiet) {
          toast("ok", `Permission ${replyMode === "deny" ? "denied" : "approved"} (${requestId}).`);
        }
      } catch (error) {
        if (!quiet) toast("err", toTextError(error));
      } finally {
        setPermissionBusy((prev) => {
          const next = new Set(prev);
          next.delete(requestId);
          return next;
        });
        void refreshPermissionRequests();
      }
    },
    [client.permissions, permissionBusy, refreshPermissionRequests, removePermissionRequest, toast]
  );

  const autoApprovePendingRequests = useCallback(async () => {
    if (!autoApprove || autoApproveInFlight || permissions.length === 0) return;
    setAutoApproveInFlight(true);
    try {
      for (const row of [...permissions]) {
        await replyPermission(row.id, "always", true);
      }
    } finally {
      if (mountedRef.current) setAutoApproveInFlight(false);
    }
  }, [autoApprove, autoApproveInFlight, permissions, replyPermission]);

  const extractToolsFromPayload = useCallback((raw: any) => {
    const rows = Array.isArray(raw) ? raw : Array.isArray(raw?.tools) ? raw.tools : [];
    return rows
      .map((item: any) => {
        if (typeof item === "string") return item;
        const rec = item || {};
        return String(rec.name || rec.id || rec.tool || "").trim();
      })
      .filter(Boolean) as string[];
  }, []);

  const refreshAvailableTools = useCallback(async () => {
    try {
      const direct = await (client as any).listTools?.().catch(() => null);
      let ids = extractToolsFromPayload(direct || []);
      if (!ids.length) {
        const fallback = await api("/api/engine/tool", { method: "GET" }).catch(() => []);
        ids = extractToolsFromPayload(fallback || []);
      }
      if (mountedRef.current) {
        const unique = [...new Set(ids)].sort((a, b) => a.localeCompare(b));
        setAvailableTools(unique);
      }
    } catch {
      if (mountedRef.current) setAvailableTools([]);
    }
  }, [api, client, extractToolsFromPayload]);

  const uploadOne = useCallback((file: File) => {
    return new Promise<any>((resolve, reject) => {
      const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
      setUploadRows((prev) => [...prev, { id, name: file.name, progress: 0, error: "" }]);

      const xhr = new XMLHttpRequest();
      xhr.open("POST", `/api/files/upload?dir=${encodeURIComponent(CHAT_UPLOAD_DIR)}`);
      xhr.withCredentials = true;
      xhr.responseType = "json";
      xhr.setRequestHeader("x-file-name", encodeURIComponent(file.name));

      xhr.upload.onprogress = (event) => {
        if (!event.lengthComputable) return;
        const pct = (event.loaded / event.total) * 100;
        setUploadRows((prev) =>
          prev.map((row) => (row.id === id ? { ...row, progress: pct } : row))
        );
      };

      xhr.onerror = () => {
        setUploadRows((prev) =>
          prev.map((row) => (row.id === id ? { ...row, error: "Network error" } : row))
        );
        window.setTimeout(() => {
          setUploadRows((prev) => prev.filter((row) => row.id !== id));
        }, 1200);
        reject(new Error(`Upload failed: ${file.name}`));
      };

      xhr.onload = () => {
        const payload = xhr.response || {};
        if (xhr.status < 200 || xhr.status >= 300 || payload?.ok === false) {
          const message = String(payload?.error || `Upload failed (${xhr.status})`);
          setUploadRows((prev) =>
            prev.map((row) => (row.id === id ? { ...row, error: message } : row))
          );
          window.setTimeout(() => {
            setUploadRows((prev) => prev.filter((row) => row.id !== id));
          }, 1600);
          reject(new Error(message));
          return;
        }

        setUploadRows((prev) => prev.filter((row) => row.id !== id));
        resolve(payload);
      };

      xhr.send(file);
    });
  }, []);

  const uploadFiles = useCallback(
    async (fileList: FileList | null) => {
      const files = [...(fileList || [])];
      if (!files.length) return;

      let success = 0;
      for (const file of files) {
        try {
          const rec = await uploadOne(file);
          setUploads((prev) => [
            {
              name: String(rec?.name || file.name),
              path: String(rec?.path || file.name),
              size: Number(rec?.size || file.size || 0),
              mime: file.type || inferMime(String(rec?.name || file.name)),
              url: String(
                rec?.absPath || joinRootAndRel(rec?.root, rec?.path) || rec?.path || file.name
              ),
            },
            ...prev,
          ]);
          success += 1;
        } catch (error) {
          toast("err", toTextError(error));
        }
      }

      if (success > 0) toast("ok", `Uploaded ${success} file${success === 1 ? "" : "s"}.`);
    },
    [toast, uploadOne]
  );

  const removeSession = useCallback(
    async (sessionId: string) => {
      await client.sessions.delete(sessionId);
      setSessions((prev) => prev.filter((row) => row.id !== sessionId));
      setDeleteConfirm(null);
      toast("ok", "Session deleted.");

      if (selectedSessionId === sessionId) {
        const next = sessions.find((row) => row.id !== sessionId)?.id || "";
        if (next) {
          setSelectedSessionId(next);
        } else {
          const created = await createSession();
          setSelectedSessionId(created);
        }
      }
    },
    [client.sessions, createSession, selectedSessionId, sessions, toast]
  );

  const appendTransientUserMessage = useCallback((text: string, attachedCount: number) => {
    const content = String(text || "").trim();
    if (!content) return;
    const attachedLabel =
      attachedCount > 0 ? `\n\n${attachedCount} attachment${attachedCount === 1 ? "" : "s"}` : "";
    setMessages((prev) => [
      ...prev,
      {
        id: `local-user-${Date.now()}-${Math.random().toString(16).slice(2)}`,
        role: "user",
        displayRole: "User",
        text: `${content}${attachedLabel}`,
        markdown: false,
      },
    ]);
  }, []);

  const sendPrompt = useCallback(async () => {
    if (sending) return;

    const promptRaw = prompt.trim();
    const attached = [...uploads];
    const resolvedPrompt =
      promptRaw || (attached.length ? "Please analyze the attached file(s)." : "");
    if (!resolvedPrompt) return;

    try {
      const setup = (await api("/api/engine/setup/understand", {
        method: "POST",
        body: JSON.stringify({
          surface: "control_panel_chat",
          session_id: selectedSessionId || undefined,
          text: resolvedPrompt,
          channel: null,
          trigger: {
            source: "direct_message",
            is_direct_message: true,
            was_explicitly_mentioned: false,
            is_reply_to_bot: false,
          },
          scope: {
            kind: "direct",
            id: selectedSessionId || "control-panel-chat",
          },
        }),
      })) as SetupUnderstandResponse;
      if (
        setup.intent_kind !== "workflow_planner_create" &&
        shouldShowSetupCard(resolvedPrompt, setup)
      ) {
        const card = setupCardFromResponse(setup);
        if (card) {
          setSetupCard(card);
          setPrompt("");
          return;
        }
      }
    } catch {
      // continue with normal chat flow
    }

    setPrompt("");
    setSending(true);
    const assistantRepliesBeforeSend = countAssistantReplies(messages);
    appendTransientUserMessage(resolvedPrompt, attached.length);

    try {
      let sessionId = selectedSessionId;
      if (!sessionId) {
        sessionId = await createSession();
      }

      if (!sessionId) throw new Error("No active session.");

      const modelRoute = await resolveModelRoute();
      if (!modelRoute) {
        throw new Error(
          "No default provider/model configured. Set it in Settings before sending chat."
        );
      }

      const parts: Array<Record<string, string>> = attached.map((file) => ({
        type: "file",
        mime: file.mime || inferMime(file.name || file.path),
        filename: file.name || file.path || "attachment",
        url: file.url || file.path,
      }));
      parts.push({ type: "text", text: resolvedPrompt });

      const getActiveRunId = async () => {
        const res = await fetch(`/api/engine/session/${encodeURIComponent(sessionId)}/run`, {
          method: "GET",
          credentials: "include",
        });
        if (!res.ok) return "";
        const payload = await res.json().catch(() => ({}));
        return payload?.active?.runID || payload?.active?.runId || payload?.active?.run_id || "";
      };

      const activeRunIdFromPayload = (payload: any) =>
        String(
          payload?.activeRun?.runID ||
            payload?.activeRun?.runId ||
            payload?.activeRun?.run_id ||
            payload?.active?.runID ||
            payload?.active?.runId ||
            payload?.active?.run_id ||
            ""
        ).trim();

      const cancelAndWaitForIdle = async (knownRunId = "") => {
        const activeRunId =
          String(knownRunId || "").trim() || (await getActiveRunId().catch(() => ""));
        if (!activeRunId) return true;
        if (activeRunId) {
          await fetch(
            `/api/engine/session/${encodeURIComponent(sessionId)}/run/${encodeURIComponent(activeRunId)}/cancel`,
            {
              method: "POST",
              credentials: "include",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({}),
            }
          ).catch(() => {});
        }

        await fetch(`/api/engine/session/${encodeURIComponent(sessionId)}/cancel`, {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({}),
        }).catch(() => {});

        for (let i = 0; i < 50; i += 1) {
          const active = await getActiveRunId().catch(() => "");
          if (!active) return true;
          await new Promise((resolve) => window.setTimeout(resolve, 200));
        }
        return false;
      };

      const startRun = async () =>
        fetch(`/api/engine/session/${encodeURIComponent(sessionId)}/prompt_async?return=run`, {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            parts,
            model: {
              providerID: modelRoute.providerID,
              modelID: modelRoute.modelID,
            },
            ...(selectedTools.length
              ? {
                  toolMode: "auto",
                  toolAllowlist: selectedTools,
                }
              : {}),
          }),
        });

      const preflightIdle = await cancelAndWaitForIdle();
      if (!preflightIdle) throw new Error("Session has a stuck active run. Cancel it and retry.");

      let runResp = await startRun();
      let runId = "";

      if (runResp.status === 409) {
        const conflictPayload = await runResp.json().catch(() => ({}));
        const idle = await cancelAndWaitForIdle(activeRunIdFromPayload(conflictPayload));
        if (!idle) throw new Error("Session has a stuck active run. Cancel it and retry.");

        runResp = await startRun();
        if (runResp.ok) {
          const payload = await runResp.json().catch(() => ({}));
          runId = payload?.runID || payload?.runId || payload?.run_id || "";
        } else if (runResp.status === 409) {
          throw new Error("Session is still busy with another run. Retry in a moment.");
        } else {
          const body = await runResp.text().catch(() => "");
          throw new Error(`prompt_async retry failed (${runResp.status}): ${body}`);
        }
      } else if (runResp.ok) {
        const payload = await runResp.json().catch(() => ({}));
        runId = payload?.runID || payload?.runId || payload?.run_id || "";
      } else {
        const body = await runResp.text().catch(() => "");
        throw new Error(`prompt_async failed (${runResp.status}): ${body}`);
      }

      if (!runId) throw new Error("No run ID returned from engine.");
      if (attached.length) setUploads([]);
      recordRunTimeline(
        "session.run.started",
        {
          runID: runId,
          sessionID: sessionId,
          status: "started",
          message: "Prompt accepted",
          startedAtMs: Date.now(),
        },
        runId
      );

      setStreamingText("");
      setShowThinking(true);

      const streamAbort = new AbortController();
      runAbortRef.current?.abort();
      runAbortRef.current = streamAbort;

      let gotDelta = false;
      let streamTimedOut = false;
      let streamAbortReason = "";
      let streamBuffer = "";
      const NO_EVENT_TIMEOUT_MS = 30000;
      const MAX_STREAM_WINDOW_MS = 180000;

      const waitForRunToSettle = async (targetRunId: string, timeoutMs: number) => {
        const startedAt = Date.now();
        while (Date.now() - startedAt < timeoutMs) {
          const active = await getActiveRunId().catch(() => targetRunId);
          await loadMessagesForSession(sessionId);
          if (!active || active !== targetRunId) return true;
          await new Promise((resolve) => window.setTimeout(resolve, 350));
        }
        return false;
      };

      const waitForAssistantReply = async (timeoutMs: number) => {
        const startedAt = Date.now();
        let latest: ChatMessage[] = [];
        while (Date.now() - startedAt < timeoutMs) {
          latest = await loadMessagesForSession(sessionId);
          if (countAssistantReplies(latest) > assistantRepliesBeforeSend) return true;
          await new Promise((resolve) => window.setTimeout(resolve, 300));
        }
        return countAssistantReplies(latest) > assistantRepliesBeforeSend;
      };

      const resetNoEventTimer = () => {
        if (noEventTimerRef.current) window.clearTimeout(noEventTimerRef.current);
        noEventTimerRef.current = window.setTimeout(() => {
          streamTimedOut = true;
          streamAbortReason = "no-events-timeout";
          streamAbort.abort("no-events-timeout");
        }, NO_EVENT_TIMEOUT_MS);
      };

      resetNoEventTimer();
      if (maxStreamTimerRef.current) window.clearTimeout(maxStreamTimerRef.current);
      maxStreamTimerRef.current = window.setTimeout(() => {
        streamTimedOut = true;
        streamAbortReason = "max-stream-window";
        streamAbort.abort("max-stream-window");
      }, MAX_STREAM_WINDOW_MS);

      try {
        for await (const rawEvent of client.stream(sessionId, runId, {
          signal: streamAbort.signal,
        })) {
          const event: any = rawEvent;
          if (isRunSignalEvent(event.type)) resetNoEventTimer();
          recordRunTimeline(event.type, event.properties || {}, runId);

          if (
            event.type === "approval.requested" ||
            event.type === "permission.request" ||
            event.type === "permission.asked"
          ) {
            const req = normalizePermissionRequest(event.properties || {});
            if (req && sameSession(req.sessionId, sessionId)) {
              upsertPermissionRequest(req);
              void autoApprovePendingRequests();
            } else {
              void refreshPermissionRequests();
            }
          }

          if (
            event.type === "approval.resolved" ||
            event.type === "permission.resolved" ||
            event.type === "permission.replied"
          ) {
            const req = normalizePermissionRequest(event.properties || {});
            if (req?.id) removePermissionRequest(req.id);
            void refreshPermissionRequests();
          }

          const evRunId = extractRunId(event);
          if (evRunId && evRunId !== runId) continue;
          const eventTypeLower = String(event.type || "").toLowerCase();
          const isKnownToolEvent =
            TOOL_START_EVENTS.has(event.type) ||
            TOOL_PROGRESS_EVENTS.has(event.type) ||
            TOOL_END_EVENTS.has(event.type);
          if (!isKnownToolEvent && eventTypeLower.includes("tool")) {
            recordToolEvent(event.type, event.properties || {}, evRunId || runId);
          }

          if (event.type === "session.response") {
            const delta = String(event.properties?.delta || "");
            if (!delta) continue;
            gotDelta = true;
            streamBuffer += delta;
            setShowThinking(false);
            setStreamingText((prev) => `${prev}${delta}`);
          }

          if (TOOL_START_EVENTS.has(event.type)) {
            recordToolEvent(event.type, event.properties || {}, evRunId || runId);
          }

          if (TOOL_PROGRESS_EVENTS.has(event.type)) {
            recordToolEvent(event.type, event.properties || {}, evRunId || runId);
          }

          if (TOOL_END_EVENTS.has(event.type)) {
            recordToolEvent(event.type, event.properties || {}, evRunId || runId);
          }

          if (event.type === "message.part.updated") {
            const part = event.properties?.part || {};
            const partType = String(part.type || "")
              .trim()
              .toLowerCase()
              .replace(/_/g, "-");
            const tool = extractToolName(part) || extractToolName(event.properties);
            const partId = extractToolCallId(part);
            const partState = part?.state;
            const partStatus = String(
              (partState && typeof partState === "object" ? partState.status : partState) ||
                part.status ||
                ""
            )
              .trim()
              .toLowerCase();
            const hasError =
              !!part.error ||
              !!(partState && typeof partState === "object" && partState.error) ||
              partStatus.includes("fail") ||
              partStatus.includes("error") ||
              partStatus.includes("deny") ||
              partStatus.includes("reject") ||
              partStatus.includes("cancel");
            const hasOutput =
              !!part.result ||
              !!part.output ||
              !!(
                partState &&
                typeof partState === "object" &&
                (partState.output || partState.result)
              ) ||
              partStatus.includes("done") ||
              partStatus.includes("complete") ||
              partStatus.includes("success");

            if (tool && (partType === "tool" || partType === "tool-invocation")) {
              const status: ToolActivityStatus = hasError
                ? "failed"
                : hasOutput
                  ? "completed"
                  : "started";
              recordToolActivity(tool, status, `${partId || evRunId || runId}:${tool}:${status}`, {
                callId: partId,
                runId: evRunId || runId,
                source: "message.part.updated",
                summary: summarizeToolPayload(part),
                detail: summarizeToolPayload(part),
              });
            }
            if (tool && partType === "tool-result") {
              recordToolActivity(
                tool,
                hasError ? "failed" : "completed",
                `${partId || evRunId || runId}:${tool}:${hasError ? "failed" : "completed"}`,
                {
                  callId: partId,
                  runId: evRunId || runId,
                  source: "message.part.updated",
                  summary: summarizeToolPayload(part),
                  detail: summarizeToolPayload(part),
                }
              );
            }
          }

          if (TERMINAL_FAILURE_EVENTS.has(event.type)) {
            throw new Error(String(event.properties?.error || "Run failed."));
          }
          if (
            (event.type === "session.updated" || event.type === "session.status") &&
            String(event.properties?.status || "").toLowerCase() === "idle"
          ) {
            break;
          }
          if (TERMINAL_SUCCESS_EVENTS.has(event.type)) break;
        }
      } catch (streamErr: any) {
        const errText = String(streamErr?.message || streamErr || "").toLowerCase();
        const isAbortLike =
          streamTimedOut ||
          errText.includes("abort") ||
          errText.includes("terminated") ||
          errText.includes("networkerror");
        if (!isAbortLike) throw streamErr;
      } finally {
        if (noEventTimerRef.current) window.clearTimeout(noEventTimerRef.current);
        if (maxStreamTimerRef.current) window.clearTimeout(maxStreamTimerRef.current);
      }

      if (streamTimedOut) {
        const settled = await waitForRunToSettle(runId, 45000);
        await loadMessagesForSession(sessionId);
        if (!settled) {
          throw new Error(
            "Run stream timed out and the run is still active. Check logs and retry."
          );
        }
      }

      if (!gotDelta) {
        setShowThinking(true);
      }

      let assistantReplyLoaded = await waitForAssistantReply(gotDelta ? 3500 : 9000);

      if (!gotDelta) {
        const activeAfter = await getActiveRunId().catch(() => "");
        if (activeAfter === runId) {
          const settled = await waitForRunToSettle(runId, 30000);
          if (settled) {
            assistantReplyLoaded = await waitForAssistantReply(5000);
          } else {
            throw new Error(
              `Run ${runId} is still active without a final response (${streamAbortReason || "stream-ended"}).`
            );
          }
        }
      }

      if (!assistantReplyLoaded && streamBuffer.trim()) {
        setMessages((prev) =>
          appendUniqueAssistantMessage(prev, runId, identity.botName || "Assistant", streamBuffer)
        );
      }

      setStreamingText("");
      setShowThinking(false);
      await refreshSessions();
      await queryClient.invalidateQueries({ queryKey: ["chat"] }).catch(() => {});
    } catch (error: any) {
      const raw = toTextError(error);
      const msg =
        raw.includes("no-events-timeout") ||
        raw.includes("max-stream-window") ||
        raw.includes("AbortError") ||
        raw.toLowerCase().includes("terminated")
          ? "Run stream timed out before events were received. Check engine/provider logs and retry."
          : raw;
      toast("err", msg);
      await refreshMessages();
    } finally {
      if (mountedRef.current) {
        setSending(false);
        setShowThinking(false);
      }
    }
  }, [
    api,
    appendTransientUserMessage,
    autoApprovePendingRequests,
    client,
    createSession,
    identity.botName,
    loadMessagesForSession,
    messages,
    prompt,
    queryClient,
    recordRunTimeline,
    recordToolActivity,
    recordToolEvent,
    refreshMessages,
    refreshPermissionRequests,
    refreshSessions,
    removePermissionRequest,
    resolveModelRoute,
    selectedSessionId,
    selectedTools,
    sending,
    toast,
    uploads,
    upsertPermissionRequest,
  ]);

  useEffect(() => {
    void refreshSessions();
    void refreshAvailableTools();

    const onNewChat = async () => {
      const sid = await createSession().catch(() => "");
      if (sid) setSelectedSessionId(sid);
      setSessionsOpen(false);
    };

    window.addEventListener("tcp:new-chat", onNewChat as EventListener);

    const sessionsPoll = window.setInterval(() => {
      void refreshSessions();
    }, 8000);

    return () => {
      window.removeEventListener("tcp:new-chat", onNewChat as EventListener);
      window.clearInterval(sessionsPoll);
    };
  }, [createSession, refreshAvailableTools, refreshSessions]);

  useEffect(() => {
    if (!selectedSessionId) return;
    runAbortRef.current?.abort();
    setSessionsOpen(false);
    resetToolTracking();
    resetRunTimeline();
    void refreshPermissionRequests();
    void refreshMessages();
  }, [
    refreshMessages,
    refreshPermissionRequests,
    resetRunTimeline,
    resetToolTracking,
    selectedSessionId,
  ]);

  useEffect(() => {
    if (selectedSessionId) return;
    setMessages([]);
  }, [selectedSessionId]);

  useEffect(() => {
    if (!selectedSessionId && sessions.length === 0) {
      void createSession().catch(() => {});
    }
  }, [createSession, selectedSessionId, sessions.length]);

  useEffect(() => {
    saveAutoApprovePreference(autoApprove);
    if (autoApprove) void autoApprovePendingRequests();
  }, [autoApprove, autoApprovePendingRequests]);

  useEffect(() => {
    saveToolAllowlistPreference(selectedTools);
  }, [selectedTools]);

  useEffect(() => {
    const poll = window.setInterval(() => {
      if (!selectedSessionId) return;
      void refreshPermissionRequests();
    }, 2500);
    return () => window.clearInterval(poll);
  }, [refreshPermissionRequests, selectedSessionId]);

  useEffect(() => {
    const unsubscribe = subscribeSse(
      "/api/engine/global/event",
      (event: MessageEvent) => {
        let payload: any = null;
        try {
          payload = JSON.parse(String(event.data || "{}"));
        } catch {
          return;
        }

        const type = String(payload?.type || "").trim();
        if (!type) return;

        const props = payload?.properties || {};
        const req = normalizePermissionRequest(props);
        if (req && sameSession(req.sessionId, selectedSessionId)) {
          if (
            type === "approval.requested" ||
            type === "permission.request" ||
            type === "permission.asked"
          ) {
            upsertPermissionRequest(req);
          }
          if (
            type === "approval.resolved" ||
            type === "permission.resolved" ||
            type === "permission.replied"
          ) {
            removePermissionRequest(req.id);
          }
        }

        recordRunTimeline(type, props, extractRunId(payload, "global"));

        const isKnownToolEvent =
          TOOL_START_EVENTS.has(type) ||
          TOOL_PROGRESS_EVENTS.has(type) ||
          TOOL_END_EVENTS.has(type);
        if (!isKnownToolEvent && type.toLowerCase().includes("tool")) {
          const runId = extractRunId(payload, "global");
          recordToolEvent(type, props, runId);
        }

        if (TOOL_START_EVENTS.has(type)) {
          const runId = extractRunId(payload, "global");
          recordToolEvent(type, props, runId);
        }

        if (TOOL_PROGRESS_EVENTS.has(type)) {
          const runId = extractRunId(payload, "global");
          recordToolEvent(type, props, runId);
        }

        if (TOOL_END_EVENTS.has(type)) {
          const runId = extractRunId(payload, "global");
          recordToolEvent(type, props, runId);
        }
      },
      {
        withCredentials: true,
      }
    );

    return () => unsubscribe();
  }, [
    recordRunTimeline,
    recordToolEvent,
    removePermissionRequest,
    selectedSessionId,
    upsertPermissionRequest,
  ]);

  const attachedCount = uploads.length;
  const messagePaneEmpty = !messagesLoading && !messages.length && !showThinking && !streamingText;
  const workflowNudgePrompt = prompt.trim();
  const showWorkflowNudge =
    !sending &&
    workflowNudgePrompt.length > 0 &&
    workflowNudgePrompt !== workflowNudgeDismissedFor &&
    /\b(workflow|workflows)\b/i.test(workflowNudgePrompt);
  const openWorkflowPlannerFromPrompt = () => {
    seedWorkflowPlanner({
      prompt: workflowNudgePrompt,
      plan_source: "control_panel_chat",
      session_id: selectedSessionId || "",
    });
    setWorkflowNudgeDismissedFor(workflowNudgePrompt);
    navigate("planner");
  };

  const railSections = (
    <>
          <section className="chat-rail-section chat-rail-tools-section">
            <div className="mb-2 flex items-center justify-between">
              <p className="chat-rail-label">Tools</p>
              <div className="flex items-center gap-2">
                <span className="chat-rail-count">
                  {selectedTools.length
                    ? `${selectedTools.length}/${availableTools.length}`
                    : "all"}
                </span>
                {selectedTools.length ? (
                  <button
                    type="button"
                    className="tcp-btn h-7 px-2 tcp-text-caption"
                    onClick={() => setSelectedTools([])}
                  >
                    All
                  </button>
                ) : null}
              </div>
            </div>
            <div className="chat-tools-list">
              {availableTools.length ? (
                availableTools.slice(0, 48).map((tool) => {
                  const selected = selectedToolSet.has(tool);
                  return (
                    <button
                      key={tool}
                      type="button"
                      className={`chat-tool-pill ${selected ? "selected" : ""}`}
                      title={
                        selected
                          ? `Remove ${tool} from the next-run allowlist`
                          : `Allow only selected tools; add ${tool}`
                      }
                      aria-pressed={selected}
                      onClick={() => {
                        setSelectedTools((prev) => {
                          const current = new Set(prev);
                          if (current.has(tool)) current.delete(tool);
                          else current.add(tool);
                          return [...current].sort((a, b) => a.localeCompare(b));
                        });
                      }}
                    >
                      {selected ? <Icon name="check" /> : null}
                      {tool}
                    </button>
                  );
                })
              ) : (
                <p className="chat-rail-empty">No tools loaded.</p>
              )}
            </div>
            <p className="chat-rail-empty mt-2">
              {selectedTools.length
                ? "Selected tools are sent as the next run's allowlist."
                : "No allowlist selected; the run can use the default tool set."}
            </p>
          </section>

          <section className="chat-rail-section chat-rail-approvals-section">
            <div className="mb-2 flex items-center justify-between">
              <p className="chat-rail-label">Approvals</p>
              <span className="chat-rail-count">{permissions.length}</span>
            </div>
            <div className="mb-2 flex items-center gap-2">
              <button
                type="button"
                className="tcp-btn h-7 px-2 tcp-text-caption"
                disabled={!permissions.length || autoApproveInFlight}
                onClick={async () => {
                  const pendingIds = permissions.map((req) => req.id).filter(Boolean);
                  if (!pendingIds.length) return;
                  for (const requestId of pendingIds) {
                    await replyPermission(requestId, "once", true);
                  }
                  await refreshPermissionRequests();
                  const unresolved = pendingIds.filter((id) =>
                    permissions.some((req) => String(req.id || "").trim() === id)
                  ).length;
                  if (unresolved > 0) {
                    toast(
                      "warn",
                      `${unresolved} request${unresolved === 1 ? "" : "s"} still pending (likely stale/expired).`
                    );
                  } else {
                    toast(
                      "ok",
                      `Approved ${pendingIds.length} pending request${pendingIds.length === 1 ? "" : "s"}.`
                    );
                  }
                }}
              >
                Approve all
              </button>
              <label className="chat-auto-approve-label">
                <input
                  type="checkbox"
                  className="chat-auto-approve-checkbox"
                  checked={autoApprove}
                  onChange={(event) => setAutoApprove((event.target as HTMLInputElement).checked)}
                />
                Auto
              </label>
            </div>
            <div className="chat-tools-activity">
              {permissions.length ? (
                permissions.slice(0, 20).map((req) => {
                  const busy = permissionBusy.has(req.id);
                  const bits = [req.permission, req.pattern].filter(Boolean).join(" ");
                  return (
                    <article key={req.id} className="chat-pack-event-card">
                      <div className="chat-pack-event-title truncate" title={req.id}>
                        {req.tool}
                      </div>
                      <div className="chat-pack-event-summary mt-0.5">{bits || req.id}</div>
                      <div className="mt-1 flex gap-1">
                        <button
                          className="tcp-btn h-6 px-1.5 tcp-text-micro"
                          disabled={busy}
                          onClick={() => void replyPermission(req.id, "once")}
                        >
                          Allow
                        </button>
                        <button
                          className="tcp-btn h-6 px-1.5 tcp-text-micro"
                          disabled={busy}
                          onClick={() => void replyPermission(req.id, "always")}
                        >
                          Always
                        </button>
                        <button
                          className="tcp-btn-danger h-6 px-1.5 tcp-text-micro"
                          disabled={busy}
                          onClick={() => void replyPermission(req.id, "deny")}
                        >
                          Deny
                        </button>
                      </div>
                    </article>
                  );
                })
              ) : (
                <p className="chat-rail-empty">No pending approvals.</p>
              )}
            </div>
          </section>

          <section className="chat-rail-section chat-rail-scroll-section">
            <div className="mb-2 flex items-center justify-between">
              <p className="chat-rail-label">Run Timeline</p>
              <div className="flex items-center gap-2">
                <span className="chat-rail-count">{runTimeline.length}</span>
                <button
                  type="button"
                  className="tcp-btn h-7 px-2 tcp-text-caption"
                  onClick={resetRunTimeline}
                >
                  <Icon name="trash-2" />
                  Clear
                </button>
              </div>
            </div>
            <div className="chat-tools-activity">
              {runTimeline.length ? (
                runTimeline.slice(0, 24).map((event) => (
                  <motion.article
                    key={`${event.id}-${event.at}`}
                    className={`chat-timeline-card chat-timeline-${event.tone}`}
                    initial={reducedMotion ? false : { opacity: 0, y: 6 }}
                    animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <div className="chat-pack-event-title truncate" title={event.type}>
                        {event.title}
                      </div>
                      <span className="chat-pack-event-time">
                        {new Date(event.at).toLocaleTimeString()}
                      </span>
                    </div>
                    <div className="chat-pack-event-summary mt-0.5">{event.summary}</div>
                    {event.runId ? (
                      <div className="chat-pack-event-summary mt-1 truncate" title={event.runId}>
                        run: {event.runId}
                      </div>
                    ) : null}
                  </motion.article>
                ))
              ) : (
                <p className="chat-rail-empty">No run events yet.</p>
              )}
            </div>
          </section>

          <section className="chat-rail-section chat-rail-scroll-section">
            <div className="mb-2 flex items-center justify-between">
              <p className="chat-rail-label">Tool Activity</p>
              <button
                type="button"
                className="tcp-btn h-7 px-2 tcp-text-caption"
                onClick={resetToolTracking}
              >
                <Icon name="trash-2" />
                Clear
              </button>
            </div>
            <div className="chat-tools-activity">
              {toolActivity.length ? (
                toolActivity.slice(0, 24).map((entry) => (
                  <details
                    key={entry.id}
                    className={`chat-tool-event ${toolStatusClass(entry.status)}`}
                  >
                    <summary>
                      <span className="truncate">{entry.tool}</span>
                      <span>{entry.status}</span>
                    </summary>
                    <div className="chat-tool-event-body">
                      <div>{new Date(entry.at).toLocaleTimeString()}</div>
                      {entry.source ? <div>source: {entry.source}</div> : null}
                      {entry.callId ? <div>call: {entry.callId}</div> : null}
                      {entry.runId ? <div>run: {entry.runId}</div> : null}
                      {entry.summary ? <pre>{entry.summary}</pre> : null}
                    </div>
                  </details>
                ))
              ) : (
                <p className="chat-rail-empty">No tool events yet.</p>
              )}
            </div>
          </section>
    </>
  );

  return (
    <div
      ref={rootRef}
      className="chat-layout chat-layout-fill min-w-0 min-h-0 h-full w-full flex-1"
    >
      <motion.aside
        className={`chat-sessions-panel ${sessionsOpen ? "open" : ""}`}
        initial={false}
        animate={
          reducedMotion
            ? { x: sessionsOpen ? 0 : "-104%" }
            : { x: sessionsOpen ? 0 : "-104%", transition: { duration: 0.18, ease: "easeOut" } }
        }
      >
        <div className="chat-sessions-header">
          <h3 className="chat-sessions-title">
            <Icon name="history" />
            Sessions
          </h3>
          <div className="flex items-center gap-1">
            <button
              type="button"
              className="tcp-btn h-8 px-2.5 text-xs"
              onClick={() => {
                void createSession().catch((error) => toast("err", toTextError(error)));
                setSessionsOpen(false);
              }}
            >
              <Icon name="plus" />
              New
            </button>
            <button
              type="button"
              aria-label="Refresh sessions"
              className="tcp-btn h-8 px-2.5 text-xs"
              onClick={() => void refreshSessions()}
            >
              <Icon name="refresh-cw" />
            </button>
          </div>
        </div>

        <div className="chat-session-list">
          <AnimatePresence>
            {sessions.map((session) => (
              <motion.div
                key={session.id}
                className="chat-session-row"
                initial={reducedMotion ? false : { opacity: 0, y: 6 }}
                animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
                exit={reducedMotion ? undefined : { opacity: 0, y: -6 }}
              >
                <button
                  type="button"
                  className={`chat-session-btn ${session.id === selectedSessionId ? "active" : ""}`}
                  title={session.id}
                  onClick={() => {
                    setSelectedSessionId(session.id);
                    setSessionsOpen(false);
                  }}
                >
                  <span className="block truncate">{session.title}</span>
                </button>
                <button
                  type="button"
                  aria-label="Delete session"
                  className="chat-session-del"
                  title="Delete session"
                  onClick={() => setDeleteConfirm({ id: session.id, title: session.title })}
                >
                  <Icon name="trash-2" />
                </button>
              </motion.div>
            ))}
          </AnimatePresence>

          {!sessions.length ? <p className="chat-rail-empty px-1 py-2">No sessions yet.</p> : null}
        </div>
      </motion.aside>

      <AnimatePresence>
        {sessionsOpen ? (
          <motion.button
            type="button"
            className="chat-scrim open"
            aria-label="Close sessions"
            initial={reducedMotion ? false : { opacity: 0 }}
            animate={reducedMotion ? undefined : { opacity: 1 }}
            exit={reducedMotion ? undefined : { opacity: 0 }}
            onClick={() => setSessionsOpen(false)}
          />
        ) : null}
      </AnimatePresence>

      <div className="chat-workspace min-h-0 min-w-0">
        <section className="chat-main-shell flex min-h-0 min-w-0 flex-col overflow-hidden">
          <header className="chat-main-header shrink-0">
            <button
              type="button"
              aria-label="Toggle sessions"
              className="chat-icon-btn h-8 w-8"
              title="Sessions"
              onClick={() => setSessionsOpen((prev) => !prev)}
            >
              <Icon name="history" />
            </button>
            <div className="chat-main-dot"></div>
            <h3 className="tcp-title chat-main-title">{sessionTitle}</h3>
            {availableTools.length ? (
              <span className="chat-main-tools">
                {selectedTools.length
                  ? `${selectedTools.length} enabled`
                  : `${availableTools.length} tools`}
              </span>
            ) : null}
            <button
              type="button"
              aria-label="Open tools, approvals, and activity"
              className="chat-icon-btn h-8 w-8 ml-auto xl:hidden"
              title="Tools, approvals, and activity"
              onClick={() => setRailDrawerOpen(true)}
            >
              <Icon name="panel-right-open" />
            </button>
          </header>

          {setupCard ? (
            <div className="mx-3 mb-2 rounded-xl border border-amber-500/30 bg-amber-500/8 p-3">
              <div className="mb-2 flex items-start justify-between gap-3">
                <div>
                  <div className="tcp-title text-sm">{setupCard.title}</div>
                  <div className="tcp-subtle text-sm">{setupCard.body}</div>
                </div>
                <button
                  type="button"
                  className="tcp-btn tcp-btn-ghost"
                  onClick={() => setSetupCard(null)}
                >
                  Dismiss
                </button>
              </div>
              {setupCard.clarifier ? (
                <div className="mb-2 flex flex-wrap gap-2">
                  {setupCard.clarifier.options.map((option) => (
                    <button
                      key={option.id}
                      type="button"
                      className="tcp-btn tcp-btn-ghost"
                      onClick={() => {
                        const isWorkflowPlanner = option.id.startsWith("workflow_planner_");
                        setSetupCard({
                          title:
                            option.id === "provider_setup"
                              ? "Provider setup"
                              : option.id === "integration_setup"
                                ? "Tool connection"
                                : isWorkflowPlanner
                                  ? "Workflow planning"
                                  : "Automation setup",
                          body:
                            option.id === "provider_setup"
                              ? "Open Providers to configure a provider."
                              : option.id === "integration_setup"
                                ? "Open MCP to connect the tool you need."
                                : isWorkflowPlanner
                                  ? "Open the planner to answer the missing workflow details."
                                  : "Open Automations to build the workflow.",
                          cta:
                            option.id === "provider_setup"
                              ? "Open Providers"
                              : option.id === "integration_setup"
                                ? "Open MCP"
                                : isWorkflowPlanner
                                  ? "Open Planner"
                                  : "Open Automations",
                          actionType:
                            option.id === "provider_setup"
                              ? "open_provider_setup"
                              : option.id === "integration_setup"
                                ? "open_mcp_setup"
                                : isWorkflowPlanner
                                  ? "open_planner"
                                  : "open_automations",
                          payload: setupCard.payload,
                        });
                      }}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              ) : null}
              <button
                type="button"
                className="tcp-btn"
                onClick={() => {
                  if (setupCard.actionType === "open_provider_setup") navigate("settings");
                  else if (setupCard.actionType === "open_mcp_setup") navigate("mcp");
                  else if (setupCard.actionType === "open_planner") {
                    seedWorkflowPlanner(setupCard.payload);
                    navigate("planner");
                  } else if (setupCard.actionType === "open_automations") {
                    seedAutomationPlanner(setupCard.payload);
                    navigate("automations");
                  }
                }}
              >
                {setupCard.cta}
              </button>
            </div>
          ) : null}

          <ChatInterfacePanel
            messages={messages.map((m) => ({
              id: m.id,
              role: m.role,
              displayRole: m.displayRole,
              text: m.text,
              markdown: m.markdown,
            }))}
            emptyText="No messages yet. Send a prompt to start."
            inputValue={prompt}
            inputPlaceholder="Ask anything... (Enter to send, Shift+Enter newline)"
            sendLabel="Send"
            onInputChange={setPrompt}
            onSend={() => void sendPrompt()}
            sendDisabled={sending}
            inputDisabled={sending}
            botIdentity={{ botName: identity.botName, botAvatarUrl: identity.botAvatarUrl }}
            streamingText={streamingText}
            showThinking={showThinking}
            thinkingText="Thinking"
            attachments={uploads.map((u) => ({ path: u.path, name: u.path, size: u.size }))}
            onOpenAttachment={(index) => {
              const file = uploads[index];
              if (!file?.path) return;
              openFilesExplorer(navigate, { path: file.path });
            }}
            onRemoveAttachment={(index) => setUploads((prev) => prev.filter((_, i) => i !== index))}
            onAttach={() => fileInputRef.current?.click()}
            attachDisabled={sending}
            statusTitle={sending && !showThinking && !streamingText ? "Sending…" : ""}
            composerAccessory={
              showWorkflowNudge ? (
                <div className="chat-planner-nudge" role="status">
                  <button
                    type="button"
                    className="chat-planner-nudge-action"
                    onClick={openWorkflowPlannerFromPrompt}
                  >
                    <Icon name="workflow" />
                    Open workflow planner
                  </button>
                  <span className="chat-planner-nudge-hint">for this message</span>
                  <button
                    type="button"
                    aria-label="Dismiss workflow suggestion"
                    className="chat-planner-nudge-dismiss"
                    title="Dismiss"
                    onClick={() => setWorkflowNudgeDismissedFor(workflowNudgePrompt)}
                  >
                    <Icon name="x" />
                  </button>
                </div>
              ) : null
            }
          />

          <input
            ref={fileInputRef}
            type="file"
            className="hidden"
            multiple
            onChange={(event) => {
              void uploadFiles((event.target as HTMLInputElement).files);
              (event.target as HTMLInputElement).value = "";
            }}
          />
        </section>

        <aside className="chat-right-rail hidden min-h-0 flex-col overflow-hidden xl:flex">
          {railSections}
        </aside>
      </div>

      <DetailDrawer
        open={railDrawerOpen}
        title="Tools, approvals & activity"
        onClose={() => setRailDrawerOpen(false)}
      >
        <div className="grid gap-4">{railSections}</div>
      </DetailDrawer>

      <AnimatePresence>
        {deleteConfirm ? (
          <motion.div
            className="tcp-confirm-overlay"
            initial={reducedMotion ? false : { opacity: 0 }}
            animate={reducedMotion ? undefined : { opacity: 1 }}
            exit={reducedMotion ? undefined : { opacity: 0 }}
            onClick={() => setDeleteConfirm(null)}
          >
            <motion.div
              className="tcp-confirm-dialog"
              initial={reducedMotion ? false : { opacity: 0, y: 8, scale: 0.98 }}
              animate={reducedMotion ? undefined : { opacity: 1, y: 0, scale: 1 }}
              exit={reducedMotion ? undefined : { opacity: 0, y: 6, scale: 0.98 }}
              onClick={(event) => event.stopPropagation()}
            >
              <h3 className="tcp-confirm-title">Delete session</h3>
              <p className="tcp-confirm-message">
                This will permanently remove <strong>{deleteConfirm.title}</strong> and all its
                messages.
              </p>
              <div className="tcp-confirm-actions">
                <button type="button" className="tcp-btn" onClick={() => setDeleteConfirm(null)}>
                  <Icon name="x" />
                  Cancel
                </button>
                <button
                  type="button"
                  className="tcp-btn-danger"
                  onClick={() =>
                    void removeSession(deleteConfirm.id).catch((error) =>
                      toast("err", toTextError(error))
                    )
                  }
                >
                  <Icon name="trash-2" />
                  Delete session
                </button>
              </div>
            </motion.div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
