import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { MessageSquarePlus, Send, Loader2, Inbox } from "lucide-react";
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

const TICKET_SESSION_KEY = "tandem_portal_ticket_session_id";

export const TicketTriageDashboard: React.FC = () => {
  const [ticketContent, setTicketContent] = useState("");
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
      localStorage.removeItem(TICKET_SESSION_KEY);
      return;
    }

    try {
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(TICKET_SESSION_KEY, sessionId);
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
          content: `Restored triage session: ${sessionId.substring(0, 8)}`,
        },
        ...restored,
      ]);
    } catch {
      setCurrentSessionId(null);
    }
  };

  useEffect(() => {
    const existingSessionId = localStorage.getItem(TICKET_SESSION_KEY);
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
    if (!ticketContent.trim() || isRunning) return;

    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    setLogs([]);
    setIsRunning(true);
    setCurrentSessionId(null);
    addLog({ type: "system", content: "Initializing Support Inbox sequence..." });

    try {
      const sessionId = await api.createSession(`Ticket: ${ticketContent.substring(0, 20)}`);
      localStorage.setItem(TICKET_SESSION_KEY, sessionId);
      setCurrentSessionId(sessionId);

      const prompt = `You are a Customer Support Triage Agent.
A new support ticket has arrived in the Inbox.

Ticket Content:
${ticketContent}

Instructions:
1. Read the ticket and classify the issue.
2. Search the codebase or use the webfetch tool to retrieve the relevant Knowledge Base articles to diagnose or solve the issue.
3. Once you determine the mitigation, draft a highly polite and empathetic email response for the customer.
4. Output the triage metadata (urgency, category) to 'out/triage.json'.
5. Do NOT send the email. Instead, output the drafted response to 'out/drafts/response.md' for human-in-the-loop review.
Use your tools to achieve this.`;

      const { runId } = await api.startAsyncRun(sessionId, prompt);
      const eventSource = new EventSource(api.getEventStreamUrl(sessionId, runId));
      eventSourceRef.current = eventSource;
      let finalized = false;

      const finalizeRun = (status: string) => {
        if (finalized) return;
        finalized = true;
        addLog({
          type: "system",
          content: `Ticket triage finished with status: ${status}. Draft awaits approval in out/drafts.`,
        });
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
                  content: `Executing Support Action: ${part.tool}`,
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
                  content: `Action completed: ${part.tool}`,
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
              <MessageSquarePlus className="text-indigo-500" />
              Ticket Triage & Support Drafts
            </h2>
            <p className="text-gray-400 mt-1">
              Simulates an Inbox flow. The Agent classifies the ticket, queries KB tools, and queues
              a draft response for Human approval.
            </p>
          </div>
        </div>

        <form onSubmit={handleStart} className="flex gap-4 mb-6 shrink-0">
          <div className="flex-1 flex gap-4 bg-gray-900 border border-gray-800 rounded-lg p-2 focus-within:ring-2 focus-within:ring-indigo-500 transition-shadow">
            <div className="flex items-center pl-2 text-indigo-400">
              <Inbox size={24} />
            </div>
            <textarea
              value={ticketContent}
              onChange={(e) => setTicketContent(e.target.value)}
              placeholder="Paste an incoming customer ticket: E.g., 'Hello, I forgot my password but I don't have access to my email anymore...'"
              className="flex-1 bg-transparent px-2 py-2 text-white placeholder:text-gray-500 focus:outline-none min-h-[80px]"
              disabled={isRunning}
            />
          </div>
          <button
            type="submit"
            disabled={isRunning || !ticketContent.trim()}
            className="bg-indigo-600 hover:bg-indigo-700 disabled:opacity-50 text-white px-6 py-3 rounded-lg font-medium flex items-center gap-2 transition-colors shrink-0 max-h-[96px]"
          >
            {isRunning ? <Loader2 className="animate-spin" size={20} /> : <Send size={20} />}
            {isRunning ? "Diagnosing..." : "Triage Ticket"}
          </button>
        </form>

        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
          <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider shrink-0 flex justify-between">
            <span>Agentic Inbox Log</span>
            {currentSessionId && (
              <span className="text-indigo-400 font-mono opacity-60">
                ID: {currentSessionId.substring(0, 8)}
              </span>
            )}
          </div>
          <div className="flex-1 overflow-y-auto p-4 space-y-4">
            {logs.length === 0 && (
              <div className="text-gray-600 text-center mt-10 italic">
                Awaiting incoming tickets...
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
                    <span className="text-indigo-400 font-semibold mt-1 inline-block">
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
          className="w-full"
        />
      </div>
    </div>
  );
};
