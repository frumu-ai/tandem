import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { DatabaseZap, Send, Loader2 } from "lucide-react";
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

const DATA_EXTRACT_SESSION_KEY = "tandem_portal_data_extract_session_id";

export const DataExtractionDashboard: React.FC = () => {
  const [sourceData, setSourceData] = useState("");
  const [jsonSchema, setJsonSchema] = useState(
    '{\n  "type": "object",\n  "properties": {\n    "name": { "type": "string" },\n    "price": { "type": "number" }\n  }\n}'
  );
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
          content: `Extracting via tool: ${tool}`,
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
        addLog({ type: "system", content: `Data extraction finished with status: ${status}` });
        setIsRunning(false);
      },
    });
  };

  const loadSession = async (sessionId: string) => {
    if (!sessionId) {
      setLogs([]);
      setCurrentSessionId(null);
      localStorage.removeItem(DATA_EXTRACT_SESSION_KEY);
      return;
    }

    try {
      setLogs([]);
      setCurrentSessionId(sessionId);
      localStorage.setItem(DATA_EXTRACT_SESSION_KEY, sessionId);
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
          content: `Restored extraction session: ${sessionId.substring(0, 8)}`,
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
    const existingSessionId = localStorage.getItem(DATA_EXTRACT_SESSION_KEY);
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
    if (!sourceData.trim() || !jsonSchema.trim() || isRunning) return;

    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    setLogs([]);
    setIsRunning(true);
    setCurrentSessionId(null);
    addLog({ type: "system", content: "Initializing Data Extraction pipeline..." });

    try {
      const sessionId = await api.createSession(`Extract: ${sourceData.substring(0, 20)}`);
      localStorage.setItem(DATA_EXTRACT_SESSION_KEY, sessionId);
      setCurrentSessionId(sessionId);

      const prompt = `You are an expert Data Extraction Agent.
Target Source (URL or File Path or raw text target): ${sourceData}

Required JSON Schema for the output records:
\`\`\`json
${jsonSchema}
\`\`\`

Instructions:
1. Use the webfetch or mcp file read tools to retrieve the messy unstructured data from the Target Source.
2. Parse the content and extract fields that map cleanly to the JSON Schema provided.
3. Validate your output against the schema.
4. Output the final structured JSON records to 'out/records.json'.
5. Additionally, generate a CSV version in 'out/records.csv'.
6. If any fields could not be mapped, write a note in 'out/validation_report.md'.`;

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
              <DatabaseZap className="text-blue-500" />
              Structured Data Extraction
            </h2>
            <p className="text-gray-400 mt-1">
              Provide a URL or file path, along with a target JSON schema. The engine will scrape,
              parse, and structure messy data into normalized JSON/CSV artifacts.
            </p>
            <p className="text-sm mt-2">
              <a
                href="https://microsoftedge.github.io/Demos/json-dummy-data/"
                target="_blank"
                rel="noopener noreferrer"
                className="text-blue-400 hover:text-blue-300 underline"
              >
                Need sample JSON data? Open the JSON Dummy Data generator.
              </a>
            </p>
          </div>
        </div>

        <form onSubmit={handleStart} className="flex flex-col gap-4 mb-6 shrink-0">
          <input
            type="text"
            value={sourceData}
            onChange={(e) => setSourceData(e.target.value)}
            placeholder="Target Source (e.g., https://example.com/products or /var/www/data.html)"
            className="w-full bg-gray-900 border border-gray-800 rounded-lg px-4 py-3 text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
            disabled={isRunning}
          />
          <div className="flex gap-4 items-stretch">
            <textarea
              value={jsonSchema}
              onChange={(e) => setJsonSchema(e.target.value)}
              placeholder="Target JSON Schema..."
              className="flex-1 bg-gray-900 border border-t-[4px] border-t-gray-800 border-gray-800 rounded-lg px-4 py-3 text-emerald-400 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 min-h-[140px]"
              disabled={isRunning}
            />
            <button
              type="submit"
              disabled={isRunning || !sourceData.trim() || !jsonSchema.trim()}
              className="bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white px-8 py-3 rounded-lg font-medium flex flex-col items-center justify-center gap-2 transition-colors shrink-0"
            >
              {isRunning ? <Loader2 className="animate-spin" size={24} /> : <Send size={24} />}
              {isRunning ? "Extracting..." : "Start Pipeline"}
            </button>
          </div>
        </form>

        <div className="flex-1 bg-gray-900 border border-gray-800 rounded-lg overflow-hidden flex flex-col font-mono text-sm leading-relaxed shadow-inner">
          <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-2 text-gray-400 text-xs uppercase tracking-wider shrink-0 flex justify-between">
            <span>Extraction Trace</span>
            {currentSessionId && (
              <span className="text-blue-400 font-mono opacity-60">
                ID: {currentSessionId.substring(0, 8)}
              </span>
            )}
          </div>
          <div className="flex-1 overflow-y-auto p-4 space-y-4">
            {logs.length === 0 && (
              <div className="text-gray-600 text-center mt-10 italic">
                Awaiting source target and schema...
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
                    <span className="text-blue-400 font-semibold mt-1 inline-block">
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
          query="Extract:"
          scopePrefix="Extract:"
          className="w-full"
        />
      </div>
    </div>
  );
};
