import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { BrainCircuit, Send, FileCode2, Database, FolderGit2, Loader2 } from "lucide-react";
import { SessionHistory } from "../components/SessionHistory";
import { ToolCallResult } from "../components/ToolCallResult";

interface ChatEvent {
  id: string;
  role: "user" | "agent" | "system";
  type: "text" | "tool_start" | "tool_end";
  content: string;
  toolName?: string;
  toolResult?: string;
}

const SECOND_BRAIN_SESSION_KEY = "tandem_portal_second_brain_session_id";
const RUN_TIMEOUT_MS = 45000;

const buildChatEvents = (
  messages: Awaited<ReturnType<typeof api.getSessionMessages>>
): ChatEvent[] => {
  return messages
    .filter((m) => m.info?.role === "user" || m.info?.role === "assistant")
    .flatMap((m) => {
      const events: ChatEvent[] = [];
      const role = m.info?.role === "assistant" ? "agent" : "user";

      const text = (m.parts || [])
        .filter((p) => p.type === "text" && p.text)
        .map((p) => p.text)
        .join("\n")
        .trim();

      if (text) {
        events.push({
          id: Math.random().toString(),
          role,
          type: "text",
          content: text,
        });
      }
      return events;
    });
};

export const SecondBrainDashboard: React.FC = () => {
  const [messages, setMessages] = useState<ChatEvent[]>([]);
  const [input, setInput] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [isThinking, setIsThinking] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, isThinking]);

  const loadSession = async (sid: string) => {
    if (!sid) {
      setMessages([]);
      setSessionId(null);
      localStorage.removeItem(SECOND_BRAIN_SESSION_KEY);
      return;
    }

    try {
      setSessionId(sid);
      localStorage.setItem(SECOND_BRAIN_SESSION_KEY, sid);
      const history = await api.getSessionMessages(sid);
      const restored = buildChatEvents(history);

      setMessages([
        {
          id: "sys-restored",
          role: "system",
          type: "text",
          content: `Restored session ${sid.substring(0, 8)}`,
        },
        ...restored,
      ]);
    } catch (err) {
      console.error("Failed to restore session", err);
      setSessionId(null);
      localStorage.removeItem(SECOND_BRAIN_SESSION_KEY);
    }
  };

  useEffect(() => {
    const initBrain = async () => {
      try {
        const existingSessionId = localStorage.getItem(SECOND_BRAIN_SESSION_KEY);
        if (existingSessionId) {
          await loadSession(existingSessionId);
          return;
        }

        const sid = await api.createSession("Second Brain MVP");
        localStorage.setItem(SECOND_BRAIN_SESSION_KEY, sid);
        setSessionId(sid);
        // System prompt sets up the MCP expectation for new sessions only.
        const prompt = `You are a Second Brain AI assistant. You have access to the local server's workspace via MCP tools and memory_store capabilities. When users ask you to learn a folder, use the memory_store tool to index local files, and output a summary into 'out/index_stats.json'. When answering questions, write your detailed answer to 'out/answers.md' alongside citing the file paths in your chat response.`;
        await api.sendMessage(sid, prompt);
        setMessages([
          {
            id: "welcome",
            role: "agent",
            type: "text",
            content:
              "Hello! I am connected to the local headless engine. I can use MCP tools to browse files, run commands, or interact with databases. What would you like to explore?",
          },
        ]);
      } catch (err) {
        const errorMessage = err instanceof Error ? err.message : String(err);
        setMessages([
          {
            id: "err",
            role: "system",
            type: "text",
            content: `CRITICAL ERROR: Failed to connect to engine. ${errorMessage}`,
          },
        ]);
      }
    };
    initBrain();
  }, []);

  const handleSend = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim() || isThinking || !sessionId) return;

    const userMsg = input.trim();
    setInput("");
    setMessages((prev) => [
      ...prev,
      { id: Math.random().toString(), role: "user", type: "text", content: userMsg },
    ]);
    setIsThinking(true);

    try {
      const { runId } = await api.startAsyncRun(sessionId, userMsg);
      const source = new EventSource(api.getEventStreamUrl(sessionId, runId));
      eventSourceRef.current = source;
      let finalized = false;
      let timeoutHandle: ReturnType<typeof setTimeout> | null = null;

      const finalize = async (reason: "completed" | "failed" | "timeout" | "stream_error") => {
        if (finalized) return;
        finalized = true;
        if (timeoutHandle) {
          clearTimeout(timeoutHandle);
          timeoutHandle = null;
        }
        try {
          const history = await api.getSessionMessages(sessionId);
          const restored = buildChatEvents(history);
          if (restored.length > 0) {
            setMessages(restored);
          }
          if (reason === "timeout") {
            setMessages((prev) => [
              ...prev,
              {
                id: Math.random().toString(),
                role: "agent",
                type: "text",
                content:
                  "[Run timed out in UI. Loaded latest saved session history so you can continue.]",
              },
            ]);
          } else if (reason === "stream_error") {
            setMessages((prev) => [
              ...prev,
              {
                id: Math.random().toString(),
                role: "agent",
                type: "text",
                content: "[Stream disconnected. Loaded latest saved session history.]",
              },
            ]);
          }
        } catch (err) {
          console.error("Failed to load session history after run", err);
        } finally {
          setIsThinking(false);
          source.close();
          if (eventSourceRef.current === source) {
            eventSourceRef.current = null;
          }
        }
      };

      timeoutHandle = setTimeout(() => {
        void finalize("timeout");
      }, RUN_TIMEOUT_MS);

      source.onmessage = (evt) => {
        const data = JSON.parse(evt.data);

        if (data.type === "message.part.updated") {
          const part = data.properties.part;

          if (part.type === "tool") {
            if (part.state.status === "running") {
              setMessages((prev) => [
                ...prev,
                {
                  id: Math.random().toString(),
                  role: "agent",
                  type: "tool_start",
                  content: `Using tool: ${part.tool}`,
                  toolName: part.tool,
                },
              ]);
            } else if (part.state.status === "completed") {
              let rString = "";
              if (part.state.result) {
                rString =
                  typeof part.state.result === "string"
                    ? part.state.result
                    : JSON.stringify(part.state.result);
              }

              setMessages((prev) => {
                const updated = [...prev];
                let lastStartIdx = -1;
                for (let i = prev.length - 1; i >= 0; i--) {
                  if (prev[i].type === "tool_start" && prev[i].toolName === part.tool) {
                    lastStartIdx = i;
                    break;
                  }
                }

                if (lastStartIdx !== -1) {
                  updated[lastStartIdx] = {
                    ...updated[lastStartIdx],
                    type: "tool_end",
                    content: `Tool completed: ${part.tool}`,
                    toolResult: rString,
                  };
                  return updated;
                }
                return prev;
              });
            }
          } else if (part.type === "text" && data.properties.delta) {
            setMessages((prev) => {
              const updated = [...prev];
              const last = updated[updated.length - 1];
              if (
                last &&
                last.role === "agent" &&
                last.type === "text" &&
                last.id !== "welcome" &&
                last.id !== "err"
              ) {
                last.content += data.properties.delta;
              } else {
                updated.push({
                  id: Math.random().toString(),
                  role: "agent",
                  type: "text",
                  content: data.properties.delta,
                });
              }
              return updated;
            });
          }
        } else if (
          data.type === "run.status.updated" &&
          (data.properties.status === "completed" || data.properties.status === "failed")
        ) {
          void finalize(data.properties.status === "failed" ? "failed" : "completed");
        } else if (
          data.type === "session.run.finished" &&
          (data.properties?.status === "completed" || data.properties?.status === "failed")
        ) {
          void finalize(data.properties.status === "failed" ? "failed" : "completed");
        }
      };
      source.onerror = () => {
        void finalize("stream_error");
      };
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setMessages((prev) => [
        ...prev,
        {
          id: Math.random().toString(),
          role: "agent",
          type: "text",
          content: `[Error: ${errorMessage}]`,
        },
      ]);
      setIsThinking(false);
    }
  };

  const resetSession = async () => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    localStorage.removeItem(SECOND_BRAIN_SESSION_KEY);
    setMessages([]);
    setSessionId(null);
    setIsThinking(false);
    try {
      const sid = await api.createSession("Second Brain MVP");
      localStorage.setItem(SECOND_BRAIN_SESSION_KEY, sid);
      setSessionId(sid);
      setMessages([
        {
          id: "welcome",
          role: "agent",
          type: "text",
          content: "Session reset. Ask me to inspect files, git, or sqlite on this server.",
        },
      ]);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setMessages([
        {
          id: "err-reset",
          role: "agent",
          type: "text",
          content: `Failed to reset session: ${errorMessage}`,
        },
      ]);
    }
  };

  return (
    <div className="flex h-full bg-gray-950 text-white">
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Header */}
        <div className="bg-gray-900 border-b border-gray-800 p-6 shrink-0">
          <div className="flex items-center justify-between">
            <h2 className="text-2xl font-bold flex items-center gap-2 text-indigo-400">
              <BrainCircuit />
              Unified Local Brain
            </h2>
            <button
              type="button"
              onClick={resetSession}
              className="px-3 py-1.5 text-xs border border-gray-700 rounded text-gray-300 hover:text-white hover:bg-gray-800"
            >
              Reset Session
            </button>
          </div>
          <p className="text-gray-400 mt-2 text-sm flex items-center gap-4">
            <span>
              <FileCode2 className="inline mr-1" size={14} /> Local Files Access
            </span>
            <span>
              <Database className="inline mr-1" size={14} /> SQLite Queries
            </span>
            <span>
              <FolderGit2 className="inline mr-1" size={14} /> Git Sync
            </span>
          </p>
          <p className="text-gray-500 mt-2 text-xs">
            Demonstrates MCP integration running on the local VPS engine daemon.
          </p>
        </div>

        {/* Chat Area */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          {messages.map((m) => (
            <div
              key={m.id}
              className={`flex ${m.role === "user" ? "justify-end" : "justify-start"}`}
            >
              {m.type === "text" && (
                <div
                  className={`max-w-[75%] rounded-2xl px-5 py-3 ${
                    m.role === "user"
                      ? "bg-indigo-600 text-white shadow-md"
                      : "bg-gray-800 text-gray-200 border border-gray-700"
                  }`}
                >
                  <p className="whitespace-pre-wrap leading-relaxed">{m.content}</p>
                </div>
              )}
              {m.type === "tool_start" && (
                <div className="max-w-[75%] rounded-lg px-3 py-2 text-yellow-500 flex items-center gap-2 bg-gray-800/50 border border-yellow-900/30">
                  <Loader2 size={16} className="animate-spin" /> {m.content}
                </div>
              )}
              {m.type === "tool_end" && m.toolResult && (
                <div className="max-w-[85%]">
                  <ToolCallResult
                    toolName={m.toolName || "tool"}
                    resultString={m.toolResult}
                    defaultExpanded={false}
                  />
                </div>
              )}
            </div>
          ))}
          {isThinking && (
            <div className="flex justify-start">
              <div className="bg-gray-800 border border-gray-700 rounded-2xl px-5 py-3 flex gap-1 items-center">
                <div
                  className="w-2 h-2 bg-gray-500 rounded-full animate-bounce"
                  style={{ animationDelay: "0ms" }}
                ></div>
                <div
                  className="w-2 h-2 bg-gray-500 rounded-full animate-bounce"
                  style={{ animationDelay: "150ms" }}
                ></div>
                <div
                  className="w-2 h-2 bg-gray-500 rounded-full animate-bounce"
                  style={{ animationDelay: "300ms" }}
                ></div>
              </div>
            </div>
          )}
          <div ref={messagesEndRef} />
        </div>

        {/* Input Bar */}
        <div className="p-4 bg-gray-900 border-t border-gray-800 shrink-0">
          <form onSubmit={handleSend} className="max-w-4xl mx-auto relative flex items-center">
            <input
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder="Ask the engine to read a local file or query a database via MCP..."
              className="w-full bg-gray-800 border border-gray-700 text-white rounded-full pl-6 pr-12 py-3 focus:outline-none focus:ring-2 focus:ring-indigo-500 shadow-inner"
              disabled={isThinking || !sessionId}
            />
            <button
              type="submit"
              disabled={!input.trim() || isThinking || !sessionId}
              className="absolute right-2 p-2 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white rounded-full transition-colors flex items-center justify-center"
            >
              <Send size={18} />
            </button>
          </form>
        </div>
      </div>

      {/* Sidebar right for Session History */}
      <div className="w-80 shrink-0 border-l border-gray-800 bg-gray-900 overflow-y-auto">
        <SessionHistory
          currentSessionId={sessionId}
          onSelectSession={loadSession}
          className="w-full"
        />
      </div>
    </div>
  );
};
