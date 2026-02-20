import React, { useState } from "react";
import { api } from "../api";
import { Loader2, Users } from "lucide-react";

interface AgentResponse {
  persona: string;
  response: string;
  loading: boolean;
  error: string | null;
}

const personas = [
  {
    name: "The Critic",
    prompt: "You are a harsh critic. Analyze the following and point out all flaws: ",
  },
  {
    name: "The Optimist",
    prompt: "You are an eternal optimist. Point out the best features and potential of: ",
  },
  {
    name: "The Engineer",
    prompt:
      "You are a pragmatic software engineer. Evaluate the technical feasibility and edge cases of: ",
  },
];

export const SwarmDashboard: React.FC = () => {
  const [query, setQuery] = useState("");
  const [isRunning, setIsRunning] = useState(false);
  const [agents, setAgents] = useState<AgentResponse[]>(
    personas.map((p) => ({ persona: p.name, response: "", loading: false, error: null }))
  );

  const handleStart = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim() || isRunning) return;

    setIsRunning(true);

    // Reset state
    setAgents(personas.map((p) => ({ persona: p.name, response: "", loading: true, error: null })));

    // Fan out requests to 3 distinct agent sessions in parallel
    await Promise.all(
      personas.map(async (persona, index) => {
        try {
          // 1. Create a dedicated session for this persona
          const sessionId = await api.createSession(`Swarm: ${persona.name}`);

          // 2. Start the run
          const fullPrompt = `${persona.prompt}\n\n${query}`;
          const { runId } = await api.startAsyncRun(sessionId, fullPrompt);

          // 3. Listen to the event stream
          const eventSource = new EventSource(api.getEventStreamUrl(sessionId, runId));

          eventSource.onmessage = (evt) => {
            const data = JSON.parse(evt.data);

            if (
              data.type === "message.part.updated" &&
              data.properties.part.type === "text" &&
              data.properties.delta
            ) {
              setAgents((prev) => {
                const updated = [...prev];
                updated[index].response += data.properties.delta;
                return updated;
              });
            } else if (
              data.type === "run.status.updated" &&
              (data.properties.status === "completed" || data.properties.status === "failed")
            ) {
              setAgents((prev) => {
                const updated = [...prev];
                updated[index].loading = false;
                return updated;
              });
              eventSource.close();
            }
          };

          eventSource.onerror = () => {
            setAgents((prev) => {
              const updated = [...prev];
              updated[index].loading = false;
              updated[index].error = "Stream disconnected";
              return updated;
            });
            eventSource.close();
          };
        } catch (err: any) {
          setAgents((prev) => {
            const updated = [...prev];
            updated[index].loading = false;
            updated[index].error = err.message || "Failed to start agent";
            return updated;
          });
        }
      })
    );

    setIsRunning(false);
  };

  return (
    <div className="flex flex-col h-full bg-gray-950 p-6">
      <div className="mb-6">
        <h2 className="text-2xl font-bold text-white flex items-center gap-2">
          <Users className="text-purple-500" />
          Parallel Agent Swarm
        </h2>
        <p className="text-gray-400 mt-1">
          Submit an idea. Watch three distinct AI personas evaluate it concurrently.
        </p>
      </div>

      <form onSubmit={handleStart} className="flex gap-4 mb-6">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="E.g., A mobile app that reminds you to drink water by locking your screen."
          className="flex-1 bg-gray-900 border border-gray-800 rounded-lg px-4 py-3 text-white focus:outline-none focus:ring-2 focus:ring-purple-500"
          disabled={isRunning}
        />
        <button
          type="submit"
          disabled={isRunning || !query.trim()}
          className="bg-purple-600 hover:bg-purple-700 disabled:opacity-50 text-white px-6 py-3 rounded-lg font-medium flex items-center gap-2 transition-colors"
        >
          {isRunning ? <Loader2 className="animate-spin" size={20} /> : <Users size={20} />}
          {isRunning ? "Deploying Swarm..." : "Run Swarm Review"}
        </button>
      </form>

      <div className="flex-1 grid grid-cols-1 md:grid-cols-3 gap-6">
        {agents.map((agent, i) => (
          <div
            key={i}
            className="bg-gray-900 border border-gray-800 rounded-lg flex flex-col shadow-inner"
          >
            <div className="bg-gray-800/50 border-b border-gray-800 px-4 py-3 flex items-center justify-between">
              <span className="font-semibold text-gray-200">{agent.persona}</span>
              {agent.loading && <Loader2 size={16} className="text-purple-500 animate-spin" />}
            </div>
            <div className="flex-1 p-4 overflow-y-auto text-sm text-gray-300 leading-relaxed whitespace-pre-wrap">
              {agent.response ||
                (agent.loading ? (
                  <span className="text-gray-600 italic">Thinking...</span>
                ) : (
                  <span className="text-gray-600 italic">Waiting for input.</span>
                ))}
              {agent.error && <p className="text-red-400 mt-2">{agent.error}</p>}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};
