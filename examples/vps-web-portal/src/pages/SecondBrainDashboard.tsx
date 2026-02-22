import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { BrainCircuit, Send, FileCode2, Database, FolderGit2 } from "lucide-react";

interface ChatMessage {
  id: string;
  role: "user" | "agent";
  content: string;
}

const SECOND_BRAIN_SESSION_KEY = "tandem_portal_second_brain_session_id";
const RUN_TIMEOUT_MS = 45000;

const toChatMessages = (
  messages: Awaited<ReturnType<typeof api.getSessionMessages>>
): ChatMessage[] => {
  return messages
    .filter((m) => m.info?.role === "user" || m.info?.role === "assistant")
    .map((m) => {
      const role: ChatMessage["role"] = m.info?.role === "assistant" ? "agent" : "user";
      const content = (m.parts || [])
        .filter((p) => p.type === "text" && p.text)
        .map((p) => p.text)
        .join("\n")
        .trim();
      return {
        id: Math.random().toString(),
        role,
        content,
      };
    })
    .filter((m) => m.content.length > 0);
};

export const SecondBrainDashboard: React.FC = () => {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [isThinking, setIsThinking] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, isThinking]);

  useEffect(() => {
    const initBrain = async () => {
      try {
        const existingSessionId = localStorage.getItem(SECOND_BRAIN_SESSION_KEY);
        if (existingSessionId) {
          const history = await api.getSessionMessages(existingSessionId);
          const restored = toChatMessages(history);
          setSessionId(existingSessionId);
          if (restored.length > 0) {
            setMessages(restored);
            return;
          }
        }

        const sid = await api.createSession("Second Brain MVP");
        localStorage.setItem(SECOND_BRAIN_SESSION_KEY, sid);
        setSessionId(sid);
        // System prompt sets up the MCP expectation for new sessions only.
        const prompt = `You are a Second Brain AI assistant. You have access to local the server's workspace and tools via MCP (Model Context Protocol). You can search files, read local databases, and help the user synthesize information from their local filesystem.`;
        await api.sendMessage(sid, prompt);
        setMessages([
          {
            id: "welcome",
            role: "agent",
            content:
              "Hello! I am connected to the local headless engine. I can use MCP tools to browse files, run commands, or interact with databases. What would you like to explore?",
          },
        ]);
      } catch (err: any) {
        setMessages([
          {
            id: "err",
            role: "agent",
            content: `CRITICAL ERROR: Failed to connect to engine. ${err.message}`,
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
      { id: Math.random().toString(), role: "user", content: userMsg },
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
          const restored = toChatMessages(history);
          if (restored.length > 0) {
            setMessages(restored);
          }
          if (reason === "timeout") {
            setMessages((prev) => [
              ...prev,
              {
                id: Math.random().toString(),
                role: "agent",
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

        if (
          data.type === "message.part.updated" &&
          data.properties.part.type === "text" &&
          data.properties.delta
        ) {
          setMessages((prev) => {
            const updated = [...prev];
            const last = updated[updated.length - 1];
            if (last && last.role === "agent" && last.id !== "welcome" && last.id !== "err") {
              last.content += data.properties.delta;
            } else {
              updated.push({
                id: Math.random().toString(),
                role: "agent",
                content: data.properties.delta,
              });
            }
            return updated;
          });
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
    } catch (err: any) {
      setMessages((prev) => [
        ...prev,
        { id: Math.random().toString(), role: "agent", content: `[Error: ${err.message}]` },
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
          content: "Session reset. Ask me to inspect files, git, or sqlite on this server.",
        },
      ]);
    } catch (err: any) {
      setMessages([
        {
          id: "err-reset",
          role: "agent",
          content: `Failed to reset session: ${err.message}`,
        },
      ]);
    }
  };

  return (
    <div className="flex flex-col h-full bg-gray-950 text-white">
      {/* Header */}
      <div className="bg-gray-900 border-b border-gray-800 p-6">
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
          <div key={m.id} className={`flex ${m.role === "user" ? "justify-end" : "justify-start"}`}>
            <div
              className={`max-w-[75%] rounded-2xl px-5 py-3 ${
                m.role === "user"
                  ? "bg-indigo-600 text-white shadow-md"
                  : "bg-gray-800 text-gray-200 border border-gray-700"
              }`}
            >
              <p className="whitespace-pre-wrap leading-relaxed">{m.content}</p>
            </div>
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
      <div className="p-4 bg-gray-900 border-t border-gray-800">
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
  );
};
