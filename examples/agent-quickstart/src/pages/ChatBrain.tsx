import { useState, useEffect, useRef, useCallback } from "react";
import { client } from "../api";
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
}
interface StoredSession {
  id: string;
  title: string;
  created: number;
}

const SESSIONS_KEY = "tandem_aq_sessions";
const ACTIVE_KEY = "tandem_aq_active_session";
const PRIMED_PREFIX = "tandem_aq_primed_";
const PRIME_MARKER = "[AQ_PRIMED_V1]";

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
    <div className="border border-gray-800 rounded-xl overflow-hidden bg-gray-900/60 text-sm max-w-[85%]">
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
          <div className="prose prose-invert prose-sm max-w-none prose-pre:bg-gray-800 prose-pre:border prose-pre:border-gray-700 prose-a:text-blue-400">
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
        <div className="max-w-[75%] rounded-2xl rounded-br-sm px-4 py-3 bg-violet-600 text-white shadow-lg shadow-violet-900/30">
          <p className="whitespace-pre-wrap leading-relaxed text-sm">{msg.content}</p>
        </div>
      </div>
    );
  }

  // Agent text
  return (
    <div className={`flex justify-start ${isLast ? "" : ""}`}>
      <div className="max-w-[78%] rounded-2xl rounded-bl-sm px-4 py-3 bg-gray-800/80 border border-gray-700/60 shadow-sm">
        <div className="prose prose-invert prose-sm max-w-none prose-pre:bg-gray-900/70 prose-pre:border prose-pre:border-gray-700 prose-a:text-violet-400 hover:prose-a:text-violet-300 prose-code:text-violet-300">
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
6. Prefer concise replies; use memory_store when the user asks you to remember something.`;

/* ─── Main component ─── */
export default function ChatBrain() {
  const [messages, setMessages] = useState<ChatMsg[]>([]);
  const [input, setInput] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [sessionTitle, setSessionTitle] = useState("Chat");
  const [storedSessions, setStoredSessions] = useState<StoredSession[]>([]);
  const [pendingApprovals, setPendingApprovals] = useState<PendingApproval[]>([]);
  const [availableTools, setAvailableTools] = useState<string[]>([]);
  const [isThinking, setIsThinking] = useState(false);
  const [approving, setApproving] = useState(false);
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

  const addLog = useCallback((msg: string) => {
    setLog((p) => [...p.slice(-60), msg]);
  }, []);

  const refreshApprovals = useCallback(async (sid: string) => {
    try {
      const snapshot = await client.permissions.list();
      const reqs = (snapshot.requests || []) as PermissionRequestRecord[];
      const pending = reqs
        .filter(
          (r) =>
            (!r.sessionId || r.sessionId === sid) &&
            (r.status === "pending" || r.status === "asked" || r.status === "waiting")
        )
        .map((r) => ({ id: r.id, tool: r.tool || r.permission || "tool" }));
      setPendingApprovals(pending);
      return pending;
    } catch {
      return [];
    }
  }, []);

  const buildChatFromHistory = (msgs: EngineMessage[]): ChatMsg[] =>
    msgs
      .filter((m) => m.info?.role === "user" || m.info?.role === "assistant")
      .flatMap((m) => {
        const role = m.info?.role === "assistant" ? "agent" : "user";
        const text = (m.parts || [])
          .filter((p) => p.type === "text" && p.text)
          .map((p) => p.text)
          .join("\n")
          .trim();
        if (!text) return [];
        return [{ id: Math.random().toString(36), role, type: "text" as const, content: text }];
      });

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

  const attachStream = async (sid: string, rid: string) => {
    try {
      const ctrl = new AbortController();
      esRef.current = { close: () => ctrl.abort() } as any;

      for await (const data of client.stream(sid, rid, { signal: ctrl.signal })) {
        const type = data.type;
        if (type === "session.response") {
          const delta = data.properties?.delta as string;
          if (delta) {
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
          addLog("Run completed");
          if (sessionId) {
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
        }
      }
    } catch (e) {
      addLog(`Stream terminated`);
      setIsThinking(false);
    }
  };

  const loadSession = async (sid: string) => {
    setSessionId(sid);
    localStorage.setItem(ACTIVE_KEY, sid);
    addLog(`Loading session ${sid.slice(0, 8)}`);
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
      client
        .listToolIds()
        .then((ids) => setAvailableTools(ids))
        .catch(() => {
          /* ignore */
        });
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
    const sid = await client.sessions.create({ title: t });
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
    setIsThinking(false);
    await ensurePrimed(sid);
    addLog(`New session ${sid.slice(0, 8)}`);
  };

  const handleSend = async (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!input.trim() || isThinking || !sessionId) return;
    const text = input.trim();
    lastPromptRef.current = text;
    setInput("");
    setMessages((p) => [
      ...p,
      { id: Math.random().toString(36), role: "user", type: "text", content: text },
    ]);
    setIsThinking(true);
    setError(null);
    addLog("Starting run…");
    try {
      const { runId } = await client.sessions.promptAsync(sessionId, text);
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
                onClick={() => setLogOpen((o) => !o)}
                className="p-1.5 rounded-lg hover:bg-gray-800 text-gray-500 hover:text-gray-300 transition-colors"
                title="Toggle debug log"
              >
                <Settings2 size={16} />
              </button>
              <button
                onClick={() => void createNewSession()}
                className="p-1.5 rounded-lg hover:bg-gray-800 text-gray-500 hover:text-gray-300 transition-colors"
                title="New session"
              >
                <Plus size={16} />
              </button>
            </div>
          </div>
          {logOpen && log.length > 0 && (
            <div className="mt-2 rounded-lg bg-gray-950 border border-gray-800 px-3 py-2 max-h-20 overflow-y-auto">
              {log.slice(-8).map((entry, i) => (
                <p key={i} className="text-[10px] text-gray-500 font-mono leading-5">
                  {entry}
                </p>
              ))}
            </div>
          )}
        </div>

        {/* Messages */}
        <div className="flex-1 overflow-y-auto px-4 py-6 space-y-4">
          {error && (
            <div className="bg-rose-900/20 border border-rose-800/40 rounded-xl p-3 flex items-start gap-2">
              <AlertCircle size={14} className="text-rose-400 mt-0.5 shrink-0" />
              <p className="text-sm text-rose-300">{error}</p>
            </div>
          )}
          {messages.map((m, i) => (
            <ChatMessage key={m.id} msg={m} isLast={i === messages.length - 1} />
          ))}
          {isThinking && <Thinking />}
          <div ref={bottomRef} />
        </div>

        {/* Input */}
        <div className="shrink-0 px-4 pb-4 pt-2 bg-gray-950 border-t border-gray-800/60">
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
      </div>
    </div>
  );
}
