import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { FileWarning, Send, Loader2 } from "lucide-react";
import { SessionHistory } from "../components/SessionHistory";
import { ToolCallResult } from "../components/ToolCallResult";

interface LogEvent {
  id: string;
  timestamp: Date;
  type: "tool_start" | "tool_end" | "text" | "system";
  content: string;
  toolName?: string;
  toolResult?: string;
}

const INCIDENT_SESSION_KEY = "tandem_portal_incident_session_id";

export const IncidentTriageDashboard: React.FC = () => {
  const [incidentLogs, setIncidentLogs] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(INCIDENT_SESSION_KEY);
      return;
    }

    try {
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(INCIDENT_SESSION_KEY, sessionId);
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
          content: `Restored incident session: ${sessionId.substring(0, 8)}`,
        },
        ...restored,
      ]);
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
4. Output your formal incident report to 'out/incident_report.md'.
5. Output proposed mitigation actions to 'out/proposed_actions.md'.
Use your tools to achieve this.`;

      const { runId } = await api.startAsyncRun(sessionId, prompt);
      const eventSource = new EventSource(api.getEventStreamUrl(sessionId, runId));
      eventSourceRef.current = eventSource;
      let finalized = false;

      const finalizeRun = (status: string) => {
        if (finalized) return;
        finalized = true;
        addLog({ type: "system", content: `Incident triage finished with status: ${status}` });
        setIsRunning(false);
        eventSource.close();
      };

      eventSource.onmessage = (evt) => {
        try {
          const data = JSON.parse(evt.data);

          if (data.type === "message.part.updated") {
            const part = data.properties.part;
            if (part.type === "tool") {
              if (part.state.status === "running") {
                addLog({
                  type: "tool_start",
                  content: `Executing Runbook Step: ${part.tool}`,
                  toolName: part.tool,
                });
              } else if (part.state.status === "completed") {
                let rString = "";
                if (part.state.result) {
                  rString =
                    typeof part.state.result === "string"
                      ? part.state.result
                      : JSON.stringify(part.state.result);
                }
                addLog({
                  type: "tool_end",
                  content: `Runbook Step completed: ${part.tool}`,
                  toolName: part.tool,
                  toolResult: rString,
                });
              }
            } else if (part.type === "text" && data.properties.delta) {
              addLog({ type: "text", content: data.properties.delta });
            }
          } else if (
            data.type === "run.status.updated" &&
            (data.properties.status === "completed" || data.properties.status === "failed")
          ) {
            finalizeRun(data.properties.status);
          }
        } catch (e) {
          console.error("Stream parse error", e);
        }
      };

      eventSource.onerror = () => {
        addLog({ type: "system", content: "Stream disconnected." });
        setIsRunning(false);
        eventSource.close();
      };
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
              <FileWarning className="text-orange-500" />
              Incident Triage & Runbook Executor
            </h2>
            <p className="text-gray-400 mt-1">
              Paste logs or error traces. The agent will read configs, correlate the timeline, and
              output a detailed incident report and mitigation plan.
            </p>
          </div>
        </div>

        <form onSubmit={handleStart} className="flex gap-4 mb-6 shrink-0">
          <textarea
            value={incidentLogs}
            onChange={(e) => setIncidentLogs(e.target.value)}
            placeholder="E.g., Production database latency spiked to 5000ms. Log trace: ..."
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
          <div className="flex-1 overflow-y-auto p-4 space-y-4">
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
            <div ref={logsEndRef} />
          </div>
        </div>
      </div>

      <div className="w-80 shrink-0 border-l border-gray-800 bg-gray-900">
        <SessionHistory
          currentSessionId={currentSessionId}
          onSelectSession={loadSession}
          className="w-full"
        />
      </div>
    </div>
  );
};
