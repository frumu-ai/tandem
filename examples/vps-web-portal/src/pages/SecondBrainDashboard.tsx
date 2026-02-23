import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { BrainCircuit, Send, FileCode2, Database, FolderGit2, Loader2 } from "lucide-react";
import { SessionHistory } from "../components/SessionHistory";
import { ToolCallResult } from "../components/ToolCallResult";
import { handleCommonRunEvent } from "../utils/liveEventDebug";

interface ChatEvent {
  id: string;
  role: "user" | "agent" | "system";
  type: "text" | "tool_start" | "tool_end";
  content: string;
  toolName?: string;
  toolResult?: string;
}

interface RuntimeTraceEntry {
  id: string;
  timestamp: Date;
  content: string;
}

interface PendingApproval {
  id: string;
  tool: string;
}

const SECOND_BRAIN_SESSION_KEY = "tandem_portal_second_brain_session_id";
const RUN_TIMEOUT_MS = 45000;

const buildChatEvents = (
  messages: Awaited<ReturnType<typeof api.getSessionMessages>>
): ChatEvent[] => {
  return messages
    .filter((m) => m.info?.role === "user" || m.info?.role === "assistant")
    .flatMap((m) => {
      const events: ChatEvent[] = [];
      const role = m.info?.role === "assistant" ? "agent" : "user";

      const text = (m.parts || [])
        .filter((p) => p.type === "text" && p.text)
        .map((p) => p.text)
        .join("\n")
        .trim();

      if (text) {
        events.push({
          id: Math.random().toString(),
          role,
          type: "text",
          content: text,
        });
      }
      return events;
    });
};

export const SecondBrainDashboard: React.FC = () => {
  const [messages, setMessages] = useState<ChatEvent[]>([]);
  const [runtimeTrace, setRuntimeTrace] = useState<RuntimeTraceEntry[]>([]);
  const [pendingApprovals, setPendingApprovals] = useState<PendingApproval[]>([]);
  const [input, setInput] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [isThinking, setIsThinking] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, isThinking]);

  const addTrace = (content: string) => {
    setRuntimeTrace((prev) => {
      const next = [
        ...prev,
        { id: Math.random().toString(36).substring(2), timestamp: new Date(), content },
      ];
      return next.slice(-120);
    });
  };

  const refreshPendingApprovals = async (sid: string): Promise<PendingApproval[]> => {
    try {
      const snapshot = await api.listPermissions();
      const pending = (snapshot.requests || [])
        .filter((req) => req.sessionID === sid && req.status === "pending")
        .map((req) => ({
          id: req.id,
          tool: req.tool || req.permission || "tool",
        }));
      setPendingApprovals(pending);
      return pending;
    } catch {
      return [];
    }
  };

  const approvePendingForSession = async () => {
    if (!sessionId || pendingApprovals.length === 0) return;
    addTrace(`Approving ${pendingApprovals.length} pending permission request(s).`);
    for (const req of pendingApprovals) {
      try {
        await api.replyPermission(req.id, "allow");
      } catch {
        addTrace(`Failed to approve permission request ${req.id.substring(0, 8)}.`);
      }
    }
    const refreshed = await refreshPendingApprovals(sessionId);
    if (refreshed.length === 0) {
      addTrace("All pending permission requests approved.");
    }
  };

  const attachRunStream = (sid: string, runId: string) => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }

    addTrace(`Attaching run stream: ${runId.substring(0, 8)}`);
    const source = new EventSource(api.getEventStreamUrl(sid, runId));
    eventSourceRef.current = source;
    let finalized = false;
    let timeoutHandle: ReturnType<typeof setTimeout> | null = null;
    let watchdogHandle: ReturnType<typeof setTimeout> | null = null;
    let runStatePollHandle: ReturnType<typeof setInterval> | null = null;
    let sawRunEvent = false;

    const finalize = async (reason: string) => {
      if (finalized) return;
      finalized = true;
      addTrace(`Finalizing run: ${reason}`);
      if (timeoutHandle) {
        clearTimeout(timeoutHandle);
        timeoutHandle = null;
      }
      if (watchdogHandle) {
        clearTimeout(watchdogHandle);
        watchdogHandle = null;
      }
      if (runStatePollHandle) {
        clearInterval(runStatePollHandle);
        runStatePollHandle = null;
      }
      try {
        const history = await api.getSessionMessages(sid);
        const restored = buildChatEvents(history);
        if (restored.length > 0) {
          setMessages(restored);
        }
        if (reason === "timeout") {
          const pending = await refreshPendingApprovals(sid);
          if (pending.length > 0) {
            const tools = [...new Set(pending.map((p) => p.tool))].join(", ");
            setMessages((prev) => [
              ...prev,
              {
                id: Math.random().toString(),
                role: "system",
                type: "text",
                content: `[Run is waiting for permission approval: ${tools}. Use "Approve Pending" to continue.]`,
              },
            ]);
            addTrace(`Timeout caused by pending permission approvals: ${tools}.`);
          } else {
            setMessages((prev) => [
              ...prev,
              {
                id: Math.random().toString(),
                role: "agent",
                type: "text",
                content:
                  "[Run timed out in UI. Loaded latest saved session history so you can continue.]",
              },
            ]);
          }
        } else if (reason === "stream_error") {
          setMessages((prev) => [
            ...prev,
            {
              id: Math.random().toString(),
              role: "agent",
              type: "text",
              content: "[Stream disconnected. Loaded latest saved session history.]",
            },
          ]);
        } else if (reason === "inactive_no_events") {
          setMessages((prev) => [
            ...prev,
            {
              id: Math.random().toString(),
              role: "agent",
              type: "text",
              content:
                "[Run ended before live deltas arrived. Check provider key/model and engine logs.]",
            },
          ]);
        } else if (reason === "inactive") {
          setMessages((prev) => [
            ...prev,
            {
              id: Math.random().toString(),
              role: "system",
              type: "text",
              content:
                "[Run became inactive with no terminal stream event. Synced latest history.]",
            },
          ]);
        }
      } catch (err) {
        console.error("Failed to load session history after run", err);
        addTrace("Failed to reload session history after finalize.");
      } finally {
        setIsThinking(false);
        source.close();
        if (eventSourceRef.current === source) {
          eventSourceRef.current = null;
        }
      }
    };

    timeoutHandle = setTimeout(() => {
      addTrace(`Run timeout reached (${RUN_TIMEOUT_MS}ms).`);
      void finalize("timeout");
    }, RUN_TIMEOUT_MS);
    watchdogHandle = setTimeout(async () => {
      if (finalized || sawRunEvent) return;
      try {
        const runState = await api.getActiveRun(sid);
        if (!runState?.active) {
          addTrace("No events and run is already inactive.");
          void finalize("inactive_no_events");
          return;
        }
        const pending = await refreshPendingApprovals(sid);
        if (pending.length > 0) {
          const tools = [...new Set(pending.map((p) => p.tool))].join(", ");
          addTrace(`Run is blocked on permission approval: ${tools}.`);
          setMessages((prev) => [
            ...prev,
            {
              id: Math.random().toString(),
              role: "system",
              type: "text",
              content: `Run is waiting for permission approval: ${tools}.`,
            },
          ]);
          return;
        }
        addTrace("Run active but no live deltas yet.");
        setMessages((prev) => [
          ...prev,
          {
            id: Math.random().toString(),
            role: "system",
            type: "text",
            content: "Run is active but no live deltas yet. Waiting for provider output...",
          },
        ]);
      } catch {
        addTrace("No events yet and run-state check failed.");
        setMessages((prev) => [
          ...prev,
          {
            id: Math.random().toString(),
            role: "system",
            type: "text",
            content: "No live events yet and failed to query run state.",
          },
        ]);
      }
    }, 4000);
    runStatePollHandle = setInterval(async () => {
      if (finalized) return;
      try {
        const runState = await api.getActiveRun(sid);
        if (!runState?.active) {
          addTrace("Run became inactive from periodic poll.");
          void finalize("inactive");
        }
      } catch {
        // keep stream attached
      }
    }, 5000);

    source.onmessage = (evt) => {
      try {
        const data = JSON.parse(evt.data);
        if (data.type !== "server.connected" && data.type !== "engine.lifecycle.ready") {
          sawRunEvent = true;
        }

        if (
          handleCommonRunEvent(
            data,
            (event) => addTrace(event.content),
            (status) => finalize(status)
          )
        ) {
          return;
        }

        if (data.type === "message.part.updated") {
          const part = data.properties?.part;
          if (!part) return;

          if (part.type === "tool") {
            if (part.state?.status === "running") {
              addTrace(`Tool started: ${part.tool}`);
              setMessages((prev) => [
                ...prev,
                {
                  id: Math.random().toString(),
                  role: "agent",
                  type: "tool_start",
                  content: `Using tool: ${part.tool}`,
                  toolName: part.tool,
                },
              ]);
            } else if (part.state?.status === "completed") {
              let rString = "";
              if (part.state.result) {
                rString =
                  typeof part.state.result === "string"
                    ? part.state.result
                    : JSON.stringify(part.state.result);
              }
              addTrace(`Tool completed: ${part.tool}`);

              setMessages((prev) => {
                const updated = [...prev];
                let lastStartIdx = -1;
                for (let i = prev.length - 1; i >= 0; i--) {
                  if (prev[i].type === "tool_start" && prev[i].toolName === part.tool) {
                    lastStartIdx = i;
                    break;
                  }
                }

                if (lastStartIdx !== -1) {
                  updated[lastStartIdx] = {
                    ...updated[lastStartIdx],
                    type: "tool_end",
                    content: `Tool completed: ${part.tool}`,
                    toolResult: rString,
                  };
                  return updated;
                }
                return prev;
              });
            }
          } else if (part.type === "text" && data.properties?.delta) {
            setMessages((prev) => {
              const updated = [...prev];
              const last = updated[updated.length - 1];
              if (
                last &&
                last.role === "agent" &&
                last.type === "text" &&
                last.id !== "welcome" &&
                last.id !== "err"
              ) {
                last.content += data.properties.delta;
              } else {
                updated.push({
                  id: Math.random().toString(),
                  role: "agent",
                  type: "text",
                  content: data.properties.delta,
                });
              }
              return updated;
            });
          }
        }
      } catch (err) {
        console.error("Second Brain stream parse error", err);
        addTrace("Failed to parse stream event payload.");
      }
    };
    source.onerror = () => {
      addTrace("Stream disconnected.");
      void finalize("stream_error");
    };
  };

  const loadSession = async (sid: string) => {
    if (!sid) {
      setMessages([]);
      setSessionId(null);
      setPendingApprovals([]);
      localStorage.removeItem(SECOND_BRAIN_SESSION_KEY);
      addTrace("Session cleared.");
      return;
    }

    try {
      setSessionId(sid);
      localStorage.setItem(SECOND_BRAIN_SESSION_KEY, sid);
      addTrace(`Loading session ${sid.substring(0, 8)}.`);
      void refreshPendingApprovals(sid);
      const history = await api.getSessionMessages(sid);
      const restored = buildChatEvents(history);

      setMessages([
        {
          id: "sys-restored",
          role: "system",
          type: "text",
          content: `Restored session ${sid.substring(0, 8)}`,
        },
        ...restored,
      ]);

      const runState = await api.getActiveRun(sid);
      const active = runState?.active || null;
      const activeRunId =
        (active?.runID as string | undefined) ||
        (active?.runId as string | undefined) ||
        (active?.run_id as string | undefined) ||
        "";
      if (activeRunId) {
        setIsThinking(true);
        addTrace(`Resuming active run ${activeRunId.substring(0, 8)}.`);
        setMessages((prev) => [
          ...prev,
          {
            id: Math.random().toString(),
            role: "system",
            type: "text",
            content: `Resuming active run ${activeRunId.substring(0, 8)}...`,
          },
        ]);
        attachRunStream(sid, activeRunId);
      } else {
        setIsThinking(false);
      }
    } catch (err) {
      console.error("Failed to restore session", err);
      addTrace("Failed to restore saved session.");
      setPendingApprovals([]);
      setSessionId(null);
      localStorage.removeItem(SECOND_BRAIN_SESSION_KEY);
    }
  };

  useEffect(() => {
    const initBrain = async () => {
      try {
        const existingSessionId = localStorage.getItem(SECOND_BRAIN_SESSION_KEY);
        if (existingSessionId) {
          addTrace(`Found saved session ${existingSessionId.substring(0, 8)}.`);
          await loadSession(existingSessionId);
          return;
        }

        const sid = await api.createSession("Second Brain MVP");
        localStorage.setItem(SECOND_BRAIN_SESSION_KEY, sid);
        setSessionId(sid);
        // System prompt sets up the MCP expectation for new sessions only.
        const prompt = `You are a Second Brain AI assistant. You have access to the local server's workspace via MCP tools and memory_store capabilities. When users ask you to learn a folder, use the memory_store tool to index local files, and output a summary into 'out/index_stats.json'. When answering questions, write your detailed answer to 'out/answers.md' alongside citing the file paths in your chat response.`;
        await api.sendMessage(sid, prompt);
        addTrace(`Created new session ${sid.substring(0, 8)} and primed instructions.`);
        setMessages([
          {
            id: "welcome",
            role: "agent",
            type: "text",
            content:
              "Hello! I am connected to the local headless engine. I can use MCP tools to browse files, run commands, or interact with databases. What would you like to explore?",
          },
        ]);
      } catch (err) {
        const errorMessage = err instanceof Error ? err.message : String(err);
        addTrace(`Engine connection failed: ${errorMessage}`);
        setMessages([
          {
            id: "err",
            role: "system",
            type: "text",
            content: `CRITICAL ERROR: Failed to connect to engine. ${errorMessage}`,
          },
        ]);
      }
    };
    initBrain();
    return () => {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
        eventSourceRef.current = null;
      }
    };
  }, []);

  const handleSend = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim() || isThinking || !sessionId) return;

    const userMsg = input.trim();
    setInput("");
    setMessages((prev) => [
      ...prev,
      { id: Math.random().toString(), role: "user", type: "text", content: userMsg },
    ]);
    setIsThinking(true);
    addTrace("Submitting prompt to async run.");

    try {
      const { runId } = await api.startAsyncRun(sessionId, userMsg);
      addTrace(`Run started ${runId.substring(0, 8)}.`);
      attachRunStream(sessionId, runId);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      addTrace(`Failed to start run: ${errorMessage}`);
      setMessages((prev) => [
        ...prev,
        {
          id: Math.random().toString(),
          role: "agent",
          type: "text",
          content: `[Error: ${errorMessage}]`,
        },
      ]);
      setIsThinking(false);
    }
  };

  const resetSession = async () => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    localStorage.removeItem(SECOND_BRAIN_SESSION_KEY);
    setMessages([]);
    setSessionId(null);
    setIsThinking(false);
    setRuntimeTrace([]);
    setPendingApprovals([]);
    try {
      const sid = await api.createSession("Second Brain MVP");
      localStorage.setItem(SECOND_BRAIN_SESSION_KEY, sid);
      setSessionId(sid);
      addTrace(`Session reset. New session ${sid.substring(0, 8)}.`);
      setMessages([
        {
          id: "welcome",
          role: "agent",
          type: "text",
          content: "Session reset. Ask me to inspect files, git, or sqlite on this server.",
        },
      ]);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      addTrace(`Reset failed: ${errorMessage}`);
      setMessages([
        {
          id: "err-reset",
          role: "agent",
          type: "text",
          content: `Failed to reset session: ${errorMessage}`,
        },
      ]);
    }
  };

  return (
    <div className="flex h-full bg-gray-950 text-white">
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Header */}
        <div className="bg-gray-900 border-b border-gray-800 p-6 shrink-0">
          <div className="flex items-center justify-between">
            <h2 className="text-2xl font-bold flex items-center gap-2 text-indigo-400">
              <BrainCircuit />
              Unified Local Brain
            </h2>
            <button
              type="button"
              onClick={resetSession}
              className="px-3 py-1.5 text-xs border border-gray-700 rounded text-gray-300 hover:text-white hover:bg-gray-800"
            >
              Reset Session
            </button>
          </div>
          <p className="text-gray-400 mt-2 text-sm flex items-center gap-4">
            <span>
              <FileCode2 className="inline mr-1" size={14} /> Local Files Access
            </span>
            <span>
              <Database className="inline mr-1" size={14} /> SQLite Queries
            </span>
            <span>
              <FolderGit2 className="inline mr-1" size={14} /> Git Sync
            </span>
          </p>
          <p className="text-gray-500 mt-2 text-xs">
            Demonstrates MCP integration running on the local VPS engine daemon.
          </p>
          <div className="mt-3 border border-gray-800 rounded bg-gray-950/70">
            <div className="px-3 py-1.5 text-[11px] tracking-wide text-gray-400 border-b border-gray-800 flex items-center justify-between gap-2">
              <span>RUNTIME TRACE</span>
              <div className="flex items-center gap-2">
                {pendingApprovals.length > 0 && (
                  <span className="text-amber-300">
                    Pending approvals: {pendingApprovals.length}
                  </span>
                )}
                <button
                  type="button"
                  onClick={() => void approvePendingForSession()}
                  disabled={!sessionId || pendingApprovals.length === 0}
                  className="px-2 py-0.5 rounded border border-gray-700 text-gray-300 hover:text-white hover:bg-gray-800 disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  Approve Pending
                </button>
              </div>
            </div>
            <div className="px-3 py-2 max-h-24 overflow-y-auto space-y-1">
              {runtimeTrace.length === 0 ? (
                <p className="text-[11px] text-gray-600">No runtime events yet.</p>
              ) : (
                runtimeTrace.slice(-8).map((entry) => (
                  <p key={entry.id} className="text-[11px] text-gray-300 font-mono">
                    <span className="text-gray-500 mr-2">
                      {entry.timestamp.toLocaleTimeString()}
                    </span>
                    {entry.content}
                  </p>
                ))
              )}
            </div>
          </div>
        </div>

        {/* Chat Area */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          {messages.map((m) => (
            <div
              key={m.id}
              className={`flex ${m.role === "user" ? "justify-end" : "justify-start"}`}
            >
              {m.type === "text" && (
                <div
                  className={`max-w-[75%] rounded-2xl px-5 py-3 ${
                    m.role === "user"
                      ? "bg-indigo-600 text-white shadow-md"
                      : "bg-gray-800 text-gray-200 border border-gray-700"
                  }`}
                >
                  <p className="whitespace-pre-wrap leading-relaxed">{m.content}</p>
                </div>
              )}
              {m.type === "tool_start" && (
                <div className="max-w-[75%] rounded-lg px-3 py-2 text-yellow-500 flex items-center gap-2 bg-gray-800/50 border border-yellow-900/30">
                  <Loader2 size={16} className="animate-spin" /> {m.content}
                </div>
              )}
              {m.type === "tool_end" && m.toolResult && (
                <div className="max-w-[85%]">
                  <ToolCallResult
                    toolName={m.toolName || "tool"}
                    resultString={m.toolResult}
                    defaultExpanded={false}
                  />
                </div>
              )}
            </div>
          ))}
          {isThinking && (
            <div className="flex justify-start">
              <div className="bg-gray-800 border border-gray-700 rounded-2xl px-5 py-3 flex gap-1 items-center">
                <div
                  className="w-2 h-2 bg-gray-500 rounded-full animate-bounce"
                  style={{ animationDelay: "0ms" }}
                ></div>
                <div
                  className="w-2 h-2 bg-gray-500 rounded-full animate-bounce"
                  style={{ animationDelay: "150ms" }}
                ></div>
                <div
                  className="w-2 h-2 bg-gray-500 rounded-full animate-bounce"
                  style={{ animationDelay: "300ms" }}
                ></div>
              </div>
            </div>
          )}
          <div ref={messagesEndRef} />
        </div>

        {/* Input Bar */}
        <div className="p-4 bg-gray-900 border-t border-gray-800 shrink-0">
          <form onSubmit={handleSend} className="max-w-4xl mx-auto relative flex items-center">
            <input
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder="Ask the engine to read a local file or query a database via MCP..."
              className="w-full bg-gray-800 border border-gray-700 text-white rounded-full pl-6 pr-12 py-3 focus:outline-none focus:ring-2 focus:ring-indigo-500 shadow-inner"
              disabled={isThinking || !sessionId}
            />
            <button
              type="submit"
              disabled={!input.trim() || isThinking || !sessionId}
              className="absolute right-2 p-2 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white rounded-full transition-colors flex items-center justify-center"
            >
              <Send size={18} />
            </button>
          </form>
        </div>
      </div>

      {/* Sidebar right for Session History */}
      <div className="w-80 shrink-0 border-l border-gray-800 bg-gray-900 overflow-y-auto">
        <SessionHistory
          currentSessionId={sessionId}
          onSelectSession={loadSession}
          query="Second Brain"
          className="w-full"
        />
      </div>
    </div>
  );
};
