import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { Loader2, Play, CheckCircle2, BotMessageSquare } from "lucide-react";

interface LogEvent {
  id: string;
  timestamp: Date;
  type: "tool_start" | "tool_end" | "text" | "system";
  content: string;
  toolName?: string;
}

export const ResearchDashboard: React.FC = () => {
  const [query, setQuery] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const logsEndRef = useRef<HTMLDivElement>(null);

  // Auto-scroll logs
  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const addLog = (event: Omit<LogEvent, "id" | "timestamp">) => {
    setLogs((prev) => [
      ...prev,
      {
        ...event,
        id: Math.random().toString(36).substring(7),
        timestamp: new Date(),
      },
    ]);
  };

  const handleStart = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim() || isRunning) return;

    setLogs([]);
    setIsRunning(true);
    addLog({ type: "system", content: `Initializing research swarm for topic: "${query}"...` });

    try {
      // 1. Create a session
      const sessionId = await api.createSession("Deep Research Session");
      addLog({ type: "system", content: `Session Created: ${sessionId.substring(0, 8)}` });

      // 2. Start the Run
      const prompt = `Use the webfetch and websearch tools to deeply research the following topic: ${query}. Give me a final unified summary of your findings.`;
      const { runId } = await api.startAsyncRun(sessionId, prompt);
      addLog({ type: "system", content: `Run Started: ${runId.substring(0, 8)}` });

      // 3. Connect to the SSE stream
      const eventSource = new EventSource(api.getEventStreamUrl(sessionId, runId));

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
                addLog({
                  type: "tool_end",
                  content: `Tool completed: ${part.tool}`,
                  toolName: part.tool,
                });
              }
            } else if (part.type === "text" && data.properties.delta) {
              addLog({
                type: "text",
                content: data.properties.delta,
              });
            }
          } else if (
            data.type === "run.status.updated" &&
            (data.properties.status === "completed" || data.properties.status === "failed")
          ) {
            addLog({
              type: "system",
              content: `Run finished with status: ${data.properties.status}`,
            });
            setIsRunning(false);
            eventSource.close();
          }
        } catch (err) {
          console.error("Failed to parse SSE event", err);
        }
      };

      eventSource.onerror = (err) => {
        console.error("SSE Error", err);
        addLog({ type: "system", content: "Stream disconnected." });
        setIsRunning(false);
        eventSource.close();
      };
    } catch (error: any) {
      addLog({ type: "system", content: `Error: ${error.message}` });
      setIsRunning(false);
    }
  };

  return (
    <div className="flex flex-col h-full bg-gray-950 p-6">
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-white flex items-center gap-2">
          <BotMessageSquare className="text-blue-500" />
          Deep Research Dashboard
        </h2>
        <p className="text-gray-400 mt-1">
          Watch the engine think in real-time as it uses tools to browse the web.
        </p>
      </div>

      <form onSubmit={handleStart} className="flex gap-4 mb-6">
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
          className="bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white px-6 py-3 rounded-lg font-medium flex items-center gap-2 transition-colors"
        >
          {isRunning ? <Loader2 className="animate-spin" size={20} /> : <Play size={20} />}
          {isRunning ? "Researching..." : "Start Research"}
        </button>
      </form>

      <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
        <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider">
          Execution Log
        </div>
        <div className="flex-1 overflow-y-auto p-4 space-y-2">
          {logs.length === 0 && (
            <div className="text-gray-600 text-center mt-10 italic">Awaiting instructions...</div>
          )}
          {logs.map((log) => (
            <div key={log.id} className="flex gap-3">
              <span className="text-gray-600 shrink-0">
                {log.timestamp.toLocaleTimeString([], {
                  hour12: false,
                  hour: "2-digit",
                  minute: "2-digit",
                  second: "2-digit",
                })}
              </span>
              <div className="flex-1">
                {log.type === "system" && (
                  <span className="text-purple-400 font-semibold">{log.content}</span>
                )}
                {log.type === "tool_start" && (
                  <span className="text-yellow-500 flex items-center gap-1">
                    <Loader2 size={14} className="animate-spin inline" />
                    {log.content}
                  </span>
                )}
                {log.type === "tool_end" && (
                  <span className="text-emerald-500 flex items-center gap-1">
                    <CheckCircle2 size={14} className="inline" />
                    {log.content}
                  </span>
                )}
                {log.type === "text" && <span className="text-blue-300">{log.content}</span>}
              </div>
            </div>
          ))}
          <div ref={logsEndRef} />
        </div>
      </div>
    </div>
  );
};
