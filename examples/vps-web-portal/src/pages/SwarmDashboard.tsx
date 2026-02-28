import React, { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { Loader2, Users } from "lucide-react";
import { handleCommonRunEvent } from "../utils/liveEventDebug";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSanitize from "rehype-sanitize";

interface AgentResponse {
  persona: string;
  response: string;
  loading: boolean;
  error: string | null;
  logs: string[];
}

const personas = [
  {
    name: "The Critic",
    prompt:
      "You are a rigorous critic. Identify failure modes, blind spots, and hidden costs. End with a risk score (1-10): ",
  },
  {
    name: "The Optimist",
    prompt:
      "You are an ambitious optimist. Identify upside, leverage points, and breakout potential. End with an upside score (1-10): ",
  },
  {
    name: "The Engineer",
    prompt:
      "You are a pragmatic software engineer. Evaluate technical feasibility, architecture choices, and operational risks. End with a buildability score (1-10): ",
  },
];

const SWARM_SESSION_KEY = "tandem_portal_swarm_sessions";

export const SwarmDashboard: React.FC = () => {
  const [query, setQuery] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const logContainerRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const [agents, setAgents] = useState<AgentResponse[]>(
    personas.map((p) => ({
      persona: p.name,
      response: "",
      loading: false,
      error: null,
      logs: ["Idle. Waiting for run."],
    }))
  );
  const appendAgentLog = (index: number, message: string) => {
    const stamp = new Date().toLocaleTimeString();
    setAgents((prev) => {
      const updated = [...prev];
      const current = updated[index];
      if (!current) return prev;
      updated[index] = {
        ...current,
        logs: [...current.logs, `${stamp} | ${message}`].slice(-40),
      };
      return updated;
    });
  };

  const loadAgentResponse = async (personaName: string, sessionId: string) => {
    try {
      const messages = await api.getSessionMessages(sessionId);
      const lastAssistant = [...messages].reverse().find((m) => m.info?.role === "assistant");
      const finalText = (lastAssistant?.parts || [])
        .filter((p) => p.type === "text" && p.text)
        .map((p) => p.text)
        .join("\n")
        .trim();
      if (finalText) {
        setAgents((prev) => {
          const updated = [...prev];
          const idx = personas.findIndex((p) => p.name === personaName);
          if (idx >= 0) {
            updated[idx].response = finalText;
            updated[idx].loading = false;
            updated[idx].error = null;
            updated[idx].logs = [
              ...updated[idx].logs,
              `${new Date().toLocaleTimeString()} | Loaded final response (${finalText.length} chars)`,
            ].slice(-40);
          }
          return updated;
        });
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setAgents((prev) => {
        const updated = [...prev];
        const idx = personas.findIndex((p) => p.name === personaName);
        if (idx >= 0) {
          updated[idx].loading = false;
          updated[idx].error = errorMessage || "Failed to load saved session";
        }
        return updated;
      });
    }
  };

  useEffect(() => {
    const raw = localStorage.getItem(SWARM_SESSION_KEY);
    if (!raw) return;
    try {
      const parsed = JSON.parse(raw) as Record<string, string>;
      void Promise.all(
        Object.entries(parsed).map(([personaName, sid]) => loadAgentResponse(personaName, sid))
      );
    } catch (err) {
      console.error("Failed to parse swarm session map", err);
    }
  }, []);

  useEffect(() => {
    const id = window.requestAnimationFrame(() => {
      agents.forEach((agent) => {
        const el = logContainerRefs.current[agent.persona];
        if (!el) return;
        el.scrollTop = el.scrollHeight;
      });
    });
    return () => window.cancelAnimationFrame(id);
  }, [agents]);

  const handleStart = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim() || isRunning) return;

    setIsRunning(true);

    // Reset state
    setAgents(
      personas.map((p) => ({
        persona: p.name,
        response: "",
        loading: true,
        error: null,
        logs: [`${new Date().toLocaleTimeString()} | Run queued`],
      }))
    );

    // Fan out requests to 3 distinct agent sessions in parallel
    await Promise.all(
      personas.map(async (persona, index) => {
        try {
          // 1. Create a dedicated session for this persona
          const sessionId = await api.createSession(`Swarm: ${persona.name}`);
          appendAgentLog(index, `Session created: ${sessionId.slice(0, 8)}...`);
          const raw = localStorage.getItem(SWARM_SESSION_KEY);
          const currentMap = raw ? (JSON.parse(raw) as Record<string, string>) : {};
          currentMap[persona.name] = sessionId;
          localStorage.setItem(SWARM_SESSION_KEY, JSON.stringify(currentMap));

          // 2. Start the run
          const fullPrompt = `${persona.prompt}\n\n${query}`;
          const { runId } = await api.startAsyncRun(sessionId, fullPrompt);
          appendAgentLog(index, `Run started: ${runId.slice(0, 8)}...`);

          // 3. Listen to the event stream
          const eventSource = new EventSource(api.getEventStreamUrl(sessionId, runId));
          let finalized = false;
          let sawRunEvent = false;
          let sawFirstDelta = false;
          const startedAt = Date.now();
          const watchdog = window.setTimeout(async () => {
            if (finalized || sawRunEvent) return;
            try {
              const runState = await api.getActiveRun(sessionId);
              if (!runState?.active) {
                appendAgentLog(index, "Run inactive before live events arrived.");
                setAgents((prev) => {
                  const updated = [...prev];
                  updated[index].loading = false;
                  if (!updated[index].response) {
                    updated[index].error =
                      "Run ended before live events arrived. Check provider key/model and logs.";
                  }
                  return updated;
                });
                void finalizeAgent();
                return;
              }
              setAgents((prev) => {
                const updated = [...prev];
                if (!updated[index].response) {
                  updated[index].response = "[Run active, waiting for live deltas...]";
                }
                return updated;
              });
              appendAgentLog(index, "Run active, waiting for first delta.");
            } catch {
              appendAgentLog(index, "No live events yet; run state poll failed.");
              setAgents((prev) => {
                const updated = [...prev];
                updated[index].error = "No live events yet and failed to query run state.";
                return updated;
              });
            }
          }, 4000);
          const runStatePoll = window.setInterval(async () => {
            if (finalized) return;
            try {
              const runState = await api.getActiveRun(sessionId);
              if (!runState?.active) {
                appendAgentLog(index, "Run became inactive during poll.");
                setAgents((prev) => {
                  const updated = [...prev];
                  updated[index].loading = false;
                  if (!updated[index].response) {
                    updated[index].error = "Run became inactive before a terminal stream event.";
                  }
                  return updated;
                });
                void finalizeAgent();
              }
            } catch {
              // Non-fatal polling failure while stream remains attached.
            }
          }, 5000);

          const finalizeAgent = async () => {
            if (finalized) return;
            finalized = true;
            window.clearTimeout(watchdog);
            window.clearInterval(runStatePoll);
            appendAgentLog(index, `Finalizing run (${Date.now() - startedAt}ms elapsed)`);
            await loadAgentResponse(persona.name, sessionId);
            eventSource.close();
          };

          eventSource.onopen = () => {
            appendAgentLog(index, "SSE stream connected.");
          };

          eventSource.onmessage = (evt) => {
            try {
              const data = JSON.parse(evt.data);
              if (data.type !== "server.connected" && data.type !== "engine.lifecycle.ready") {
                sawRunEvent = true;
              }

              const handledCommon = handleCommonRunEvent(
                data,
                ({ content }) => {
                  appendAgentLog(index, `System: ${content}`);
                  if (/engine error/i.test(content)) {
                    setAgents((prev) => {
                      const updated = [...prev];
                      updated[index].loading = false;
                      updated[index].error = content;
                      return updated;
                    });
                  }
                },
                (status) => {
                  appendAgentLog(index, `Run status: ${status}`);
                  if (
                    status === "completed" ||
                    status === "failed" ||
                    status === "error" ||
                    status === "cancelled" ||
                    status === "canceled" ||
                    status === "timeout" ||
                    status === "timed_out"
                  ) {
                    void finalizeAgent();
                  }
                }
              );
              if (handledCommon) return;

              if (
                data.type === "message.part.updated" &&
                data.properties?.part?.type === "text" &&
                data.properties?.delta
              ) {
                if (!sawFirstDelta) {
                  sawFirstDelta = true;
                  appendAgentLog(index, `First delta received (${Date.now() - startedAt}ms).`);
                }
                setAgents((prev) => {
                  const updated = [...prev];
                  updated[index].response += data.properties.delta;
                  return updated;
                });
              } else if (
                data.type === "run.status.updated" &&
                (data.properties?.status === "completed" || data.properties?.status === "failed")
              ) {
                appendAgentLog(index, `run.status.updated: ${String(data.properties?.status)}`);
                void finalizeAgent();
              } else if (
                data.type === "session.run.finished" &&
                (data.properties?.status === "completed" || data.properties?.status === "failed")
              ) {
                appendAgentLog(index, `session.run.finished: ${String(data.properties?.status)}`);
                void finalizeAgent();
              } else if (data.type === "session.error") {
                appendAgentLog(
                  index,
                  `session.error: ${String(data.properties?.error?.message || "unknown error")}`
                );
                setAgents((prev) => {
                  const updated = [...prev];
                  updated[index].loading = false;
                  updated[index].error =
                    data.properties?.error?.message || "Engine error during swarm run.";
                  return updated;
                });
                void finalizeAgent();
              }
            } catch {
              appendAgentLog(index, "Failed to parse stream event payload.");
              setAgents((prev) => {
                const updated = [...prev];
                updated[index].loading = false;
                updated[index].error = "Failed to parse stream event payload.";
                return updated;
              });
              void finalizeAgent();
            }
          };

          eventSource.onerror = () => {
            window.clearTimeout(watchdog);
            window.clearInterval(runStatePoll);
            appendAgentLog(index, "SSE stream disconnected.");
            setAgents((prev) => {
              const updated = [...prev];
              updated[index].loading = false;
              updated[index].error = "Stream disconnected";
              return updated;
            });
            eventSource.close();
          };
        } catch (err) {
          const errorMessage = err instanceof Error ? err.message : String(err);
          appendAgentLog(index, `Failed to start agent: ${errorMessage || "unknown error"}`);
          setAgents((prev) => {
            const updated = [...prev];
            updated[index].loading = false;
            updated[index].error = errorMessage || "Failed to start agent";
            return updated;
          });
        }
      })
    );

    setIsRunning(false);
  };

  return (
    <div className="flex flex-col h-full bg-transparent p-4 sm:p-6 lg:p-8 max-w-7xl mx-auto w-full">
      <div className="mb-8 mt-2 lg:mt-0">
        <h2 className="text-3xl font-bold text-white flex items-center gap-3 tracking-tight">
          <div className="p-2 bg-purple-500/20 rounded-xl border border-purple-500/30 shadow-[0_0_20px_rgba(168,85,247,0.4)]">
            <Users className="text-purple-400" size={24} />
          </div>
          Parallel Agent Swarm
        </h2>
        <p className="text-gray-400 mt-2 text-sm sm:text-base max-w-2xl leading-relaxed">
          Submit an idea. Watch three distinct AI personas evaluate it concurrently using shared
          context and execution streams.
        </p>
      </div>

      <form onSubmit={handleStart} className="flex flex-col gap-3 sm:flex-row sm:gap-4 mb-8">
        <div className="flex-1 relative group">
          <div className="absolute inset-0 bg-gradient-to-r from-purple-500/20 to-blue-500/20 rounded-2xl blur-lg transition-opacity opacity-0 group-focus-within:opacity-100"></div>
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="E.g., Build an AI incident co-pilot that reads logs and proposes fixes..."
            className="relative w-full bg-gray-950/60 backdrop-blur-md border border-white/10 rounded-2xl px-5 py-4 text-white font-medium placeholder:text-gray-500 focus:outline-none focus:border-purple-500/50 focus:ring-1 focus:ring-purple-500/50 transition-all shadow-inner"
            disabled={isRunning}
          />
        </div>
        <button
          type="submit"
          disabled={isRunning || !query.trim()}
          className="relative overflow-hidden group bg-gray-900 border border-white/10 hover:border-purple-500/50 disabled:opacity-50 disabled:hover:border-white/10 text-white px-8 py-4 rounded-2xl font-medium flex items-center justify-center gap-3 transition-all sm:w-auto shadow-lg"
        >
          {isRunning ? (
            <div className="absolute inset-0 bg-purple-600/20 animate-pulse"></div>
          ) : (
            <div className="absolute inset-0 bg-gradient-to-r from-purple-600/80 to-blue-600/80 opacity-0 group-hover:opacity-100 transition-opacity duration-300"></div>
          )}
          <span className="relative z-10 flex items-center gap-2">
            {isRunning ? (
              <Loader2 className="animate-spin text-purple-400" size={20} />
            ) : (
              <Users size={20} />
            )}
            {isRunning ? "Deploying Swarm..." : "Run Swarm Review"}
          </span>
        </button>
      </form>

      <div className="flex-1 grid grid-cols-1 md:grid-cols-3 gap-6 pb-6">
        {agents.map((agent, i) => (
          <div
            key={i}
            className="group relative bg-gray-950/40 backdrop-blur-xl border border-white/5 rounded-3xl flex flex-col shadow-2xl overflow-hidden transition-all duration-300 hover:border-white/10"
          >
            {agent.loading && (
              <div className="absolute top-0 left-0 right-0 h-1 bg-gradient-to-r from-purple-500/0 via-purple-500 to-purple-500/0 animate-[pulse_2s_ease-in-out_infinite]"></div>
            )}
            <div className="bg-white/[0.02] border-b border-white/5 px-5 py-4 flex items-center justify-between">
              <span className="font-semibold tracking-wide text-white flex items-center gap-2">
                <span className="w-2 h-2 rounded-full bg-purple-500 shadow-[0_0_8px_rgba(168,85,247,0.8)]"></span>
                {agent.persona}
              </span>
              {agent.loading && <Loader2 size={16} className="text-purple-400 animate-spin" />}
            </div>
            <div className="border-b border-white/5 bg-black/40 px-5 py-3 flex-shrink-0">
              <div className="text-[10px] uppercase tracking-widest font-semibold text-gray-500 mb-2 flex items-center gap-2">
                Live Execution Log
                <div className="h-px flex-1 bg-gradient-to-r from-white/10 to-transparent"></div>
              </div>
              <div
                ref={(el) => {
                  logContainerRefs.current[agent.persona] = el;
                }}
                className="max-h-24 min-h-[6rem] overflow-y-auto custom-scrollbar rounded-xl border border-white/5 bg-gray-950/80 px-3 py-2 text-[11px] font-mono text-gray-400"
              >
                {agent.logs.length > 0 ? (
                  agent.logs.map((line, idx) => (
                    <div
                      key={`${agent.persona}-log-${idx}`}
                      className="truncate py-0.5 opacity-80 hover:opacity-100 transition-opacity"
                    >
                      <span className="text-gray-600 mr-2">{line.split("|")[0]}</span>
                      <span className="text-emerald-400">{line.split("|")[1]}</span>
                    </div>
                  ))
                ) : (
                  <div className="animate-pulse">Waiting for execution stream...</div>
                )}
              </div>
            </div>
            <div className="flex-1 p-5 overflow-y-auto custom-scrollbar text-sm text-gray-300 leading-relaxed bg-gradient-to-b from-white/[0.01] to-transparent">
              {agent.response ? (
                <div className="prose prose-invert prose-sm max-w-none prose-p:leading-relaxed prose-pre:bg-black/40 prose-pre:border prose-pre:border-white/10 prose-pre:rounded-xl prose-a:text-purple-400 hover:prose-a:text-purple-300">
                  <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]}>
                    {agent.response}
                  </ReactMarkdown>
                </div>
              ) : agent.loading ? (
                <div className="flex items-center gap-3 text-gray-500 italic h-full justify-center">
                  <Loader2 size={16} className="animate-spin" /> Synthesizing...
                </div>
              ) : (
                <div className="text-gray-600 italic h-full flex items-center justify-center text-center px-4">
                  Awaiting input parameter injection.
                </div>
              )}
              {agent.error && (
                <div className="mt-4 p-3 bg-red-500/10 border border-red-500/20 rounded-xl text-red-400 text-xs flex gap-2 items-start">
                  <div className="text-red-500 font-bold mt-0.5">!</div>
                  <p>{agent.error}</p>
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};
