import React, { useState, useEffect, useRef } from "react";
import { api } from "../api";
import { MessageSquareQuote, ChevronRight, RotateCcw } from "lucide-react";

interface GameEvent {
  id: string;
  type: "text" | "choice" | "system" | "hero";
  content: string;
  options?: string[];
  questionId?: string;
}

const ADVENTURE_SESSION_KEY = "tandem_portal_adventure_session_id";

const buildAdventureEventsFromMessages = (
  messages: Awaited<ReturnType<typeof api.getSessionMessages>>
): GameEvent[] => {
  const mapped: GameEvent[] = messages.flatMap((m): GameEvent[] => {
    const role = m.info?.role;
    if (role !== "user" && role !== "assistant") return [];
    const text = (m.parts || [])
      .filter((p) => p.type === "text" && p.text)
      .map((p) => p.text)
      .join("\n")
      .trim();
    if (!text) return [];
    if (role === "assistant") {
      return [{ id: Math.random().toString(36).substring(7), type: "text", content: text }];
    }
    return [{ id: Math.random().toString(36).substring(7), type: "hero", content: `> ${text}` }];
  });
  return mapped;
};

export const TextAdventure: React.FC = () => {
  const [events, setEvents] = useState<GameEvent[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [hasStarted, setHasStarted] = useState(false);
  const [manualInput, setManualInput] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const eventSourceRef = useRef<EventSource | null>(null);

  // Auto-scroll logic
  useEffect(() => {
    scrollRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [events]);

  useEffect(() => {
    const restore = async () => {
      const savedSessionId = localStorage.getItem(ADVENTURE_SESSION_KEY);
      if (!savedSessionId) return;
      try {
        const messages = await api.getSessionMessages(savedSessionId);
        const restoredEvents = buildAdventureEventsFromMessages(messages);
        if (restoredEvents.length > 0) {
          setSessionId(savedSessionId);
          setHasStarted(true);
          setEvents(restoredEvents);
          maybeAddChoiceFromLastAssistant(restoredEvents);
        }
      } catch (err) {
        console.error("Failed to restore adventure session", err);
      }
    };
    void restore();
  }, []);

  const addEvent = (evt: Omit<GameEvent, "id">) => {
    setEvents((prev) => [...prev, { ...evt, id: Math.random().toString(36).substring(7) }]);
  };

  const parseChoicesFromText = (text: string): string[] => {
    const lines = text
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const numbered = lines
      .map((line) => line.match(/^\d+[.)]\s+(.+)$/)?.[1]?.trim())
      .filter((v): v is string => !!v);
    return numbered.slice(0, 6);
  };

  const maybeAddChoiceFromLastAssistant = (
    restoredEvents: GameEvent[],
    fallbackPrompt = "Choose your next action:"
  ) => {
    const lastText = [...restoredEvents].reverse().find((e) => e.type === "text");
    if (!lastText) return;
    const options = parseChoicesFromText(lastText.content);
    if (options.length < 2) return;
    const alreadyExists = restoredEvents.some(
      (e) => e.type === "choice" && e.content === fallbackPrompt
    );
    if (alreadyExists) return;
    addEvent({
      type: "choice",
      content: fallbackPrompt,
      options,
    });
  };

  const startGame = async () => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    setIsLoading(true);
    setEvents([]);
    setSessionId(null);
    setManualInput("");
    addEvent({ type: "system", content: "INITIALIZING RPG SERVER CONNECTION..." });

    try {
      const sid = await api.createSession("RPG Game Master");
      setSessionId(sid);
      localStorage.setItem(ADVENTURE_SESSION_KEY, sid);
      setHasStarted(true);

      const prompt = `You are a text-based RPG Game Master.
The player has just woken up in a dark, mysterious forest.
Describe the environment vividly.
At the end of every turn, present exactly 3 numbered choices in plain text:
1) ...
2) ...
3) ...
Do not continue the story until the player picks one choice.
Keep responses under 3 paragraphs plus the 3 choices.`;
      const { runId } = await api.startAsyncRun(sid, prompt);

      connectStream(sid, runId);
    } catch (err: any) {
      addEvent({ type: "system", content: `CRITICAL ERROR: ${err.message}` });
      setIsLoading(false);
    }
  };

  const connectStream = (sid: string, rid: string) => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    const eventSource = new EventSource(api.getEventStreamUrl(sid, rid));
    eventSourceRef.current = eventSource;
    let activeText = "";
    let finalized = false;

    const finalize = async () => {
      if (finalized) return;
      finalized = true;
      try {
        const messages = await api.getSessionMessages(sid);
        const restoredEvents = buildAdventureEventsFromMessages(messages);
        if (restoredEvents.length > 0) {
          setEvents(restoredEvents);
          maybeAddChoiceFromLastAssistant(restoredEvents);
        }
      } catch (err) {
        console.error("Failed to restore adventure history after run", err);
      } finally {
        setIsLoading(false);
        eventSource.close();
        if (eventSourceRef.current === eventSource) {
          eventSourceRef.current = null;
        }
      }
    };

    eventSource.onmessage = (evt) => {
      const data = JSON.parse(evt.data);

      if (
        data.type === "message.part.updated" &&
        data.properties.part.type === "text" &&
        data.properties.delta
      ) {
        activeText += data.properties.delta;
        // Live typing effect (we could throttle this to React state, but to avoid
        // huge re-renders for every token, we accumulate and flush periodically,
        // or just rely on the end of the text chunk)
        setEvents((prev) => {
          const updated = [...prev];
          const last = updated[updated.length - 1];
          if (last && last.type === "text") {
            last.content += data.properties.delta;
            return updated;
          } else {
            return [
              ...updated,
              { id: Math.random().toString(), type: "text", content: data.properties.delta },
            ];
          }
        });
      } else if (data.type === "question.asked") {
        // Format the question as a choice
        const qData = data.properties;
        // Assuming the engine emits a list of options in the UI definition
        addEvent({
          type: "choice",
          content: qData.question,
          // Extract choices from question properties if they exist, or fallback
          options: qData.options || ["Look around", "Check pockets", "Call out for help"],
          questionId: qData.question_id,
        });
      } else if (
        data.type === "run.status.updated" &&
        (data.properties.status === "completed" || data.properties.status === "failed")
      ) {
        void finalize();
      } else if (
        data.type === "session.run.finished" &&
        (data.properties?.status === "completed" || data.properties?.status === "failed")
      ) {
        void finalize();
      }
    };

    eventSource.onerror = () => {
      setIsLoading(false);
      eventSource.close();
      if (eventSourceRef.current === eventSource) {
        eventSourceRef.current = null;
      }
    };
  };

  const restartGame = async () => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    localStorage.removeItem(ADVENTURE_SESSION_KEY);
    setHasStarted(false);
    await startGame();
  };

  const handleChoice = async (choice: string, questionId?: string) => {
    if (!sessionId) return;
    setIsLoading(true);

    addEvent({ type: "hero", content: `> You chose: ${choice}` });
    setManualInput("");

    try {
      if (questionId) {
        // If it was a formal API question, answer it
        await fetch(`/engine/question/${questionId}/reply`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${localStorage.getItem("tandem_portal_token")}`,
          },
          body: JSON.stringify({ _answers: [[choice]] }),
        });
      }

      // Also emit as a message to resume the conversation flow
      const { runId } = await api.startAsyncRun(sessionId, choice);
      connectStream(sessionId, runId);
    } catch (err: any) {
      addEvent({ type: "system", content: `ERROR: ${err.message}` });
      setIsLoading(false);
    }
  };

  const handleManualSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const text = manualInput.trim();
    if (!text || !sessionId || isLoading) return;
    await handleChoice(text);
  };

  return (
    <div className="flex flex-col h-full bg-black p-0 md:p-6 font-mono">
      <div className="bg-gray-900 border border-green-900 flex flex-col h-full rounded-none md:rounded-xl shadow-[0_0_15px_rgba(16,185,129,0.1)] overflow-hidden">
        {/* Header terminal bar */}
        <div className="bg-gray-950 px-4 py-2 border-b border-green-900 flex justify-between items-center text-green-500 text-xs text-opacity-70 select-none">
          <span className="flex items-center gap-2">
            <MessageSquareQuote size={14} /> tty1 - tandem-rpg
          </span>
          <div className="flex items-center gap-3">
            {hasStarted && (
              <button
                type="button"
                onClick={restartGame}
                className="flex items-center gap-1 border border-green-800 px-2 py-1 hover:bg-green-900/40"
              >
                <RotateCcw size={12} />
                Restart Game
              </button>
            )}
            <span>{isLoading ? "EXECUTING..." : "IDLE"}</span>
          </div>
        </div>

        {/* Main terminal display */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6 text-green-400">
          {!hasStarted ? (
            <div className="h-full flex flex-col items-center justify-center space-y-8 opacity-80 hover:opacity-100 transition-opacity">
              <pre className="text-center text-green-500 font-bold text-xs sm:text-sm">
                {`
 _______  _______  _______  _______ 
(  ____ )(  ____ )(  ____ \\(  ____ \\
| (    )|| (    )|| (    \\/| (    \\/
| (____)|| (____)|| |      | |      
|     __)|  _____)| | ____ | | ____ 
| (\\ (   | (      | | \\_  )| | \\_  )
| ) \\ \\__| )      | (___) || (___) |
|/   \\__/|/       (_______)(_______)
                                `}
              </pre>
              <button
                onClick={startGame}
                className="border border-green-500 hover:bg-green-900 hover:text-green-300 transition-colors px-8 py-3 uppercase tracking-widest text-sm"
              >
                Start Adventure
              </button>
            </div>
          ) : (
            <>
              {events.map((evt) => (
                <div
                  key={evt.id}
                  className={`font-mono ${evt.type === "hero" ? "text-green-300 opacity-90" : evt.type === "system" ? "text-green-700" : "text-green-500"}`}
                >
                  {evt.type === "text" && (
                    <p className="whitespace-pre-wrap leading-relaxed">{evt.content}</p>
                  )}
                  {evt.type === "system" && <p className="opacity-50 text-xs">[{evt.content}]</p>}
                  {evt.type === "hero" && <p className="font-bold">{evt.content}</p>}
                  {evt.type === "choice" && (
                    <div className="mt-4 p-4 border border-green-900/50 bg-green-950/20 rounded">
                      <p className="font-bold mb-4 opacity-90">{evt.content}</p>
                      <div className="flex flex-col gap-2">
                        {evt.options?.map((opt, oIdx) => (
                          <button
                            key={oIdx}
                            disabled={isLoading}
                            onClick={() => handleChoice(opt, evt.questionId)}
                            className="text-left px-3 py-2 border border-green-800 hover:border-green-400 hover:bg-green-900/40 text-green-400 transition-all flex items-center gap-2 group disabled:opacity-50"
                          >
                            <ChevronRight
                              size={16}
                              className="opacity-0 group-hover:opacity-100 transition-opacity"
                            />
                            {oIdx + 1}. {opt}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              ))}
              <div ref={scrollRef} className="h-4" />
              {isLoading && (
                <div className="text-green-700 animate-pulse flex items-center gap-2">
                  <div className="w-2 h-4 bg-green-700"></div> The Game Master is typing...
                </div>
              )}
              {!isLoading && (
                <form onSubmit={handleManualSubmit} className="mt-4 flex gap-2">
                  <input
                    type="text"
                    value={manualInput}
                    onChange={(e) => setManualInput(e.target.value)}
                    placeholder="Type your action..."
                    className="flex-1 bg-black border border-green-900 px-3 py-2 text-green-300"
                  />
                  <button
                    type="submit"
                    className="border border-green-700 px-4 py-2 text-green-300 hover:bg-green-900/40"
                  >
                    Send
                  </button>
                </form>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
};
