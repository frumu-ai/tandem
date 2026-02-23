import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { Clock, Loader2, Play } from "lucide-react";
import { SessionHistory } from "../components/SessionHistory";
import { ToolCallResult } from "../components/ToolCallResult";
import { attachPortalRunStream } from "../utils/portalRunStream";

interface LogEvent {
  id: string;
  timestamp: Date;
  type: "tool_start" | "tool_end" | "text" | "system";
  content: string;
  toolName?: string;
  toolResult?: string;
}

const WATCH_SESSION_KEY = "tandem_portal_watch_session_id";

export const ScheduledWatchDashboard: React.FC = () => {
  const [targetUrl, setTargetUrl] = useState("");
  const [schedule, setSchedule] = useState("0 9 * * *"); // Default: 9 AM every day
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

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
          content: `Agent action: ${tool}`,
          toolName: tool,
        }),
      onToolEnd: ({ tool, result }) =>
        addLog({
          type: "tool_end",
          content: `Action completed: ${tool}`,
          toolName: tool,
          toolResult: result,
        }),
      onFinalize: (status) => {
        addLog({
          type: "system",
          content: `Watch execution finished with status: ${status}. Digest saved.`,
        });
        setIsRunning(false);
      },
    });
  };

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(WATCH_SESSION_KEY);
      return;
    }

    try {
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(WATCH_SESSION_KEY, sessionId);
      const messages = await api.getSessionMessages(sessionId);

      const restored: LogEvent[] = messages.flatMap((m) => {
        if (m.info?.role === "assistant" || m.info?.role === "user") {
          const text = (m.parts || [])
            .filter((p) => p.type === "text")
            .map((p) => p.text)
            .join("\n");
          if (text) {
            return {
              id: Math.random().toString(),
              timestamp: new Date(),
              type: "text",
              content: m.info?.role === "assistant" ? text : `User: ${text}`,
            };
          }
        }
        return [];
      });

      setLogs([
        {
          id: "sys-restore",
          timestamp: new Date(),
          type: "system",
          content: `Restored watch session: ${sessionId.substring(0, 8)}`,
        },
        ...restored,
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
    } catch {
      setCurrentSessionId(null);
    }
  };

  useEffect(() => {
    const existingSessionId = localStorage.getItem(WATCH_SESSION_KEY);
    if (existingSessionId) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      void loadSession(existingSessionId);
    }
    return () => {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }
    };
  }, []);

  const addLog = (event: Omit<LogEvent, "id" | "timestamp">) => {
    setLogs((prev) => {
      if (event.type === "tool_end" && event.toolName) {
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
        { ...event, id: Math.random().toString(36).substring(7), timestamp: new Date() },
      ];
    });
  };

  const handleStart = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!targetUrl.trim() || isRunning) return;

    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    setLogs([]);
    setIsRunning(true);
    setCurrentSessionId(null);
    addLog({ type: "system", content: `Starting Daily Watch for ${targetUrl}...` });

    try {
      const sessionId = await api.createSession(`Watch: ${targetUrl.substring(0, 20)}`);
      localStorage.setItem(WATCH_SESSION_KEY, sessionId);
      setCurrentSessionId(sessionId);

      const prompt = `You are a Scheduled Watchdog Agent.
Your target is: ${targetUrl}
The cron schedule for this watch is: ${schedule}

Instructions:
1. Fetch the target URL using your webfetch tool.
2. Check your memory store or tool outputs for a previous snapshot of this URL.
3. Compare the new content with the old snapshot.
4. Output a digest of the changes to 'out/digest.md'.
5. If there are diffs, save them in 'out/diffs/'.
6. Save the new snapshot state to 'out/watch_state.json'.
7. Queue a notification or summary of the digest for the user.`;

      const { runId } = await api.startAsyncRun(sessionId, prompt);
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
        <div className="mb-6 flex justify-between items-start">
          <div>
            <h2 className="text-2xl font-bold text-white flex items-center gap-2">
              <Clock className="text-pink-500" />
              Scheduled Watch & Digests
            </h2>
            <p className="text-gray-400 mt-1">
              Set up cron jobs to run periodic webfetches, diff against earlier snapshots, and
              generate actionable digests.
            </p>
          </div>
        </div>

        <form
          onSubmit={handleStart}
          className="flex flex-col gap-4 mb-6 shrink-0 bg-gray-900 border border-gray-800 p-4 rounded-lg"
        >
          <div className="flex items-center gap-4">
            <label className="text-gray-400 text-sm w-24">Target URL</label>
            <input
              type="text"
              value={targetUrl}
              onChange={(e) => setTargetUrl(e.target.value)}
              placeholder="https://news.ycombinator.com or https://github.com/microsoft/vscode/releases"
              className="flex-1 bg-gray-950 border border-gray-800 rounded px-3 py-2 text-white placeholder-gray-600 focus:outline-none focus:border-pink-500"
              disabled={isRunning}
            />
          </div>
          <div className="flex items-center gap-4">
            <label className="text-gray-400 text-sm w-24">Cron String</label>
            <input
              type="text"
              value={schedule}
              onChange={(e) => setSchedule(e.target.value)}
              placeholder="0 9 * * *"
              className="w-48 bg-gray-950 border border-gray-800 rounded px-3 py-2 text-white font-mono placeholder-gray-600 focus:outline-none focus:border-pink-500"
              disabled={isRunning}
            />
            <span className="text-gray-500 text-xs">
              (Mock scheduling: hitting start forces an immediate run)
            </span>
          </div>
          <div className="flex justify-end mt-2">
            <button
              type="submit"
              disabled={isRunning || !targetUrl.trim()}
              className="bg-pink-600 hover:bg-pink-700 disabled:opacity-50 text-white px-6 py-2 rounded font-medium flex items-center gap-2 transition-colors"
            >
              {isRunning ? <Loader2 className="animate-spin" size={18} /> : <Play size={18} />}
              {isRunning ? "Running Watch..." : "Force Run Now"}
            </button>
          </div>
        </form>

        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
          <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider shrink-0 flex justify-between">
            <span>Watch Execution Log</span>
            {currentSessionId && (
              <span className="text-pink-400 font-mono opacity-60">
                ID: {currentSessionId.substring(0, 8)}
              </span>
            )}
          </div>
          <div className="flex-1 overflow-y-auto p-4 space-y-4">
            {logs.length === 0 && (
              <div className="text-gray-600 text-center mt-10 italic">
                Awaiting target and schedule details...
              </div>
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
                    <span className="text-pink-400 font-semibold mt-1 inline-block">
                      {log.content}
                    </span>
                  )}
                  {log.type === "tool_start" && (
                    <span className="text-blue-500 flex items-center gap-1 mt-1 opacity-75">
                      <Loader2 size={14} className="animate-spin inline" /> {log.content}
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
                      <span className="text-blue-500 flex items-center gap-1 mt-1">
                        {log.content}
                      </span>
                    )
                  )}
                  {log.type === "text" && (
                    <div className="text-gray-300 mt-1 whitespace-pre-wrap">{log.content}</div>
                  )}
                </div>
              </div>
            ))}
            <div ref={logsEndRef} />
          </div>
        </div>
      </div>

      <div className="w-80 shrink-0 border-l border-gray-800 bg-gray-900">
        <SessionHistory
          currentSessionId={currentSessionId}
          onSelectSession={loadSession}
          query="Watch:"
          scopePrefix="Watch:"
          className="w-full"
        />
      </div>
    </div>
  );
};
