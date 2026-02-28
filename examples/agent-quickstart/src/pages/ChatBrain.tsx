import { useState, useEffect, useRef, useCallback } from "react";
import { client, promptAsyncWithModel } from "../api";
import type { EngineMessage, PermissionRequestRecord } from "@frumu/tandem-client";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSanitize from "rehype-sanitize";
import {
  Send,
  Plus,
  Trash2,
  Settings2,
  Loader2,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  AlertCircle,
  History,
  X,
  Zap,
  FileText,
  Code,
} from "lucide-react";

/* ─── Types ─── */
interface ChatMsg {
  id: string;
  role: "user" | "agent" | "system";
  type: "text" | "tool_start" | "tool_end";
  content: string;
  toolName?: string;
  toolResult?: string;
}

interface PendingApproval {
  id: string;
  tool: string;
  permission?: string;
  pattern?: string;
}
interface StoredSession {
  id: string;
  title: string;
  created: number;
}
interface McpAuthChallenge {
  challengeId: string;
  tool: string;
  server?: string;
  authorizationUrl: string;
  message: string;
  pending?: boolean;
  blocked?: boolean;
  retryAfterMs?: number;
}
interface ModelSpec {
  provider: string;
  model: string;
}
type ProviderCfg = {
  defaultModel?: string;
  default_model?: string;
};

const sanitizeModelText = (value: string): string => {
  let out = value;
  for (const marker of ["<|eom|>", "<|eot_id|>", "<|im_end|>", "<|end|>"]) {
    if (out.includes(marker))
      out = out.replace(new RegExp(marker.replace(/[|]/g, "\\$&"), "g"), "");
  }
  return out;
};

const eventRunId = (props: Record<string, unknown> | undefined): string | undefined => {
  const id = props?.runId ?? props?.runID ?? props?.run_id;
  return typeof id === "string" && id.trim() ? id : undefined;
};

const SESSIONS_KEY = "tandem_aq_sessions";
const ACTIVE_KEY = "tandem_aq_active_session";
const AUTO_ALLOW_KEY = "tandem_aq_auto_allow_all";
const PRIMED_PREFIX = "tandem_aq_primed_";
const PRIME_MARKER = "[AQ_PRIMED_V1]";
const SHOW_DEBUG_UI =
  typeof window !== "undefined" &&
  (window.location.hostname === "localhost" || window.location.hostname === "127.0.0.1");

/* ─── Session store helpers ─── */
const loadStoredSessions = (): StoredSession[] => {
  try {
    return JSON.parse(localStorage.getItem(SESSIONS_KEY) || "[]") as StoredSession[];
  } catch {
    return [];
  }
};
const saveStoredSessions = (s: StoredSession[]) =>
  localStorage.setItem(SESSIONS_KEY, JSON.stringify(s));

/* ─── Tool result component ─── */
function ToolResult({ name, result }: { name: string; result: string }) {
  const [open, setOpen] = useState(false);
  let display = result;
  try {
    const p = JSON.parse(result) as Record<string, unknown>;
    if (typeof p.markdown === "string") display = p.markdown;
    else if (typeof p.content === "string") display = p.content;
  } catch {
    /* raw string */
  }
  const large = display.length > 2000;

  return (
    <div className="border border-gray-800 rounded-xl overflow-hidden bg-gray-900/60 text-sm max-w-[85%] min-w-0">
      <button
        onClick={() => setOpen((o) => !o)}
        className="w-full flex items-center justify-between px-3 py-2 bg-gray-800/80 hover:bg-gray-700/60 transition-colors text-left gap-2"
      >
        <div className="flex items-center gap-2 min-w-0">
          {open ? (
            <ChevronDown size={14} className="shrink-0" />
          ) : (
            <ChevronRight size={14} className="shrink-0" />
          )}
          <CheckCircle2 size={14} className="text-emerald-400 shrink-0" />
          <span className="font-mono text-gray-200 truncate">{name}</span>
          <span className="text-[10px] text-emerald-400 bg-emerald-400/10 px-1.5 py-0.5 rounded shrink-0">
            done
          </span>
        </div>
        <div className="flex items-center gap-2 text-[11px] text-gray-500 font-mono shrink-0">
          {large && (
            <span className="text-purple-400 flex items-center gap-1">
              <Code size={11} />
              dense
            </span>
          )}
          <span className="flex items-center gap-1">
            <FileText size={11} />
            {display.length}c
          </span>
        </div>
      </button>
      {open && (
        <div className="p-3 border-t border-gray-800 bg-gray-950 max-h-72 overflow-y-auto">
          <div className="prose prose-invert prose-sm max-w-none break-words prose-pre:bg-gray-800 prose-pre:border prose-pre:border-gray-700 prose-pre:whitespace-pre-wrap prose-pre:break-words prose-a:text-blue-400">
            <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>
              {display}
            </ReactMarkdown>
          </div>
        </div>
      )}
    </div>
  );
}

/* ─── Message renderer ─── */
function ChatMessage({ msg, isLast }: { msg: ChatMsg; isLast: boolean }) {
  if (msg.type === "tool_start") {
    return (
      <div className="flex justify-start">
        <div className="flex items-center gap-2 text-amber-400 bg-amber-400/5 border border-amber-900/30 rounded-xl px-3 py-2 text-sm">
          <Loader2 size={14} className="animate-spin shrink-0" />
          <span className="font-mono">{msg.toolName || "tool"}</span>
          <span className="text-amber-400/60 text-xs">running…</span>
        </div>
      </div>
    );
  }

  if (msg.type === "tool_end" && msg.toolResult) {
    return (
      <div className="flex justify-start">
        <ToolResult name={msg.toolName || "tool"} result={msg.toolResult} />
      </div>
    );
  }

  if (msg.role === "system") {
    return (
      <div className="flex justify-center">
        <span className="text-[11px] text-gray-500 bg-gray-900/60 rounded-full px-3 py-1 border border-gray-800">
          {msg.content}
        </span>
      </div>
    );
  }

  if (msg.role === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-[75%] min-w-0 rounded-2xl rounded-br-sm px-4 py-3 bg-violet-600 text-white shadow-lg shadow-violet-900/30">
          <p className="whitespace-pre-wrap break-words leading-relaxed text-sm">{msg.content}</p>
        </div>
      </div>
    );
  }

  // Agent text
  return (
    <div className={`flex justify-start ${isLast ? "" : ""}`}>
      <div className="max-w-[78%] min-w-0 rounded-2xl rounded-bl-sm px-4 py-3 bg-gray-800/80 border border-gray-700/60 shadow-sm">
        <div className="prose prose-invert prose-sm max-w-none break-words prose-pre:bg-gray-900/70 prose-pre:border prose-pre:border-gray-700 prose-pre:whitespace-pre-wrap prose-pre:break-words prose-a:text-violet-400 hover:prose-a:text-violet-300 prose-code:text-violet-300">
          <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>
            {msg.content}
          </ReactMarkdown>
        </div>
      </div>
    </div>
  );
}

/* ─── Thinking indicator ─── */
function Thinking() {
  return (
    <div className="flex justify-start">
      <div className="bg-gray-800/80 border border-gray-700/60 rounded-2xl rounded-bl-sm px-4 py-3 flex gap-1.5 items-center">
        {[0, 150, 300].map((d) => (
          <div
            key={d}
            className="w-2 h-2 bg-violet-400 rounded-full animate-bounce"
            style={{ animationDelay: `${d}ms`, animationDuration: "1s" }}
          />
        ))}
      </div>
    </div>
  );
}

/* ─── Build prime prompt for the agent ─── */
const buildPrime = (sid: string): string =>
  `${PRIME_MARKER}
You are an AI assistant with live tool access running on a local engine.
Session id: ${sid}

Ground rules:
1. If asked about files, directories, or code — run a tool first (bash/glob/read) and report the actual result.
2. Never claim restrictions unless a tool returns an explicit denial.
3. If a tool call fails, share the exact error and suggest the next step.
4. For web questions, use websearch or webfetch before answering.
5. Format long answers with markdown headers and code blocks.
6. Prefer concise replies; use memory_store when the user asks you to remember something.
7. Do not expose raw tool-call payload JSON, hidden reasoning, or internal traces.
8. Summarize tool actions/results in plain language unless the user explicitly asks for diagnostics.
9. Default memory scope is global. For memory recall questions, call memory_search first with tier=global and allow_global=true before answering.
10. When user asks to remember something, store with memory_store tier=global and allow_global=true unless they explicitly request session/project scope.
11. Never say "no memory found" unless memory_search was executed in this turn and returned no matches.`;

/* ─── Main component ─── */
export default function ChatBrain() {
  const [messages, setMessages] = useState<ChatMsg[]>([]);
  const [input, setInput] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [sessionTitle, setSessionTitle] = useState("Chat");
  const [storedSessions, setStoredSessions] = useState<StoredSession[]>([]);
  const [pendingApprovals, setPendingApprovals] = useState<PendingApproval[]>([]);
  const [mcpAuthChallenges, setMcpAuthChallenges] = useState<McpAuthChallenge[]>([]);
  const [availableTools, setAvailableTools] = useState<string[]>([]);
  const [isThinking, setIsThinking] = useState(false);
  const [approving, setApproving] = useState(false);
  const [autoApproveAll, setAutoApproveAll] = useState<boolean>(() => {
    if (typeof window === "undefined") return false;
    return localStorage.getItem(AUTO_ALLOW_KEY) === "1";
  });
  const [showSidebar, setShowSidebar] = useState(false);
  const [log, setLog] = useState<string[]>([]);
  const [logOpen, setLogOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const esRef = useRef<EventSource | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const lastPromptRef = useRef<string | null>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, isThinking]);

  useEffect(() => {
    setStoredSessions(loadStoredSessions());
    void init();
    return () => {
      esRef.current?.close();
      esRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") return;
    localStorage.setItem(AUTO_ALLOW_KEY, autoApproveAll ? "1" : "0");
  }, [autoApproveAll]);

  const addLog = useCallback((msg: string) => {
    setLog((p) => [...p.slice(-60), msg]);
  }, []);

  const refreshToolIds = useCallback(async () => {
    try {
      const ids = await client.listToolIds();
      setAvailableTools(ids);
    } catch {
      /* ignore */
    }
  }, []);

  const refreshApprovals = useCallback(
    async (sid: string) => {
      try {
        const snapshot = await client.permissions.list();
        const reqs = (snapshot.requests || []) as PermissionRequestRecord[];
        let pending = reqs
          .filter(
            (r) =>
              (!r.sessionId || r.sessionId === sid) &&
              (r.status === "pending" || r.status === "asked" || r.status === "waiting")
          )
          .map((r) => ({
            id: r.id,
            tool: r.tool || r.permission || "tool",
            permission: typeof r.permission === "string" ? r.permission : undefined,
            pattern: typeof r.pattern === "string" ? r.pattern : undefined,
          }));

        if (autoApproveAll && pending.length > 0) {
          for (const req of pending) {
            try {
              await client.permissions.reply(req.id, "always");
              addLog(`Auto-approved ${req.tool}`);
            } catch {
              /* ignore */
            }
          }
          const fresh = await client.permissions.list();
          const freshReqs = (fresh.requests || []) as PermissionRequestRecord[];
          pending = freshReqs
            .filter(
              (r) =>
                (!r.sessionId || r.sessionId === sid) &&
                (r.status === "pending" || r.status === "asked" || r.status === "waiting")
            )
            .map((r) => ({
              id: r.id,
              tool: r.tool || r.permission || "tool",
              permission: typeof r.permission === "string" ? r.permission : undefined,
              pattern: typeof r.pattern === "string" ? r.pattern : undefined,
            }));
        }

        setPendingApprovals(pending);
        return pending;
      } catch {
        return [];
      }
    },
    [addLog, autoApproveAll]
  );

  useEffect(() => {
    if (!sessionId) return;
    const tick = () => {
      void refreshApprovals(sessionId);
    };
    tick();
    const id = window.setInterval(tick, 1500);
    return () => window.clearInterval(id);
  }, [refreshApprovals, sessionId, isThinking]);

  const buildChatFromHistory = (msgs: EngineMessage[]): ChatMsg[] =>
    msgs
      .filter((m) => m.info?.role === "user" || m.info?.role === "assistant")
      .filter(
        (m) =>
          !(m.parts || []).some(
            (p) => p.type === "text" && typeof p.text === "string" && p.text.includes(PRIME_MARKER)
          )
      )
      .flatMap((m) => {
        const role = m.info?.role === "assistant" ? "agent" : "user";
        const text = (m.parts || [])
          .filter((p) => p.type === "text" && p.text)
          .map((p) => p.text)
          .join("\n")
          .trim();
        const cleaned = sanitizeModelText(text).trim();
        if (!cleaned) return [];
        return [{ id: Math.random().toString(36), role, type: "text" as const, content: cleaned }];
      });

  const resolveModelSpec = useCallback(async (): Promise<ModelSpec | null> => {
    try {
      const [cfg, catalog] = await Promise.all([
        client.providers.config(),
        client.providers.catalog(),
      ]);
      const all = catalog.all || [];
      const connected = new Set((catalog.connected || []).filter(Boolean));

      const firstModelFor = (providerId: string): string | null => {
        const entry = all.find((p) => p.id === providerId);
        const ids = Object.keys(entry?.models || {});
        return ids.length > 0 ? ids[0] : null;
      };

      const defaultProvider = (cfg.default || catalog.default || "").trim();
      if (defaultProvider && connected.has(defaultProvider)) {
        const fromCfgEntry = (cfg.providers?.[defaultProvider] || {}) as ProviderCfg;
        const fromCfg = fromCfgEntry.defaultModel || fromCfgEntry.default_model || null;
        const fromCatalog = firstModelFor(defaultProvider);
        const model = (fromCfg || fromCatalog || "").trim();
        if (model) return { provider: defaultProvider, model };
      }

      for (const providerId of connected) {
        const fromCfgEntry = (cfg.providers?.[providerId] || {}) as ProviderCfg;
        const fromCfg = fromCfgEntry.defaultModel || fromCfgEntry.default_model || null;
        const fromCatalog = firstModelFor(providerId);
        const model = (fromCfg || fromCatalog || "").trim();
        if (model) return { provider: providerId, model };
      }
    } catch {
      // Ignore; caller will present user-facing guidance.
    }
    return null;
  }, []);

  const ensurePrimed = async (sid: string) => {
    const key = `${PRIMED_PREFIX}${sid}`;
    if (localStorage.getItem(key)) return;
    try {
      const msgs = await client.sessions.messages(sid);
      const already = msgs.some((m) =>
        (m.parts || []).some((p) => typeof p.text === "string" && p.text.includes(PRIME_MARKER))
      );
      if (!already)
        await client.sessions.promptAsync(sid, buildPrime(sid)).catch(() => {
          /* ignore */
        });
      localStorage.setItem(key, "1");
    } catch {
      /* best-effort */
    }
  };

  const attachStream = async (sid: string, rid?: string) => {
    esRef.current?.close();
    let watchdog: number | undefined;
    try {
      let receivedAnyEvent = false;
      let receivedAssistantDelta = false;
      let settled = false;
      let sawTerminalEvent = false;
      const ctrl = new AbortController();
      watchdog = window.setTimeout(async () => {
        if (receivedAnyEvent || settled) return;
        addLog("No run events received; checking backend state…");
        try {
          const run = await client.sessions.activeRun(sid);
          if (!run.active?.runId) {
            setIsThinking(false);
            setMessages((p) => [
              ...p,
              {
                id: Math.random().toString(36),
                role: "system",
                type: "text",
                content:
                  "No run activity was produced. Check Provider Setup and pick a connected provider/model.",
              },
            ]);
          }
        } catch {
          setIsThinking(false);
        }
      }, 12000);

      esRef.current = { close: () => ctrl.abort() } as EventSource;

      for await (const data of client.stream(sid, undefined, { signal: ctrl.signal })) {
        receivedAnyEvent = true;
        const type = data.type;
        const runId = eventRunId((data.properties || {}) as Record<string, unknown>);
        if (type === "session.response") {
          const delta = sanitizeModelText((data.properties?.delta as string) || "");
          if (delta) {
            receivedAssistantDelta = true;
            setMessages((prev) => {
              const upd = [...prev];
              const last = upd[upd.length - 1];
              if (
                last &&
                last.role === "agent" &&
                last.type === "text" &&
                last.id !== "welcome" &&
                last.id !== "err"
              ) {
                last.content += delta;
              } else {
                upd.push({
                  id: Math.random().toString(36),
                  role: "agent",
                  type: "text",
                  content: delta,
                });
              }
              return upd;
            });
          }
        } else if (type === "run.completed") {
          if (rid && (!runId || runId !== rid)) continue;
          settled = true;
          sawTerminalEvent = true;
          addLog("Run completed");
          if (!receivedAssistantDelta && sessionId) {
            try {
              const history = await client.sessions.messages(sessionId);
              const rebuilt = buildChatFromHistory(history);
              if (rebuilt.length > 0) setMessages(rebuilt);
            } catch {
              /* ignore */
            }
          }
          setIsThinking(false);
        } else if (type === "run.failed") {
          if (rid && (!runId || runId !== rid)) continue;
          settled = true;
          sawTerminalEvent = true;
          const detail =
            String(
              data.properties?.error || data.properties?.message || data.properties?.reason || ""
            ).trim() || "Run failed. Check provider/model configuration and credits.";
          addLog(`Run failed: ${detail}`);
          setMessages((p) => [
            ...p,
            {
              id: Math.random().toString(36),
              role: "system",
              type: "text",
              content: `Run failed: ${detail}`,
            },
          ]);
          setIsThinking(false);
        } else if (type === "session.run.finished") {
          if (rid && (!runId || runId !== rid)) continue;
          const status = String(data.properties?.status || "").toLowerCase();
          const failed = status === "failed" || status === "error";
          settled = true;
          sawTerminalEvent = true;
          if (failed) {
            const detail =
              String(
                data.properties?.error || data.properties?.message || data.properties?.reason || ""
              ).trim() || "Run failed.";
            addLog(`Run failed: ${detail}`);
            setMessages((p) => [
              ...p,
              {
                id: Math.random().toString(36),
                role: "system",
                type: "text",
                content: `Run failed: ${detail}`,
              },
            ]);
            setIsThinking(false);
          } else {
            addLog("Run completed");
            if (!receivedAssistantDelta && sessionId) {
              try {
                const history = await client.sessions.messages(sessionId);
                const rebuilt = buildChatFromHistory(history);
                if (rebuilt.length > 0) setMessages(rebuilt);
              } catch {
                /* ignore */
              }
            }
            setIsThinking(false);
          }
        } else if (type === "tool.called" || type === "tool_call.started") {
          const tool = (data.properties?.tool as string) || "tool";
          addLog(`▶ ${tool}`);
          setMessages((p) => [
            ...p,
            {
              id: Math.random().toString(36),
              role: "agent",
              type: "tool_start",
              content: "",
              toolName: tool,
            },
          ]);
        } else if (
          type === "tool.result" ||
          type === "tool_call.completed" ||
          type === "tool_call.failed"
        ) {
          const tool = (data.properties?.tool as string) || "tool";
          const result = String(data.properties?.result || data.properties?.error || "");
          addLog(`✓ ${tool}`);
          setMessages((p) => {
            const u = [...p];
            for (let i = u.length - 1; i >= 0; i--) {
              if (u[i].type === "tool_start" && u[i].toolName === tool) {
                u[i] = { ...u[i], type: "tool_end", content: "", toolResult: result };
                break;
              }
            }
            return u;
          });
        } else if (type === "approval.requested") {
          void refreshApprovals(sid);
        } else if (type === "mcp.auth.required" || type === "mcp.auth.pending") {
          const challengeId = String(data.properties?.challengeId || "").trim();
          const authorizationUrl = String(data.properties?.authorizationUrl || "").trim();
          if (!challengeId || !authorizationUrl) continue;
          const retryAfterMsRaw = Number(data.properties?.retryAfterMs ?? 0);
          const retryAfterMs =
            Number.isFinite(retryAfterMsRaw) && retryAfterMsRaw > 0 ? retryAfterMsRaw : undefined;
          const challenge: McpAuthChallenge = {
            challengeId,
            tool: String(data.properties?.tool || "mcp tool"),
            server:
              typeof data.properties?.server === "string"
                ? String(data.properties.server)
                : undefined,
            authorizationUrl,
            message:
              String(data.properties?.message || "").trim() ||
              "This MCP tool requires authorization before it can run.",
            pending: Boolean(data.properties?.pending),
            blocked: Boolean(data.properties?.blocked),
            retryAfterMs,
          };
          setMcpAuthChallenges((prev) => {
            const existingIndex = prev.findIndex(
              (item) => item.challengeId === challenge.challengeId
            );
            if (existingIndex >= 0) {
              const updated = [...prev];
              updated[existingIndex] = { ...updated[existingIndex], ...challenge };
              return updated;
            }
            return [challenge, ...prev].slice(0, 6);
          });
          setMessages((prev) => {
            if (
              prev.some(
                (m) => m.role === "system" && m.content.includes(challenge.challengeId.slice(0, 8))
              )
            ) {
              return prev;
            }
            const retryAfterSeconds = challenge.retryAfterMs
              ? Math.max(1, Math.ceil(challenge.retryAfterMs / 1000))
              : undefined;
            const prefix =
              challenge.pending && challenge.blocked
                ? "Authorization pending"
                : "Authorization required";
            const suffix =
              retryAfterSeconds && challenge.pending
                ? ` Retry after ~${retryAfterSeconds}s after completing authorization.`
                : "";
            return [
              ...prev,
              {
                id: Math.random().toString(36),
                role: "system",
                type: "text",
                content: `${prefix} for ${challenge.tool}. Complete authorization, then retry your last message.${suffix} (${challenge.challengeId.slice(0, 8)})`,
              },
            ];
          });
        }
      }
      if (!sawTerminalEvent) {
        addLog("Stream ended before terminal run event");
        try {
          const run = await client.sessions.activeRun(sid);
          if (!run.active?.runId) {
            setIsThinking(false);
            if (!receivedAssistantDelta) {
              setMessages((p) => [
                ...p,
                {
                  id: Math.random().toString(36),
                  role: "system",
                  type: "text",
                  content:
                    "Run stream ended unexpectedly. The engine may have dropped the run. Please retry your message.",
                },
              ]);
            }
          }
        } catch {
          setIsThinking(false);
          if (!receivedAssistantDelta) {
            setMessages((p) => [
              ...p,
              {
                id: Math.random().toString(36),
                role: "system",
                type: "text",
                content:
                  "Connection to the engine stream was interrupted. Please retry your message.",
              },
            ]);
          }
        }
      }
    } catch (e) {
      addLog(`Stream terminated`);
      try {
        const run = await client.sessions.activeRun(sid);
        if (!run.active?.runId) {
          setIsThinking(false);
          setMessages((p) => [
            ...p,
            {
              id: Math.random().toString(36),
              role: "system",
              type: "text",
              content: "The run stream disconnected before completion. Please retry your message.",
            },
          ]);
        }
      } catch {
        setIsThinking(false);
        setMessages((p) => [
          ...p,
          {
            id: Math.random().toString(36),
            role: "system",
            type: "text",
            content:
              "Could not reach the engine stream. Verify engine health and retry your message.",
          },
        ]);
      }
    } finally {
      if (watchdog !== undefined) window.clearTimeout(watchdog);
    }
  };

  const loadSession = async (sid: string) => {
    setSessionId(sid);
    localStorage.setItem(ACTIVE_KEY, sid);
    addLog(`Loading session ${sid.slice(0, 8)}`);
    setMcpAuthChallenges([]);
    void refreshToolIds();
    void refreshApprovals(sid);
    await ensurePrimed(sid);
    try {
      const history = await client.sessions.messages(sid);
      const msgs = buildChatFromHistory(history);
      setMessages(
        msgs.length > 0
          ? msgs
          : [
              {
                id: "welcome",
                role: "agent",
                type: "text",
                content:
                  "Hello! I'm your Tandem AI assistant. I have live access to tools — files, web search, memory and more. What would you like to explore?",
              },
            ]
      );
      // Resume active run if any
      const run = await client.sessions.activeRun(sid);
      const rid = String(run.active?.runId || "").trim();
      if (rid) {
        setIsThinking(true);
        void attachStream(sid, rid);
      }
    } catch (e) {
      setMessages([
        {
          id: "err",
          role: "system",
          type: "text",
          content: `Failed to load session: ${e instanceof Error ? e.message : String(e)}`,
        },
      ]);
    }
  };

  const init = async () => {
    try {
      await refreshToolIds();
      const saved = localStorage.getItem(ACTIVE_KEY);
      if (saved) {
        const sessions = loadStoredSessions();
        if (sessions.find((s) => s.id === saved)) {
          await loadSession(saved);
          return;
        }
      }
      await createNewSession();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const createNewSession = async (title?: string) => {
    const t =
      title || `Chat ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
    const spec = await resolveModelSpec();
    if (!spec) {
      throw new Error(
        "No default provider/model is configured. Open Provider Setup, connect a provider, choose a model, then retry."
      );
    }
    const sid = await client.sessions.create({
      title: t,
      provider: spec.provider,
      model: spec.model,
    });
    localStorage.setItem(ACTIVE_KEY, sid);
    const sessions = loadStoredSessions();
    const next = [{ id: sid, title: t, created: Date.now() }, ...sessions].slice(0, 20);
    saveStoredSessions(next);
    setStoredSessions(next);
    setSessionTitle(t);
    setSessionId(sid);
    setMessages([
      {
        id: "welcome",
        role: "agent",
        type: "text",
        content:
          "Hello! I'm your Tandem AI assistant. I have live access to tools — files, web search, memory and more. What would you like to explore?",
      },
    ]);
    setLog([]);
    setPendingApprovals([]);
    setMcpAuthChallenges([]);
    setIsThinking(false);
    await ensurePrimed(sid);
    void refreshToolIds();
    addLog(`New session ${sid.slice(0, 8)}`);
  };

  const sendPromptText = async (text: string) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    if (!sessionId) {
      const msg = "No active session. Please create or select a chat first.";
      setError(msg);
      addLog(msg);
      setMessages((p) => [
        ...p,
        { id: Math.random().toString(36), role: "system", type: "text", content: msg },
      ]);
      return;
    }
    try {
      const current = await client.sessions.activeRun(sessionId);
      const activeRunId = current.active?.runId?.trim();
      if (activeRunId) {
        addLog(`Cancelling previous active run ${activeRunId.slice(0, 8)} before send`);
        try {
          await client.sessions.cancelRun(sessionId, activeRunId);
        } catch {
          await client.sessions.cancel(sessionId);
        }
      }
    } catch {
      // Non-fatal: continue with a best-effort fresh run.
    }
    if (isThinking) {
      const msg =
        "The agent is still processing the previous run. Wait for it to finish, then send again.";
      setError(msg);
      addLog("Send blocked: run in progress");
      setMessages((p) => [
        ...p,
        { id: Math.random().toString(36), role: "system", type: "text", content: msg },
      ]);
      return;
    }
    lastPromptRef.current = trimmed;
    setInput("");
    setMessages((p) => [
      ...p,
      { id: Math.random().toString(36), role: "user", type: "text", content: trimmed },
    ]);
    setIsThinking(true);
    setError(null);
    addLog("Starting run…");
    try {
      const spec = await resolveModelSpec();
      if (!spec) {
        throw new Error(
          "No runnable provider/model is configured. Open Provider Setup and select one."
        );
      }
      const { runId } = await promptAsyncWithModel(sessionId, trimmed, {
        provider: spec.provider,
        model: spec.model,
      });
      addLog(`Run ${runId.slice(0, 8)}`);
      void attachStream(sessionId, runId);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setMessages((p) => [
        ...p,
        { id: Math.random().toString(36), role: "system", type: "text", content: `Error: ${msg}` },
      ]);
      setIsThinking(false);
      setError(msg);
    }
  };

  const handleSend = async (e?: React.FormEvent) => {
    e?.preventDefault();
    await sendPromptText(input);
  };

  const approveAll = async () => {
    if (!sessionId || pendingApprovals.length === 0 || approving) return;
    setApproving(true);
    for (const req of pendingApprovals) {
      try {
        await client.permissions.reply(req.id, "always");
        addLog(`Approved ${req.tool}`);
      } catch {
        /* ignore */
      }
    }
    await refreshApprovals(sessionId);
    setApproving(false);
  };

  const retryLastPrompt = () => {
    if (!lastPromptRef.current || isThinking) return;
    void sendPromptText(lastPromptRef.current);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  };

  const switchSession = async (sid: string) => {
    setShowSidebar(false);
    esRef.current?.close();
    setIsThinking(false);
    setMessages([]);
    setLog([]);
    const stored = loadStoredSessions().find((s) => s.id === sid);
    if (stored) setSessionTitle(stored.title);
    await loadSession(sid);
  };

  const deleteStoredSession = async (sid: string) => {
    const next = loadStoredSessions().filter((s) => s.id !== sid);
    saveStoredSessions(next);
    setStoredSessions(next);
    if (sid === sessionId) {
      await createNewSession();
    }
  };

  return (
    <div className="flex h-full bg-gray-950">
      {/* Session sidebar */}
      {showSidebar && (
        <div className="fixed inset-0 z-40 flex xl:relative xl:inset-auto xl:flex xl:w-64">
          <button
            className="absolute inset-0 bg-black/50 xl:hidden"
            onClick={() => setShowSidebar(false)}
          />
          <div className="relative z-10 w-64 bg-gray-900 border-r border-gray-800 flex flex-col h-full">
            <div className="flex items-center justify-between px-4 py-3 border-b border-gray-800">
              <span className="text-sm font-medium text-gray-200 flex items-center gap-2">
                <History size={14} />
                Sessions
              </span>
              <button
                onClick={() => setShowSidebar(false)}
                className="text-gray-400 hover:text-white xl:hidden"
              >
                <X size={16} />
              </button>
            </div>
            <div className="flex-1 overflow-y-auto p-2 space-y-1">
              {storedSessions.map((s) => (
                <div
                  key={s.id}
                  className={`group flex items-center gap-2 rounded-lg px-3 py-2 cursor-pointer transition-colors ${s.id === sessionId ? "bg-violet-600/20 border border-violet-600/40" : "hover:bg-gray-800"}`}
                  onClick={() => void switchSession(s.id)}
                >
                  <span className="flex-1 text-sm text-gray-200 truncate">{s.title}</span>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void deleteStoredSession(s.id);
                    }}
                    className="opacity-0 group-hover:opacity-100 text-gray-500 hover:text-rose-400 transition-opacity"
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              ))}
              {storedSessions.length === 0 && (
                <p className="text-xs text-gray-600 px-3 py-6 text-center">No sessions yet.</p>
              )}
            </div>
            <div className="p-3 border-t border-gray-800">
              <button
                onClick={() => {
                  setShowSidebar(false);
                  void createNewSession();
                }}
                className="w-full flex items-center justify-center gap-2 rounded-lg bg-violet-600 hover:bg-violet-500 text-white text-sm py-2 transition-colors"
              >
                <Plus size={14} />
                New session
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Main chat area */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <div className="bg-gray-900/80 backdrop-blur border-b border-gray-800 px-4 py-3 shrink-0">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-3 min-w-0">
              <button
                onClick={() => setShowSidebar((s) => !s)}
                className="shrink-0 p-1.5 rounded-lg hover:bg-gray-800 text-gray-400 hover:text-white transition-colors"
                title="Session history"
              >
                <History size={18} />
              </button>
              <div className="flex items-center gap-2 min-w-0">
                <div className="w-2 h-2 rounded-full bg-violet-400 shrink-0 animate-pulse" />
                <h2 className="text-sm font-semibold text-gray-100 truncate">{sessionTitle}</h2>
              </div>
              {availableTools.length > 0 && (
                <span className="hidden sm:flex items-center gap-1 text-[11px] text-gray-500 shrink-0">
                  <Zap size={11} className="text-violet-500" />
                  {availableTools.length} tools
                </span>
              )}
            </div>
            <div className="flex items-center gap-2 shrink-0">
              {pendingApprovals.length > 0 && (
                <button
                  onClick={() => void approveAll()}
                  disabled={approving}
                  className="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-300 hover:bg-amber-500/20 transition-colors disabled:opacity-50"
                >
                  <AlertCircle size={12} />
                  {approving ? "Approving…" : `Approve ${pendingApprovals.length}`}
                </button>
              )}
              <button
                onClick={() => setAutoApproveAll((v) => !v)}
                className={`text-xs px-3 py-1.5 rounded-lg border transition-colors ${
                  autoApproveAll
                    ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-300"
                    : "border-gray-700 text-gray-400 hover:bg-gray-800 hover:text-gray-200"
                }`}
                title="Automatically approve all permission requests in this session"
              >
                {autoApproveAll ? "Auto-allow on" : "Auto-allow all"}
              </button>
              {SHOW_DEBUG_UI && (
                <button
                  onClick={() => setLogOpen((o) => !o)}
                  className="p-1.5 rounded-lg hover:bg-gray-800 text-gray-500 hover:text-gray-300 transition-colors"
                  title="Toggle debug log"
                >
                  <Settings2 size={16} />
                </button>
              )}
              <button
                onClick={() => void createNewSession()}
                className="p-1.5 rounded-lg hover:bg-gray-800 text-gray-500 hover:text-gray-300 transition-colors"
                title="New session"
              >
                <Plus size={16} />
              </button>
            </div>
          </div>
          {SHOW_DEBUG_UI && logOpen && log.length > 0 && (
            <div className="mt-2 rounded-lg bg-gray-950 border border-gray-800 px-3 py-2 max-h-20 overflow-y-auto">
              {log.slice(-8).map((entry, i) => (
                <p key={i} className="text-[10px] text-gray-500 font-mono leading-5">
                  {entry}
                </p>
              ))}
            </div>
          )}
          {pendingApprovals.length > 0 && (
            <div className="mt-2 rounded-lg bg-amber-900/10 border border-amber-700/30 px-3 py-2 space-y-2">
              <p className="text-xs text-amber-300">
                Pending approvals ({pendingApprovals.length})
              </p>
              <div className="space-y-1.5 max-h-28 overflow-y-auto">
                {pendingApprovals.map((req) => (
                  <div
                    key={req.id}
                    className="text-[11px] text-amber-100/90 bg-amber-950/20 border border-amber-800/30 rounded px-2 py-1.5"
                  >
                    <span className="font-mono">{req.tool}</span>
                    {req.pattern ? ` · ${req.pattern}` : ""}
                    {req.permission && req.permission !== req.tool
                      ? ` · permission=${req.permission}`
                      : ""}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Messages */}
        <div className="flex-1 overflow-y-auto px-4 py-6">
          <div className="w-full max-w-5xl mx-auto space-y-4 overflow-x-hidden">
            {error && (
              <div className="bg-rose-900/20 border border-rose-800/40 rounded-xl p-3 flex items-start gap-2">
                <AlertCircle size={14} className="text-rose-400 mt-0.5 shrink-0" />
                <p className="text-sm text-rose-300">{error}</p>
              </div>
            )}
            {mcpAuthChallenges.length > 0 && (
              <div className="space-y-2">
                {mcpAuthChallenges.map((challenge) => (
                  <div
                    key={challenge.challengeId}
                    className="rounded-xl border border-cyan-700/40 bg-cyan-900/10 px-3 py-3"
                  >
                    <p className="text-sm text-cyan-100 font-medium">
                      MCP authorization needed for{" "}
                      <span className="font-mono">{challenge.tool}</span>
                    </p>
                    <p className="text-xs text-cyan-200/90 mt-1">{challenge.message}</p>
                    {challenge.pending && challenge.retryAfterMs ? (
                      <p className="text-xs text-cyan-200/70 mt-1">
                        Authorization is pending. Retry after about{" "}
                        {Math.max(1, Math.ceil(challenge.retryAfterMs / 1000))}s.
                      </p>
                    ) : null}
                    <a
                      href={challenge.authorizationUrl}
                      target="_blank"
                      rel="noreferrer"
                      className="mt-2 inline-block text-xs text-cyan-300 underline break-all"
                    >
                      {challenge.authorizationUrl}
                    </a>
                    <div className="mt-2 flex gap-2">
                      <button
                        onClick={retryLastPrompt}
                        disabled={isThinking}
                        className="text-xs px-2.5 py-1.5 rounded-lg bg-cyan-600 hover:bg-cyan-500 disabled:opacity-50 text-white"
                      >
                        Retry last message
                      </button>
                      <button
                        onClick={() =>
                          setMcpAuthChallenges((prev) =>
                            prev.filter((item) => item.challengeId !== challenge.challengeId)
                          )
                        }
                        className="text-xs px-2.5 py-1.5 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-300"
                      >
                        Dismiss
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
            {messages.map((m, i) => (
              <ChatMessage key={m.id} msg={m} isLast={i === messages.length - 1} />
            ))}
            {isThinking && <Thinking />}
            <div ref={bottomRef} />
          </div>
        </div>

        {/* Input */}
        <div className="shrink-0 px-4 pb-4 pt-2 bg-gray-950 border-t border-gray-800/60">
          <div className="w-full max-w-5xl mx-auto">
            <form onSubmit={(e) => void handleSend(e)} className="relative">
              <textarea
                ref={inputRef}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                disabled={isThinking || !sessionId}
                rows={1}
                placeholder={
                  isThinking
                    ? "Agent is thinking…"
                    : "Ask anything — files, web, memory… (⏎ send, ⇧⏎ newline)"
                }
                className="w-full bg-gray-800/80 border border-gray-700/60 rounded-2xl pl-4 pr-12 py-3 text-sm text-gray-100 placeholder:text-gray-500 resize-none overflow-hidden focus:outline-none focus:ring-2 focus:ring-violet-500/50 focus:border-violet-600/50 disabled:opacity-50 transition-all leading-relaxed"
                style={{ minHeight: 48, maxHeight: 180 }}
                onInput={(e) => {
                  const t = e.currentTarget;
                  t.style.height = "auto";
                  t.style.height = Math.min(t.scrollHeight, 180) + "px";
                }}
              />
              <button
                type="submit"
                disabled={!input.trim() || isThinking || !sessionId}
                className="absolute right-3 top-1/2 -translate-y-1/2 p-2 rounded-xl bg-violet-600 hover:bg-violet-500 disabled:bg-gray-700 disabled:text-gray-500 text-white transition-colors"
              >
                <Send size={16} />
              </button>
            </form>
            <div className="mt-1.5 flex items-center justify-between px-1">
              <p className="text-[11px] text-gray-600">
                Session <span className="font-mono">{sessionId?.slice(0, 8) ?? "—"}</span>
                {availableTools.length > 0 && ` · ${availableTools.length} tools`}
              </p>
              {pendingApprovals.length > 0 && (
                <p className="text-[11px] text-amber-400">
                  {pendingApprovals.length} approval{pendingApprovals.length > 1 ? "s" : ""} needed
                </p>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Right panel: session info on desktop */}
      <div className="hidden 2xl:flex w-52 bg-gray-900/50 border-l border-gray-800 flex-col py-4 px-3 gap-4 shrink-0">
        <div>
          <p className="text-[10px] uppercase tracking-widest text-gray-600 mb-2">
            Tools Available
          </p>
          <div className="flex flex-wrap gap-1">
            {availableTools.slice(0, 12).map((t) => (
              <span
                key={t}
                className="text-[10px] font-mono bg-gray-800 text-gray-400 rounded px-1.5 py-0.5"
              >
                {t}
              </span>
            ))}
            {availableTools.length > 12 && (
              <span className="text-[10px] text-gray-600">+{availableTools.length - 12} more</span>
            )}
          </div>
        </div>
        <div>
          <p className="text-[10px] uppercase tracking-widest text-gray-600 mb-2">
            Recent Sessions
          </p>
          <div className="space-y-1">
            {storedSessions.slice(0, 6).map((s) => (
              <button
                key={s.id}
                onClick={() => void switchSession(s.id)}
                className={`w-full text-left text-xs px-2 py-1.5 rounded-lg truncate transition-colors ${s.id === sessionId ? "bg-violet-600/20 text-violet-300" : "text-gray-400 hover:bg-gray-800 hover:text-gray-200"}`}
              >
                {s.title}
              </button>
            ))}
          </div>
        </div>
        {SHOW_DEBUG_UI && (
          <div className="mt-auto">
            <p className="text-[10px] text-gray-600 mb-1 uppercase tracking-widest">Debug Log</p>
            <div className="max-h-32 overflow-y-auto space-y-0.5">
              {log.slice(-10).map((e, i) => (
                <p key={i} className="text-[10px] text-gray-600 font-mono leading-4 break-all">
                  {e}
                </p>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
