import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { Loader2, Play, BotMessageSquare } from "lucide-react";
import { ToolCallResult } from "../components/ToolCallResult";
import { SessionHistory } from "../components/SessionHistory";
import { attachPortalRunStream } from "../utils/portalRunStream";

interface LogEvent {
  id: string;
  timestamp: Date;
  type: "tool_start" | "tool_end" | "text" | "system";
  content: string;
  toolName?: string;
  toolResult?: string;
}

const RESEARCH_SESSION_KEY = "tandem_portal_research_session_id";

const buildLogsFromMessages = (
  messages: Awaited<ReturnType<typeof api.getSessionMessages>>
): LogEvent[] => {
  return messages.flatMap((m) => {
    const logs: LogEvent[] = [];
    const role = m.info?.role;

    // We can also parse out past tool ends from messages if they are of type "tool_result"
    // For simplicity, we mostly just show the text messages from history.
    if (role === "user" || role === "assistant") {
      const text = (m.parts || [])
        .filter((p) => p.type === "text" && p.text)
        .map((p) => p.text)
        .join("\n")
        .trim();

      if (text) {
        logs.push({
          id: Math.random().toString(36).substring(7),
          timestamp: new Date(),
          type: "text" as const,
          content: role === "assistant" ? text : `User: ${text}`,
        });
      }
    }

    return logs;
  });
};

export const ResearchDashboard: React.FC = () => {
  const [query, setQuery] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  // Auto-scroll logs
  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const attachRunStream = (sessionId: string, runId: string) => {
    attachPortalRunStream(eventSourceRef, sessionId, runId, {
      addSystemLog: (content) => addLog({ type: "system", content }),
      addTextDelta: (delta) => addLog({ type: "text", content: delta }),
      onToolStart: ({ tool }) =>
        addLog({
          type: "tool_start",
          content: `Tool started: ${tool}`,
          toolName: tool,
        }),
      onToolEnd: ({ tool, result }) =>
        addLog({
          type: "tool_end",
          content: `Tool completed: ${tool}`,
          toolName: tool,
          toolResult: result,
        }),
      onFinalize: (status) => {
        addLog({ type: "system", content: `Run finished with status: ${status}` });
        setIsRunning(false);
      },
    });
  };

  useEffect(() => {
    const restore = async () => {
      const sessionId = localStorage.getItem(RESEARCH_SESSION_KEY);
      if (!sessionId) return;
      await loadSession(sessionId);
    };
    void restore();
    return () => {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }
    };
  }, []);

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(RESEARCH_SESSION_KEY);
      return;
    }

    try {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(RESEARCH_SESSION_KEY, sessionId);

      const messages = await api.getSessionMessages(sessionId);
      const restoredLogs = buildLogsFromMessages(messages);

      setLogs([
        {
          id: "sys-restore",
          timestamp: new Date(),
          type: "system",
          content: `Restored session: ${sessionId.substring(0, 8)}`,
        },
        ...restoredLogs,
      ]);

      const runState = await api.getActiveRun(sessionId);
      const active = runState?.active || null;
      const activeRunId =
        (active?.runID as string | undefined) ||
        (active?.runId as string | undefined) ||
        (active?.run_id as string | undefined) ||
        "";
      if (activeRunId) {
        setIsRunning(true);
        addLog({ type: "system", content: `Resuming active run: ${activeRunId.substring(0, 8)}` });
        attachRunStream(sessionId, activeRunId);
      } else {
        setIsRunning(false);
      }
    } catch (err) {
      console.error("Failed to restore research session", err);
      setCurrentSessionId(null);
      localStorage.removeItem(RESEARCH_SESSION_KEY);
    }
  };

  const addLog = (event: Omit<LogEvent, "id" | "timestamp">) => {
    setLogs((prev) => {
      // If this is a tool end, append the result to the matching tool_start
      if (event.type === "tool_end" && event.toolName) {
        // Fallback for older ES versions without findLastIndex
        let lastStartIdx = -1;
        for (let i = prev.length - 1; i >= 0; i--) {
          if (prev[i].type === "tool_start" && prev[i].toolName === event.toolName) {
            lastStartIdx = i;
            break;
          }
        }

        if (lastStartIdx !== -1) {
          const newLogs = [...prev];
          newLogs[lastStartIdx] = {
            ...newLogs[lastStartIdx],
            type: "tool_end",
            content: event.content,
            toolResult: event.toolResult,
          };
          return newLogs;
        }
      }

      return [
        ...prev,
        {
          ...event,
          id: Math.random().toString(36).substring(7),
          timestamp: new Date(),
        },
      ];
    });
  };

  const handleStart = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim() || isRunning) return;

    setLogs([]);
    setIsRunning(true);
    setCurrentSessionId(null);
    addLog({ type: "system", content: `Initializing research swarm for topic: "${query}"...` });

    try {
      // 1. Create a session
      const sessionId = await api.createSession(`Research: ${query.substring(0, 20)}...`);
      localStorage.setItem(RESEARCH_SESSION_KEY, sessionId);
      setCurrentSessionId(sessionId);
      addLog({ type: "system", content: `Session Created: ${sessionId.substring(0, 8)}` });

      // 2. Start the Run
      const prompt = `Use the webfetch and websearch tools to deeply research the following topic: ${query}. Give me a final unified summary of your findings.`;
      const { runId } = await api.startAsyncRun(sessionId, prompt);
      addLog({ type: "system", content: `Run Started: ${runId.substring(0, 8)}` });

      attachRunStream(sessionId, runId);
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      addLog({ type: "system", content: `Error: ${errorMessage}` });
      setIsRunning(false);
    }
  };

  return (
    <div className="flex h-full bg-gray-950">
      <div className="flex-1 flex flex-col p-6 overflow-hidden">
        <div className="mb-6">
          <h2 className="text-2xl font-bold text-white flex items-center gap-2">
            <BotMessageSquare className="text-blue-500" />
            Deep Research Dashboard
          </h2>
          <p className="text-gray-400 mt-1">
            Watch the engine think in real-time as it uses tools to browse the web.
          </p>
        </div>

        <form onSubmit={handleStart} className="flex gap-4 mb-6 shrink-0">
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="E.g., What are the latest advancements in quantum computing?"
            className="flex-1 bg-gray-900 border border-gray-800 rounded-lg px-4 py-3 text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
            disabled={isRunning}
          />
          <button
            type="submit"
            disabled={isRunning || !query.trim()}
            className="bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white px-6 py-3 rounded-lg font-medium flex items-center gap-2 transition-colors shrink-0"
          >
            {isRunning ? <Loader2 className="animate-spin" size={20} /> : <Play size={20} />}
            {isRunning ? "Researching..." : "Start Research"}
          </button>
        </form>

        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
          <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider shrink-0 flex justify-between">
            <span>Execution Log</span>
            {currentSessionId && (
              <span className="text-blue-400 font-mono opacity-60">
                ID: {currentSessionId.substring(0, 8)}
              </span>
            )}
          </div>
          <div className="flex-1 overflow-y-auto p-4 space-y-4">
            {logs.length === 0 && (
              <div className="text-gray-600 text-center mt-10 italic">Awaiting instructions...</div>
            )}
            {logs.map((log) => (
              <div key={log.id} className="flex gap-3">
                <span className="text-gray-600 shrink-0 mt-1">
                  {log.timestamp.toLocaleTimeString([], {
                    hour12: false,
                    hour: "2-digit",
                    minute: "2-digit",
                    second: "2-digit",
                  })}
                </span>
                <div className="flex-1 min-w-0">
                  {log.type === "system" && (
                    <span className="text-purple-400 font-semibold mt-1 inline-block">
                      {log.content}
                    </span>
                  )}
                  {log.type === "tool_start" && (
                    <span className="text-yellow-500 flex items-center gap-1 mt-1">
                      <Loader2 size={14} className="animate-spin inline" />
                      {log.content}
                    </span>
                  )}
                  {log.type === "tool_end" && log.toolResult ? (
                    <ToolCallResult
                      toolName={log.toolName!}
                      resultString={log.toolResult}
                      defaultExpanded={false}
                    />
                  ) : (
                    log.type === "tool_end" && (
                      <span className="text-emerald-500 flex items-center gap-1 mt-1">
                        {log.content}
                      </span>
                    )
                  )}
                  {log.type === "text" && (
                    <div className="text-blue-300 mt-1 whitespace-pre-wrap">{log.content}</div>
                  )}
                </div>
              </div>
            ))}
            <div ref={logsEndRef} />
          </div>
        </div>
      </div>

      {/* Sidebar right for Session History */}
      <div className="w-80 shrink-0 border-l border-gray-800 bg-gray-900">
        <SessionHistory
          currentSessionId={currentSessionId}
          onSelectSession={loadSession}
          query="Research:"
          scopePrefix="Research:"
          className="w-full"
        />
      </div>
    </div>
  );
};
