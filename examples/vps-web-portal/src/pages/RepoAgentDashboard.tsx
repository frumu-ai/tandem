import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { GitPullRequest, Send, Loader2, CheckCircle2, XCircle } from "lucide-react";
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

const REPO_SESSION_KEY = "tandem_portal_repo_agent_session_id";

export const RepoAgentDashboard: React.FC = () => {
  const [goal, setGoal] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [approvalPending, setApprovalPending] = useState(false);
  const logsEndRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(REPO_SESSION_KEY);
      return;
    }
    // Simple load logic
    try {
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(REPO_SESSION_KEY, sessionId);
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
          content: `Restored session: ${sessionId.substring(0, 8)}`,
        },
        ...restored,
      ]);
    } catch {
      setCurrentSessionId(null);
    }
  };

  useEffect(() => {
    const existingSessionId = localStorage.getItem(REPO_SESSION_KEY);
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
    if (!goal.trim() || isRunning) return;

    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    setLogs([]);
    setIsRunning(true);
    setApprovalPending(false);
    setCurrentSessionId(null);
    addLog({ type: "system", content: `Initializing Repo Agent for task: "${goal}"...` });

    try {
      const sessionId = await api.createSession(`Repo: ${goal.substring(0, 20)}`);
      localStorage.setItem(REPO_SESSION_KEY, sessionId);
      setCurrentSessionId(sessionId);

      const prompt = `You are an expert Software Engineer Repo Agent.
Your Goal: ${goal}.
Instructions:
1. Examine the codebase to plan the fix.
2. Produce a patch or write your implementation.
3. Write a PR description in out/pr_description.md.
4. IMPORTANT: Once you have planned and drafted the changes, wait for the user to approve before committing. Ask the user "Do you approve these changes?"
Use your tools to achieve this.`;

      const { runId } = await api.startAsyncRun(sessionId, prompt);
      const eventSource = new EventSource(api.getEventStreamUrl(sessionId, runId));
      eventSourceRef.current = eventSource;
      let finalized = false;

      const finalizeRun = (status: string) => {
        if (finalized) return;
        finalized = true;
        addLog({ type: "system", content: `Run finished with status: ${status}` });
        setIsRunning(false);
        eventSource.close();

        // Let's assume the agent asked for approval if the run completed
        if (status === "completed") {
          setApprovalPending(true);
        }
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
                  content: `Tool started: ${part.tool}`,
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
                  content: `Tool completed: ${part.tool}`,
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
          console.error("Stream parsing error", e);
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

  const handleApprove = async (approved: boolean) => {
    setApprovalPending(false);
    if (!currentSessionId) return;

    addLog({ type: "system", content: `User ${approved ? "approved" : "denied"} the execution.` });
    setIsRunning(true);

    try {
      const { runId } = await api.startAsyncRun(
        currentSessionId,
        approved
          ? "I approve. Please proceed with committing and finalizing the PR."
          : "I deny these changes. Stop execution."
      );
      const eventSource = new EventSource(api.getEventStreamUrl(currentSessionId, runId));
      eventSourceRef.current = eventSource;

      eventSource.onmessage = (evt) => {
        const data = JSON.parse(evt.data);
        if (
          data.type === "message.part.updated" &&
          data.properties.part.type === "text" &&
          data.properties.delta
        ) {
          addLog({ type: "text", content: data.properties.delta });
        } else if (
          data.type === "run.status.updated" &&
          (data.properties.status === "completed" || data.properties.status === "failed")
        ) {
          addLog({ type: "system", content: `Approval run finished: ${data.properties.status}` });
          setIsRunning(false);
          eventSource.close();
        }
      };
      eventSource.onerror = () => {
        setIsRunning(false);
        eventSource.close();
      };
    } catch {
      addLog({ type: "system", content: "Failed to send approval status." });
      setIsRunning(false);
    }
  };

  return (
    <div className="flex h-full bg-gray-950">
      <div className="flex-1 flex flex-col p-6 overflow-hidden">
        <div className="mb-6 flex justify-between items-start">
          <div>
            <h2 className="text-2xl font-bold text-white flex items-center gap-2">
              <GitPullRequest className="text-emerald-500" />
              Autonomous Repo Agent
            </h2>
            <p className="text-gray-400 mt-1">
              Describe a bug or feature. The agent will read the repo, write code, and draft a PR.
            </p>
          </div>
          {approvalPending && (
            <div className="bg-gray-900 border border-gray-700 p-4 rounded-lg flex items-center gap-4 animate-in fade-in slide-in-from-top-4">
              <span className="text-white font-medium">Gated Execution: Approval Required</span>
              <button
                onClick={() => handleApprove(true)}
                className="flex items-center gap-1 bg-emerald-600 hover:bg-emerald-500 text-white px-3 py-1.5 rounded text-sm transition-colors"
              >
                <CheckCircle2 size={16} /> Approve
              </button>
              <button
                onClick={() => handleApprove(false)}
                className="flex items-center gap-1 bg-red-600 hover:bg-red-500 text-white px-3 py-1.5 rounded text-sm transition-colors"
              >
                <XCircle size={16} /> Deny
              </button>
            </div>
          )}
        </div>

        <form onSubmit={handleStart} className="flex gap-4 mb-6 shrink-0">
          <input
            type="text"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            placeholder="E.g., Fix the off-by-one bug in the pagination component"
            className="flex-1 bg-gray-900 border border-gray-800 rounded-lg px-4 py-3 text-white focus:outline-none focus:ring-2 focus:ring-emerald-500"
            disabled={isRunning || approvalPending}
          />
          <button
            type="submit"
            disabled={isRunning || !goal.trim() || approvalPending}
            className="bg-emerald-600 hover:bg-emerald-700 disabled:opacity-50 text-white px-6 py-3 rounded-lg font-medium flex items-center gap-2 transition-colors shrink-0"
          >
            {isRunning ? <Loader2 className="animate-spin" size={20} /> : <Send size={20} />}
            {isRunning ? "Executing..." : "Start Agent"}
          </button>
        </form>

        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
          <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider shrink-0 flex justify-between">
            <span>Execution Log</span>
            {currentSessionId && (
              <span className="text-emerald-400 font-mono opacity-60">
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
                    <span className="text-emerald-500 flex items-center gap-1 mt-1 opacity-75">
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
                      <span className="text-emerald-500 flex items-center gap-1 mt-1">
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
