import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { Code, Send, Loader2, AlertTriangle } from "lucide-react";
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

const HTML_SESSION_KEY = "tandem_portal_html_session_id";

export const HtmlExtractorDashboard: React.FC = () => {
  const [targetUrl, setTargetUrl] = useState("");
  const [extractGoal, setExtractGoal] = useState(
    "Extract pricing tiers, limits, and feature differences from this page into structured JSON."
  );
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const logsContainerRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    if (!logsContainerRef.current) return;
    logsContainerRef.current.scrollTop = logsContainerRef.current.scrollHeight;
  }, [logs]);

  const attachRunStream = (sessionId: string, runId: string) => {
    attachPortalRunStream(eventSourceRef, sessionId, runId, {
      addSystemLog: (content) => addLog({ type: "system", content }),
      addTextDelta: (delta) => addLog({ type: "text", content: delta }),
      onToolStart: ({ tool }) =>
        addLog({
          type: "tool_start",
          content: `Executing Tool: ${tool}`,
          toolName: tool,
        }),
      onToolEnd: ({ tool, result }) =>
        addLog({
          type: "tool_end",
          content: `Tool Completed: ${tool}`,
          toolName: tool,
          toolResult: result,
        }),
      onFinalize: (status) => {
        addLog({
          type: "system",
          content: `Extraction finished with status: ${status}. Results in out/url_extract.md and out/url_entities.json.`,
        });
        setIsRunning(false);
      },
    });
  };

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(HTML_SESSION_KEY);
      return;
    }

    try {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(HTML_SESSION_KEY, sessionId);
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
          content: `Restored HTML extraction session: ${sessionId.substring(0, 8)}`,
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
        addLog({
          type: "system",
          content: `Resuming active run: ${activeRunId.substring(0, 8)}`,
        });
        attachRunStream(sessionId, activeRunId);
      } else {
        setIsRunning(false);
      }
    } catch {
      setCurrentSessionId(null);
    }
  };

  useEffect(() => {
    const existingSessionId = localStorage.getItem(HTML_SESSION_KEY);
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
    if (!targetUrl.trim() || !extractGoal.trim() || isRunning) return;

    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    setLogs([]);
    setIsRunning(true);
    setCurrentSessionId(null);
    addLog({ type: "system", content: "Initializing URL extraction (Markdown-first)..." });

    try {
      const sessionId = await api.createSession(`HTML Ext: ${targetUrl.substring(0, 15)}`);
      localStorage.setItem(HTML_SESSION_KEY, sessionId);
      setCurrentSessionId(sessionId);

      const prompt = `You are a URL Data Extraction Agent (Markdown-first).
Target URL: ${targetUrl}
Extraction Goal: ${extractGoal}

Instructions:
1. First call tool \`webfetch\` with explicit args: {"url":"${targetUrl}","mode":"markdown"}.
2. Produce a concise markdown extraction report and write it to 'out/url_extract.md'.
3. If key data appears missing in Markdown, call \`webfetch_html\` with explicit args: {"url":"${targetUrl}","reason":"<what markdown missed>"}.
4. If you used raw HTML fallback, extract hidden/script payloads and include them in 'out/dom_extract.json'.
5. Always provide final structured entities in 'out/url_entities.json'.`;

      const { runId } = await api.startAsyncRun(sessionId, prompt);
      attachRunStream(sessionId, runId);
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      addLog({ type: "system", content: `Error: ${errorMessage}` });
      setIsRunning(false);
    }
  };

  return (
    <div className="flex h-full flex-col xl:flex-row bg-gray-950">
      <div className="flex-1 min-h-0 flex flex-col p-3 sm:p-4 lg:p-6 overflow-hidden">
        <div className="mb-6 flex justify-between items-start">
          <div>
            <h2 className="text-2xl font-bold text-white flex items-center gap-2">
              <Code className="text-sky-500" />
              URL Markdown Extraction Demo
            </h2>
            <p className="text-gray-400 mt-1">
              Extract data from URLs in Markdown mode first for lower cost and cleaner output. Use
              raw HTML only as an escape hatch when critical fields are missing.
            </p>
            <div className="mt-3 inline-flex items-center gap-2 bg-amber-900/30 border border-amber-500/50 text-amber-200 px-3 py-1.5 rounded-md text-sm">
              <AlertTriangle size={16} />
              <strong>Note:</strong> Raw HTML fallback can increase token usage significantly and
              requires a reason.
            </div>
          </div>
        </div>

        <form
          onSubmit={handleStart}
          className="flex flex-col gap-4 mb-6 shrink-0 bg-gray-900 border border-gray-800 p-4 rounded-lg"
        >
          <div className="flex items-center gap-4">
            <label className="text-gray-400 text-sm w-32">Target URL</label>
            <input
              type="text"
              value={targetUrl}
              onChange={(e) => setTargetUrl(e.target.value)}
              placeholder="https://openai.com/pricing or another data-heavy product page"
              className="flex-1 bg-gray-950 border border-gray-800 rounded px-3 py-2 text-white placeholder-gray-600 focus:outline-none focus:border-sky-500"
              disabled={isRunning}
            />
          </div>
          <div className="flex flex-col gap-2">
            <label className="text-gray-400 text-sm">Extraction Goal</label>
            <textarea
              value={extractGoal}
              onChange={(e) => setExtractGoal(e.target.value)}
              placeholder="What exact fields do you need (e.g., plan_name, monthly_price, included_tokens, rate_limits)?"
              className="w-full bg-gray-950 border border-gray-800 rounded px-3 py-2 text-white placeholder-gray-600 focus:outline-none focus:border-sky-500 min-h-[80px]"
              disabled={isRunning}
            />
          </div>
          <div className="flex justify-end mt-2">
            <button
              type="submit"
              disabled={isRunning || !targetUrl.trim() || !extractGoal.trim()}
              className="bg-sky-600 hover:bg-sky-700 disabled:opacity-50 text-white px-6 py-2 rounded font-medium flex items-center gap-2 transition-colors"
            >
              {isRunning ? <Loader2 className="animate-spin" size={18} /> : <Send size={18} />}
              {isRunning ? "Extracting URL..." : "Run URL Extraction"}
            </button>
          </div>
        </form>

        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
          <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider shrink-0 flex justify-between">
            <span>Extraction Trace</span>
            {currentSessionId && (
              <span className="text-sky-400 font-mono opacity-60">
                ID: {currentSessionId.substring(0, 8)}
              </span>
            )}
          </div>
          <div ref={logsContainerRef} className="flex-1 overflow-y-auto p-4 space-y-4">
            {logs.length === 0 && (
              <div className="text-gray-600 text-center mt-10 italic">
                Awaiting target parameters...
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
                    <span className="text-sky-400 font-semibold mt-1 inline-block">
                      {log.content}
                    </span>
                  )}
                  {log.type === "tool_start" && (
                    <span className="text-yellow-500 flex items-center gap-1 mt-1 opacity-75">
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
                      <span className="text-yellow-500 flex items-center gap-1 mt-1">
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
          </div>
        </div>
      </div>

      <div className="w-full xl:w-80 shrink-0 border-t xl:border-t-0 xl:border-l border-gray-800 bg-gray-900 max-h-[45vh] xl:max-h-none">
        <SessionHistory
          currentSessionId={currentSessionId}
          onSelectSession={loadSession}
          query="HTML Ext:"
          scopePrefix="HTML Ext:"
          className="w-full"
        />
      </div>
    </div>
  );
};
