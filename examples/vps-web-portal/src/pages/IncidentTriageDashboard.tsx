import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { FileWarning, Send, Loader2, List, History, X } from "lucide-react";
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

const INCIDENT_SESSION_KEY = "tandem_portal_incident_session_id";

const buildRestoredLogsFromMessages = (
  messages: Awaited<ReturnType<typeof api.getSessionMessages>>
): LogEvent[] => {
  return messages.flatMap((m) => {
    if (m.info?.role === "assistant" || m.info?.role === "user") {
      const text = (m.parts || [])
        .filter((p) => p.type === "text")
        .map((p) => p.text)
        .join("\n");
      if (text) {
        return {
          id: Math.random().toString(),
          timestamp: new Date(),
          type: "text" as const,
          content: m.info?.role === "assistant" ? text : `User: ${text}`,
        };
      }
    }
    return [];
  });
};

export const IncidentTriageDashboard: React.FC = () => {
  const [incidentLogs, setIncidentLogs] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [historyOpen, setHistoryOpen] = useState(true);
  const [mobileHistoryOpen, setMobileHistoryOpen] = useState(false);
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
          content: `Executing Runbook Step: ${tool}`,
          toolName: tool,
        }),
      onToolEnd: ({ tool, result }) =>
        addLog({
          type: "tool_end",
          content: `Runbook Step completed: ${tool}`,
          toolName: tool,
          toolResult: result,
        }),
      onFinalize: (status) => {
        addLog({ type: "system", content: `Incident triage finished with status: ${status}` });
        setIsRunning(false);
      },
    });
  };

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(INCIDENT_SESSION_KEY);
      return;
    }

    try {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(INCIDENT_SESSION_KEY, sessionId);
      const messages = await api.getSessionMessages(sessionId);
      const restored = buildRestoredLogsFromMessages(messages);

      setLogs([
        {
          id: "sys-restore",
          timestamp: new Date(),
          type: "system",
          content: `Restored incident session: ${sessionId.substring(0, 8)}`,
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
    const existingSessionId = localStorage.getItem(INCIDENT_SESSION_KEY);
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
    if (!incidentLogs.trim() || isRunning) return;

    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    setLogs([]);
    setIsRunning(true);
    setCurrentSessionId(null);
    addLog({ type: "system", content: "Initializing Incident Triage sequence..." });

    try {
      const sessionId = await api.createSession(`Incident: ${incidentLogs.substring(0, 20)}`);
      localStorage.setItem(INCIDENT_SESSION_KEY, sessionId);
      setCurrentSessionId(sessionId);

      const prompt = `You are a Tier 3 SRE Incident Responder Agent.
Below are the incident logs or description provided by the user.

Incident Details:
${incidentLogs}

Instructions:
1. Analyze the logs to correlate the timeline and identify the root cause.
2. Read related configuration and application logs from the workspace to confirm the issue.
3. Propose runbook steps or remediation actions.
4. Include confidence levels and evidence for each hypothesis.
5. Output your formal incident report to 'out/incident_report.md' with:
   - Impact summary
   - Timeline
   - Root cause hypothesis + confidence
   - Immediate containment
   - Permanent fixes
6. Output proposed mitigation actions to 'out/proposed_actions.md'.
Use your tools to achieve this.`;

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
              <FileWarning className="text-orange-500" />
              Incident Triage & Runbook Executor
            </h2>
            <p className="text-gray-400 mt-1">
              Paste logs or error traces. The agent will read configs, correlate the timeline, and
              output a detailed incident report and mitigation plan.
            </p>
          </div>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => setHistoryOpen((prev) => !prev)}
              className="hidden sm:flex items-center gap-2 text-sm text-gray-300 border border-gray-700 rounded-full px-3 py-1 hover:border-white"
            >
              <List size={16} />
              {historyOpen ? "Hide History" : "Show History"}
            </button>
            <button
              type="button"
              onClick={() => setMobileHistoryOpen(true)}
              className="sm:hidden flex items-center gap-2 text-sm text-gray-300 border border-gray-700 rounded-full px-3 py-1 hover:border-white"
            >
              <History size={16} />
              Preview Sessions
            </button>
          </div>
        </div>

        <form
          onSubmit={handleStart}
          className="flex flex-col gap-3 sm:flex-row sm:gap-4 mb-6 shrink-0"
        >
          <textarea
            value={incidentLogs}
            onChange={(e) => setIncidentLogs(e.target.value)}
            placeholder="E.g., 2026-02-22 10:14:12Z API p95 jumped 120ms->4800ms; pod restarts increased; postgres connections at max=500; deploy sha=4b19f2 rolled out 8 minutes earlier..."
            className="flex-1 bg-gray-900 border border-gray-800 rounded-lg px-4 py-3 text-white focus:outline-none focus:ring-2 focus:ring-orange-500 min-h-[80px]"
            disabled={isRunning}
          />
          <button
            type="submit"
            disabled={isRunning || !incidentLogs.trim()}
            className="bg-orange-600 hover:bg-orange-700 disabled:opacity-50 text-white px-6 py-3 rounded-lg font-medium flex items-center gap-2 transition-colors shrink-0 max-h-[80px]"
          >
            {isRunning ? <Loader2 className="animate-spin" size={20} /> : <Send size={20} />}
            {isRunning ? "Triaging..." : "Triage Incident"}
          </button>
        </form>

        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
          <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider shrink-0 flex justify-between">
            <span>Triage Log & Runbook Trace</span>
            {currentSessionId && (
              <span className="text-orange-400 font-mono opacity-60">
                ID: {currentSessionId.substring(0, 8)}
              </span>
            )}
          </div>
          <div ref={logsContainerRef} className="flex-1 overflow-y-auto p-4 space-y-4">
            {logs.length === 0 && (
              <div className="text-gray-600 text-center mt-10 italic">
                Awaiting incident details...
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
                    <span className="text-orange-400 font-semibold mt-1 inline-block">
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

      {historyOpen && (
        <div className="w-full xl:w-80 shrink-0 border-t xl:border-t-0 xl:border-l border-gray-800 bg-gray-900 max-h-[45vh] xl:max-h-none">
          <SessionHistory
            currentSessionId={currentSessionId}
            onSelectSession={loadSession}
            query="Incident:"
            scopePrefix="Incident:"
            className="w-full"
          />
        </div>
      )}

      {mobileHistoryOpen && (
        <div className="fixed inset-0 z-50 xl:hidden">
          <button
            type="button"
            onClick={() => setMobileHistoryOpen(false)}
            className="absolute inset-0 bg-black/60"
            aria-label="Close session history"
          />
          <div className="absolute inset-x-0 bottom-0 max-h-[75vh] rounded-t-xl border border-gray-800 bg-gray-900 shadow-2xl flex flex-col">
            <div className="flex items-center justify-between px-4 py-3 border-b border-gray-800">
              <h3 className="text-sm font-semibold text-gray-200 flex items-center gap-2">
                <History size={15} />
                Recent Sessions
              </h3>
              <button
                type="button"
                onClick={() => setMobileHistoryOpen(false)}
                className="rounded border border-gray-700 p-1 text-gray-300 hover:text-white hover:bg-gray-800"
              >
                <X size={14} />
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto">
              <SessionHistory
                currentSessionId={currentSessionId}
                onSelectSession={(id) => {
                  setMobileHistoryOpen(false);
                  void loadSession(id);
                }}
                query="Incident:"
                scopePrefix="Incident:"
                className="w-full"
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
