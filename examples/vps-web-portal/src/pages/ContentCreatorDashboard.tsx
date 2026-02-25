import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { PenTool, Send, Loader2 } from "lucide-react";
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

const CONTENT_SESSION_KEY = "tandem_portal_content_session_id";

export const ContentCreatorDashboard: React.FC = () => {
  const [topic, setTopic] = useState("");
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
          content: `Pipeline Step: ${tool}`,
          toolName: tool,
        }),
      onToolEnd: ({ tool, result }) =>
        addLog({
          type: "tool_end",
          content: `Step Completed: ${tool}`,
          toolName: tool,
          toolResult: result,
        }),
      onFinalize: (status) => {
        addLog({
          type: "system",
          content: `Pipeline finished with status: ${status}. Artifacts are stored in 'out/'.`,
        });
        setIsRunning(false);
      },
    });
  };

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(CONTENT_SESSION_KEY);
      return;
    }

    try {
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(CONTENT_SESSION_KEY, sessionId);
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
          content: `Restored content session: ${sessionId.substring(0, 8)}`,
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
    const existingSessionId = localStorage.getItem(CONTENT_SESSION_KEY);
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
    if (!topic.trim() || isRunning) return;

    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    setLogs([]);
    setIsRunning(true);
    setCurrentSessionId(null);
    addLog({ type: "system", content: "Initializing Content Creation Pipeline..." });

    try {
      const sessionId = await api.createSession(`Content: ${topic.substring(0, 20)}`);
      localStorage.setItem(CONTENT_SESSION_KEY, sessionId);
      setCurrentSessionId(sessionId);

      const prompt = `You are an Expert Technical Writer and Content Marketer.
The user wants a comprehensive blog post and social media snippets about: "${topic}"

Instructions - Follow this exact pipeline:
1. Research Strategy (Plan + Todo): Create an outline and research plan. Save to 'out/todos.json' and 'out/outline.md'.
2. Ingestion: Use the webfetch or websearch tools to gather context and capture source links.
3. Drafting: Write a comprehensive, well-structured markdown post with concrete examples.
4. Editing: Review against best practices, refine the headers. Save the final blog post to 'out/post.md'.
5. Distribution: Generate 3 platform-specific variants (X thread opener, LinkedIn post, newsletter blurb). Save to 'out/social_snippets.md'.
6. Include a short "fact-check notes" section listing which claims require verification.
Complete all steps autonomously without stopping.`;

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
              <PenTool className="text-purple-500" />
              Content Creation Pipeline
            </h2>
            <p className="text-gray-400 mt-1">
              Enter a topic. The agent will outline, research, draft, edit, and generate social
              media copy autonomously.
            </p>
          </div>
        </div>

        <form
          onSubmit={handleStart}
          className="flex flex-col gap-3 sm:flex-row sm:gap-4 mb-6 shrink-0"
        >
          <input
            type="text"
            value={topic}
            onChange={(e) => setTopic(e.target.value)}
            placeholder="E.g., Write a practical CTO guide: 'How to deploy AI agents in production without creating a security nightmare'"
            className="flex-1 bg-gray-900 border border-gray-800 rounded-lg px-4 py-3 text-white focus:outline-none focus:ring-2 focus:ring-purple-500"
            disabled={isRunning}
          />
          <button
            type="submit"
            disabled={isRunning || !topic.trim()}
            className="bg-purple-600 hover:bg-purple-700 disabled:opacity-50 text-white px-6 py-3 rounded-lg font-medium flex items-center gap-2 transition-colors shrink-0"
          >
            {isRunning ? <Loader2 className="animate-spin" size={20} /> : <Send size={20} />}
            {isRunning ? "Generating Pipeline..." : "Start Draft"}
          </button>
        </form>

        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
          <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider shrink-0 flex justify-between">
            <span>Pipeline Trace</span>
            {currentSessionId && (
              <span className="text-purple-400 font-mono opacity-60">
                ID: {currentSessionId.substring(0, 8)}
              </span>
            )}
          </div>
          <div ref={logsContainerRef} className="flex-1 overflow-y-auto p-4 space-y-4">
            {logs.length === 0 && (
              <div className="text-gray-600 text-center mt-10 italic">
                Awaiting a topic to start the pipeline...
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
                    <span className="text-purple-400 font-semibold mt-1 inline-block">
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
          query="Content:"
          scopePrefix="Content:"
          className="w-full"
        />
      </div>
    </div>
  );
};
