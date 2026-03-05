import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { renderIcons } from "../app/icons.js";
import { renderMarkdownSafe } from "../lib/markdown";
import type { AppPageProps } from "./pageTypes";
import {
  type ChatSession,
  loadSessions,
  loadStoredSessionId,
  saveStoredSessionId,
} from "../features/chat/session";
import { normalizeMessages, type ChatMessage } from "../features/chat/messages";
import {
  extractRunId,
  extractToolCallId,
  extractToolName,
  isPendingPermissionStatus,
  normalizePackEvent,
  normalizePermissionRequest,
  TERMINAL_FAILURE_EVENTS,
  TERMINAL_SUCCESS_EVENTS,
  TOOL_END_EVENTS,
  TOOL_START_EVENTS,
  type PackEventItem,
  type PermissionRequest,
  type ToolActivityItem,
  type ToolActivityStatus,
} from "../features/chat/streaming";
import { subscribeSse } from "../services/sse.js";

const CHAT_UPLOAD_DIR = "control-panel";
const CHAT_AUTO_APPROVE_KEY = "tandem_control_panel_chat_auto_approve_tools";
const EXT_MIME: Record<string, string> = {
  md: "text/markdown",
  txt: "text/plain",
  csv: "text/csv",
  json: "application/json",
  pdf: "application/pdf",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
};

type UploadFile = {
  name: string;
  path: string;
  size: number;
  mime?: string;
  url?: string;
};

type UploadProgressRow = {
  id: string;
  name: string;
  progress: number;
  error: string;
};

type ConfirmDeleteState = { id: string; title: string } | null;

function inferMime(name = "") {
  const ext = String(name).toLowerCase().split(".").pop() || "";
  return EXT_MIME[ext] || "application/octet-stream";
}

function joinRootAndRel(root = "", rel = "") {
  if (!root || !rel) return rel || "";
  const lhs = String(root).replace(/[\\/]+$/, "");
  const rhs = String(rel).replace(/^[\\/]+/, "");
  return `${lhs}/${rhs}`;
}

function formatBytes(bytes: number) {
  const n = Number(bytes || 0);
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function sameSession(eventSessionId: string, activeSessionId: string) {
  if (!eventSessionId || !activeSessionId) return true;
  return String(eventSessionId).trim() === String(activeSessionId).trim();
}

function isRunSignalEvent(eventType: string) {
  const t = String(eventType || "").trim();
  return t !== "server.connected" && t !== "engine.lifecycle.ready";
}

function toolStatusClass(status: ToolActivityStatus) {
  if (status === "completed") return "chat-tool-chip-ok";
  if (status === "failed") return "chat-tool-chip-failed";
  return "chat-tool-chip-running";
}

function loadAutoApprovePreference() {
  try {
    return localStorage.getItem(CHAT_AUTO_APPROVE_KEY) === "1";
  } catch {
    return false;
  }
}

function saveAutoApprovePreference(enabled: boolean) {
  try {
    localStorage.setItem(CHAT_AUTO_APPROVE_KEY, enabled ? "1" : "0");
  } catch {
    // ignore
  }
}

function toTextError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
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
  const [packEvents, setPackEvents] = useState<PackEventItem[]>([]);
  const [packEventSeen, setPackEventSeen] = useState<Set<string>>(new Set());
  const [permissions, setPermissions] = useState<PermissionRequest[]>([]);
  const [permissionBusy, setPermissionBusy] = useState<Set<string>>(new Set());
  const [autoApprove, setAutoApprove] = useState(loadAutoApprovePreference());
  const [autoApproveInFlight, setAutoApproveInFlight] = useState(false);
  const [availableTools, setAvailableTools] = useState<string[]>([]);
  const [deleteConfirm, setDeleteConfirm] = useState<ConfirmDeleteState>(null);

  const sessionTitle = useMemo(() => {
    const hit = sessions.find((x) => x.id === selectedSessionId);
    return hit?.title || "Chat";
  }, [selectedSessionId, sessions]);

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
    const root = rootRef.current;
    if (root) renderIcons(root);
  }, [
    sessions,
    uploads,
    uploadRows,
    permissions,
    toolActivity,
    packEvents,
    messages,
    sessionsOpen,
    showThinking,
    streamingText,
  ]);

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
      const modelID = String(cfg?.providers?.[providerID]?.default_model || "").trim();
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
    },
    [identity.botName]
  );

  const refreshMessages = useCallback(async () => {
    if (!selectedSessionId) {
      setMessages([]);
      return;
    }
    setMessagesLoading(true);
    try {
      const rows = await client.sessions
        .messages(selectedSessionId)
        .catch(() => ({ messages: [] }));
      if (!mountedRef.current) return;
      normalizeAndSetMessages(rows);
    } finally {
      if (mountedRef.current) setMessagesLoading(false);
    }
  }, [client.sessions, normalizeAndSetMessages, selectedSessionId]);

  const resetToolTracking = useCallback(() => {
    setToolActivity([]);
    setToolEventSeen(new Set());
  }, []);

  const resetPackTracking = useCallback(() => {
    setPackEvents([]);
    setPackEventSeen(new Set());
  }, []);

  const recordToolActivity = useCallback(
    (toolName: string, status: ToolActivityStatus, eventKey = "") => {
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
          },
          ...prev,
        ];
        return next.slice(0, 80);
      });
    },
    []
  );

  const recordPackEvent = useCallback((rawType: string, rawProps: any) => {
    const normalized = normalizePackEvent(rawType, rawProps);
    const lower = String(normalized.type).toLowerCase();
    if (!lower.startsWith("pack.")) return;

    let accepted = true;
    setPackEventSeen((prev) => {
      if (prev.has(normalized.id)) {
        accepted = false;
        return prev;
      }
      const next = new Set(prev);
      next.add(normalized.id);
      if (next.size > 400) return new Set([normalized.id]);
      return next;
    });
    if (!accepted) return;

    setPackEvents((prev) => [normalized, ...prev].slice(0, 80));
  }, []);

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
      .filter(Boolean);
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

    setPrompt("");
    setSending(true);
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

      const parts = attached.map((file) => ({
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

      const cancelAndWaitForIdle = async () => {
        const activeRunId = await getActiveRunId().catch(() => "");
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
          }),
        });

      let runResp = await startRun();
      let runId = "";

      if (runResp.status === 409) {
        const idle = await cancelAndWaitForIdle();
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

      setStreamingText("");
      setShowThinking(true);

      const streamAbort = new AbortController();
      runAbortRef.current?.abort();
      runAbortRef.current = streamAbort;

      let gotDelta = false;
      let streamTimedOut = false;
      let streamAbortReason = "";
      const NO_EVENT_TIMEOUT_MS = 30000;
      const MAX_STREAM_WINDOW_MS = 180000;

      const waitForRunToSettle = async (targetRunId: string, timeoutMs: number) => {
        const startedAt = Date.now();
        while (Date.now() - startedAt < timeoutMs) {
          const active = await getActiveRunId().catch(() => targetRunId);
          await refreshMessages();
          if (!active || active !== targetRunId) return true;
          await new Promise((resolve) => window.setTimeout(resolve, 350));
        }
        return false;
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
        for await (const event of client.stream(sessionId, runId, { signal: streamAbort.signal })) {
          if (isRunSignalEvent(event.type)) resetNoEventTimer();

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

          if (
            String(event.type || "")
              .toLowerCase()
              .startsWith("pack.")
          ) {
            recordPackEvent(event.type, event.properties || {});
          }

          const evRunId = extractRunId(event);
          if (evRunId && evRunId !== runId) continue;

          if (event.type === "session.response") {
            const delta = String(event.properties?.delta || "");
            if (!delta) continue;
            gotDelta = true;
            setShowThinking(false);
            setStreamingText((prev) => `${prev}${delta}`);
          }

          if (TOOL_START_EVENTS.has(event.type)) {
            const tool = extractToolName(event.properties) || "tool";
            const callId = extractToolCallId(event.properties);
            recordToolActivity(tool, "started", `${callId || evRunId || runId}:${tool}:started`);
          }

          if (TOOL_END_EVENTS.has(event.type)) {
            const tool = extractToolName(event.properties) || "tool";
            const callId = extractToolCallId(event.properties);
            const statusHint = String(
              event.properties?.status || event.properties?.state || ""
            ).toLowerCase();
            const failed =
              event.type === "tool_call.failed" ||
              statusHint.includes("fail") ||
              statusHint.includes("error") ||
              !!event.properties?.error;
            recordToolActivity(
              tool,
              failed ? "failed" : "completed",
              `${callId || evRunId || runId}:${tool}:${failed ? "failed" : "completed"}`
            );
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
              recordToolActivity(tool, status, `${partId || evRunId || runId}:${tool}:${status}`);
            }
            if (tool && partType === "tool-result") {
              recordToolActivity(
                tool,
                hasError ? "failed" : "completed",
                `${partId || evRunId || runId}:${tool}:${hasError ? "failed" : "completed"}`
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
        await refreshMessages();
        if (!settled) {
          throw new Error(
            "Run stream timed out and the run is still active. Check logs and retry."
          );
        }
      }

      if (!gotDelta) {
        setShowThinking(true);
      }

      await refreshMessages();
      await new Promise((resolve) => window.setTimeout(resolve, 180));
      await refreshMessages();
      await new Promise((resolve) => window.setTimeout(resolve, 220));
      await refreshMessages();

      if (!gotDelta) {
        const activeAfter = await getActiveRunId().catch(() => "");
        if (activeAfter === runId) {
          const settled = await waitForRunToSettle(runId, 30000);
          if (settled) {
            await refreshMessages();
          } else {
            throw new Error(
              `Run ${runId} is still active without a final response (${streamAbortReason || "stream-ended"}).`
            );
          }
        }
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
    prompt,
    queryClient,
    recordPackEvent,
    recordToolActivity,
    refreshMessages,
    refreshPermissionRequests,
    refreshSessions,
    removePermissionRequest,
    resolveModelRoute,
    selectedSessionId,
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
    resetPackTracking();
    void refreshPermissionRequests();
    void refreshMessages();
  }, [
    refreshMessages,
    refreshPermissionRequests,
    resetPackTracking,
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
    const poll = window.setInterval(() => {
      if (!selectedSessionId) return;
      void refreshPermissionRequests();
    }, 2500);
    return () => window.clearInterval(poll);
  }, [refreshPermissionRequests, selectedSessionId]);

  useEffect(() => {
    const unsubscribe = subscribeSse(
      "/api/global/event",
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

        if (type.toLowerCase().startsWith("pack.")) {
          recordPackEvent(type, props);
        }

        if (TOOL_START_EVENTS.has(type)) {
          const tool = extractToolName(props) || "tool";
          const callId = extractToolCallId(props);
          const runId = extractRunId(payload, "global");
          recordToolActivity(tool, "started", `${callId || runId}:${tool}:started`);
        }

        if (TOOL_END_EVENTS.has(type)) {
          const tool = extractToolName(props) || "tool";
          const callId = extractToolCallId(props);
          const runId = extractRunId(payload, "global");
          const statusHint = String(props?.status || props?.state || "").toLowerCase();
          const failed =
            type === "tool_call.failed" || statusHint.includes("fail") || !!props?.error;
          recordToolActivity(
            tool,
            failed ? "failed" : "completed",
            `${callId || runId}:${tool}:${failed ? "failed" : "completed"}`
          );
        }
      },
      {
        withCredentials: true,
      }
    );

    return () => unsubscribe();
  }, [
    recordPackEvent,
    recordToolActivity,
    removePermissionRequest,
    selectedSessionId,
    upsertPermissionRequest,
  ]);

  const attachedCount = uploads.length;

  return (
    <div ref={rootRef} className="chat-layout min-w-0 min-h-0 h-full flex-1">
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
            <i data-lucide="history"></i>
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
              <i data-lucide="plus"></i>
              New
            </button>
            <button
              type="button"
              className="tcp-btn h-8 px-2.5 text-xs"
              onClick={() => void refreshSessions()}
            >
              <i data-lucide="refresh-cw"></i>
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
                  className="chat-session-del"
                  title="Delete session"
                  onClick={() => setDeleteConfirm({ id: session.id, title: session.title })}
                >
                  <i data-lucide="trash-2"></i>
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
              className="chat-icon-btn h-8 w-8"
              title="Sessions"
              onClick={() => setSessionsOpen((prev) => !prev)}
            >
              <i data-lucide="history"></i>
            </button>
            <div className="chat-main-dot"></div>
            <h3 className="tcp-title chat-main-title">{sessionTitle}</h3>
            {availableTools.length ? (
              <span className="chat-main-tools">{availableTools.length} tools</span>
            ) : null}
          </header>

          <div
            ref={messagesRef}
            className="chat-messages mb-2 min-h-0 min-w-0 flex-1 space-y-2 overflow-auto p-3"
          >
            {messagesLoading && !messages.length ? (
              <p className="chat-rail-empty">Loading messages...</p>
            ) : null}

            {!messagesLoading && !messages.length && !showThinking && !streamingText ? (
              <p className="chat-rail-empty">No messages yet. Send a prompt to start.</p>
            ) : null}

            <AnimatePresence initial={false}>
              {messages.map((message) => {
                const assistantLike = message.role === "assistant" || message.role === "system";
                const roleClass = assistantLike ? "assistant" : "user";
                return (
                  <motion.article
                    key={message.id}
                    className={`chat-msg ${roleClass}`}
                    initial={reducedMotion ? false : { opacity: 0, y: 4 }}
                    animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
                    exit={reducedMotion ? undefined : { opacity: 0, y: -4 }}
                  >
                    <div className="chat-msg-role">
                      {assistantLike ? (
                        <span className="inline-flex items-center gap-2">
                          {identity.botAvatarUrl ? (
                            <img
                              src={identity.botAvatarUrl}
                              alt={identity.botName}
                              className="chat-avatar-ring h-5 w-5 rounded-full object-cover"
                            />
                          ) : null}
                          <span>{message.displayRole}</span>
                        </span>
                      ) : (
                        message.displayRole
                      )}
                    </div>
                    {message.markdown ? (
                      <div
                        className="tcp-markdown tcp-markdown-ai"
                        dangerouslySetInnerHTML={{ __html: renderMarkdownSafe(message.text || "") }}
                      />
                    ) : (
                      <pre className="chat-msg-pre">{message.text}</pre>
                    )}
                  </motion.article>
                );
              })}
            </AnimatePresence>

            {showThinking || streamingText ? (
              <article className="chat-msg assistant">
                <div className="chat-msg-role">
                  <span className="inline-flex items-center gap-2">
                    {identity.botAvatarUrl ? (
                      <img
                        src={identity.botAvatarUrl}
                        alt={identity.botName}
                        className="chat-avatar-ring h-5 w-5 rounded-full object-cover"
                      />
                    ) : null}
                    <span>{identity.botName || "Assistant"}</span>
                  </span>
                </div>
                {showThinking && !streamingText ? (
                  <div className="tcp-thinking" aria-live="polite">
                    <span>Thinking</span>
                    <i></i>
                    <i></i>
                    <i></i>
                  </div>
                ) : null}
                {streamingText ? <pre className="chat-msg-pre">{streamingText}</pre> : null}
              </article>
            ) : null}
          </div>

          <div className="chat-composer shrink-0">
            <div className={`chat-attach-row mb-2 ${attachedCount ? "" : "hidden"}`}>
              <span className="tcp-subtle">{attachedCount} attached</span>
              <div className="chat-files-line">
                {uploads.map((file, index) => (
                  <span key={`${file.path}-${index}`} className="chat-file-pill min-w-0">
                    <span className="chat-file-pill-name" title={file.path}>
                      {file.path}
                    </span>
                    <span className="chat-file-pill-size">{formatBytes(file.size)}</span>
                    <button
                      type="button"
                      className="chat-file-pill-btn chat-file-pill-btn-danger"
                      title="Remove from list"
                      onClick={() => setUploads((prev) => prev.filter((_, i) => i !== index))}
                    >
                      <i data-lucide="x"></i>
                    </button>
                  </span>
                ))}
              </div>
            </div>

            {!!uploadRows.length ? (
              <div className="mb-2 grid gap-1.5">
                {uploadRows.map((row) => {
                  const pct = Math.max(0, Math.min(100, Number(row.progress || 0)));
                  return (
                    <div key={row.id} className="chat-upload-card">
                      <div className="mb-1 flex items-center justify-between gap-2 text-xs">
                        <span className="chat-upload-name truncate">{row.name}</span>
                        <span className={row.error ? "chat-upload-meta-error" : "chat-upload-meta"}>
                          {row.error || `${pct.toFixed(0)}%`}
                        </span>
                      </div>
                      <div className="chat-upload-bar">
                        <div className="chat-upload-bar-fill" style={{ width: `${pct}%` }}></div>
                      </div>
                    </div>
                  );
                })}
              </div>
            ) : null}

            <div className="chat-input-wrap">
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
              <button
                type="button"
                className="chat-icon-btn chat-icon-btn-inner"
                title="Attach files"
                onClick={() => fileInputRef.current?.click()}
              >
                <i data-lucide="paperclip"></i>
              </button>
              <textarea
                ref={inputRef}
                rows={1}
                value={prompt}
                className="tcp-input chat-input-with-clip chat-input-modern resize-none"
                placeholder="Ask anything... (Enter to send, Shift+Enter newline)"
                onInput={(event) => setPrompt((event.target as HTMLTextAreaElement).value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault();
                    void sendPrompt();
                  }
                }}
              />
              <button
                type="button"
                className="chat-send-btn"
                title="Send"
                disabled={sending}
                onClick={() => void sendPrompt()}
              >
                <i data-lucide="send"></i>
              </button>
            </div>
          </div>
        </section>

        <aside className="chat-right-rail hidden min-h-0 flex-col gap-3 overflow-hidden xl:flex">
          <section className="min-h-0">
            <div className="mb-2 flex items-center justify-between">
              <p className="chat-rail-label">Tools</p>
              <span className="chat-rail-count">{availableTools.length}</span>
            </div>
            <div className="chat-tools-list">
              {availableTools.length ? (
                availableTools.slice(0, 32).map((tool) => (
                  <button
                    key={tool}
                    type="button"
                    className="chat-tool-pill"
                    title={`Insert ${tool}`}
                    onClick={() => {
                      setPrompt((prev) => (prev.trim() ? `${prev} ${tool}` : tool));
                      inputRef.current?.focus();
                    }}
                  >
                    {tool}
                  </button>
                ))
              ) : (
                <p className="chat-rail-empty">No tools loaded.</p>
              )}
            </div>
          </section>

          <section className="min-h-0">
            <div className="mb-2 flex items-center justify-between">
              <p className="chat-rail-label">Approvals</p>
              <span className="chat-rail-count">{permissions.length}</span>
            </div>
            <div className="mb-2 flex items-center gap-2">
              <button
                type="button"
                className="tcp-btn h-7 px-2 text-[11px]"
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
                          className="tcp-btn h-6 px-1.5 text-[10px]"
                          disabled={busy}
                          onClick={() => void replyPermission(req.id, "once")}
                        >
                          Allow
                        </button>
                        <button
                          className="tcp-btn h-6 px-1.5 text-[10px]"
                          disabled={busy}
                          onClick={() => void replyPermission(req.id, "always")}
                        >
                          Always
                        </button>
                        <button
                          className="tcp-btn-danger h-6 px-1.5 text-[10px]"
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

          <section className="min-h-0 flex-1">
            <div className="mb-2 flex items-center justify-between">
              <p className="chat-rail-label">Pack Events</p>
              <div className="flex items-center gap-2">
                <span className="chat-rail-count">{packEvents.length}</span>
                <button
                  type="button"
                  className="tcp-btn h-7 px-2 text-[11px]"
                  onClick={() => {
                    resetPackTracking();
                  }}
                >
                  <i data-lucide="trash-2"></i>
                  Clear
                </button>
              </div>
            </div>
            <div className="chat-tools-activity">
              {packEvents.length ? (
                packEvents.slice(0, 20).map((event) => (
                  <motion.article
                    key={`${event.id}-${event.at}`}
                    className="chat-pack-event-card"
                    initial={reducedMotion ? false : { opacity: 0, y: 6 }}
                    animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <div className="chat-pack-event-title truncate" title={event.type}>
                        {event.type}
                      </div>
                      <span className="chat-pack-event-time">
                        {new Date(event.at).toLocaleTimeString()}
                      </span>
                    </div>
                    <div className="chat-pack-event-summary mt-0.5">{event.summary}</div>
                    {event.error ? (
                      <div className="chat-pack-event-error mt-1">{event.error}</div>
                    ) : null}
                    <div className="mt-1 flex flex-wrap gap-1">
                      <button
                        type="button"
                        className="tcp-btn h-6 px-1.5 text-[10px]"
                        onClick={() => navigate("packs-detail")}
                      >
                        <i data-lucide="package"></i>
                        Packs
                      </button>
                      {event.path ? (
                        <button
                          type="button"
                          className="tcp-btn h-6 px-1.5 text-[10px]"
                          onClick={async () => {
                            try {
                              const payload = await client.packs.install({
                                path: event.path,
                                source: { kind: "control_panel_chat", event: "pack.detected" },
                              });
                              toast(
                                "ok",
                                `Installed ${payload?.installed?.name || "pack"} ${payload?.installed?.version || ""}`.trim()
                              );
                            } catch (error) {
                              toast("err", `Install failed: ${toTextError(error)}`);
                            }
                          }}
                        >
                          <i data-lucide="download"></i>
                          Install path
                        </button>
                      ) : null}
                      {event.path && event.attachmentId ? (
                        <button
                          type="button"
                          className="tcp-btn h-6 px-1.5 text-[10px]"
                          onClick={async () => {
                            try {
                              const payload = await client.packs.installFromAttachment({
                                attachment_id: event.attachmentId,
                                path: event.path,
                                connector: event.connector || undefined,
                                channel_id: event.channelId || undefined,
                                sender_id: event.senderId || undefined,
                              });
                              toast(
                                "ok",
                                `Installed ${payload?.installed?.name || "pack"} ${payload?.installed?.version || ""}`.trim()
                              );
                            } catch (error) {
                              toast("err", `Install failed: ${toTextError(error)}`);
                            }
                          }}
                        >
                          <i data-lucide="paperclip"></i>
                          Install attach
                        </button>
                      ) : null}
                    </div>
                  </motion.article>
                ))
              ) : (
                <p className="chat-rail-empty">No pack events yet.</p>
              )}
            </div>
          </section>

          <section className="min-h-0 flex-1">
            <div className="mb-2 flex items-center justify-between">
              <p className="chat-rail-label">Tool Activity</p>
              <button
                type="button"
                className="tcp-btn h-7 px-2 text-[11px]"
                onClick={resetToolTracking}
              >
                <i data-lucide="trash-2"></i>
                Clear
              </button>
            </div>
            <div className="chat-tools-activity">
              {toolActivity.length ? (
                toolActivity.slice(0, 24).map((entry) => (
                  <div key={entry.id} className={`chat-tool-chip ${toolStatusClass(entry.status)}`}>
                    {entry.tool}: {entry.status}
                  </div>
                ))
              ) : (
                <p className="chat-rail-empty">No tool events yet.</p>
              )}
            </div>
          </section>
        </aside>
      </div>

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
                  <i data-lucide="x"></i>
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
                  <i data-lucide="trash-2"></i>
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
