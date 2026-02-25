import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { GitPullRequest, Send, Loader2, CheckCircle2, XCircle } from "lucide-react";
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

const REPO_SESSION_KEY = "tandem_portal_repo_agent_session_id";

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

export const RepoAgentDashboard: React.FC = () => {
  const [goal, setGoal] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [approvalPending, setApprovalPending] = useState(false);
  const logsContainerRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    if (!logsContainerRef.current) return;
    logsContainerRef.current.scrollTop = logsContainerRef.current.scrollHeight;
  }, [logs]);

  const attachRunStream = (
    sessionId: string,
    runId: string,
    options?: {
      doneMessage?: string;
      onCompleted?: (status: string) => void;
    }
  ) => {
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
        if (options?.doneMessage) {
          addLog({ type: "system", content: options.doneMessage.replace("{status}", status) });
        } else {
          addLog({ type: "system", content: `Run finished with status: ${status}` });
        }
        setIsRunning(false);
        if (options?.onCompleted) options.onCompleted(status);
      },
    });
  };

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(REPO_SESSION_KEY);
      return;
    }
    // Simple load logic
    try {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(REPO_SESSION_KEY, sessionId);
      const messages = await api.getSessionMessages(sessionId);
      const restored = buildRestoredLogsFromMessages(messages);

      setLogs([
        {
          id: "sys-restore",
          timestamp: new Date(),
          type: "system",
          content: `Restored session: ${sessionId.substring(0, 8)}`,
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
        setApprovalPending(false);
        addLog({ type: "system", content: `Resuming active run: ${activeRunId.substring(0, 8)}` });
        attachRunStream(sessionId, activeRunId, {
          onCompleted: (status) => {
            if (status === "completed" || status === "inactive") {
              setApprovalPending(true);
            }
          },
        });
      } else {
        setIsRunning(false);
      }
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
2. Produce a minimal patch with clear reasoning and avoid unrelated refactors.
3. Add or update tests when behavior changes.
4. Write a PR description in out/pr_description.md with: problem, root cause, fix, risk, and test plan.
5. IMPORTANT: Once you have planned and drafted the changes, wait for the user to approve before committing. Ask the user "Do you approve these changes?"
Use your tools to achieve this.`;

      const { runId } = await api.startAsyncRun(sessionId, prompt);
      attachRunStream(sessionId, runId, {
        onCompleted: (status) => {
          if (status === "completed" || status === "inactive") {
            setApprovalPending(true);
          }
        },
      });
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
      attachRunStream(currentSessionId, runId, {
        doneMessage: "Approval run finished: {status}",
      });
    } catch {
      addLog({ type: "system", content: "Failed to send approval status." });
      setIsRunning(false);
    }
  };

  return (
    <div className="flex h-full flex-col xl:flex-row bg-gray-950">
      <div className="flex-1 min-h-0 flex flex-col p-3 sm:p-4 lg:p-6 overflow-hidden">
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

        <form
          onSubmit={handleStart}
          className="flex flex-col gap-3 sm:flex-row sm:gap-4 mb-6 shrink-0"
        >
          <input
            type="text"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            placeholder="E.g., Fix websocket reconnect race in stream client and add regression tests for dropped SSE connections"
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
          <div ref={logsContainerRef} className="flex-1 overflow-y-auto p-4 space-y-4">
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
          </div>
        </div>
      </div>

      <div className="w-full xl:w-80 shrink-0 border-t xl:border-t-0 xl:border-l border-gray-800 bg-gray-900 max-h-[45vh] xl:max-h-none">
        <SessionHistory
          currentSessionId={currentSessionId}
          onSelectSession={loadSession}
          query="Repo:"
          scopePrefix="Repo:"
          className="w-full"
        />
      </div>
    </div>
  );
};
