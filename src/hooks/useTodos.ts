import { useState, useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getSessionTodos, type TodoItem } from "@/lib/tauri";

// Matches the Rust StreamEvent::TodoUpdated serialization format
// (uses #[serde(tag = "type", rename_all = "snake_case")])
interface TodoUpdatedPayload {
  type: "todo_updated";
  session_id: string;
  todos: TodoItem[];
}

interface StreamEventPayload {
  type: string;
  session_id?: string;
  todos?: TodoItem[];
}

function normalizeTodoStatus(status: string): TodoItem["status"] {
  const s = status.trim().toLowerCase();
  if (s === "in_progress" || s === "inprogress" || s === "running" || s === "working") {
    return "in_progress";
  }
  if (s === "completed" || s === "complete" || s === "done") {
    return "completed";
  }
  if (s === "cancelled" || s === "canceled" || s === "aborted" || s === "skipped") {
    return "cancelled";
  }
  return "pending";
}

function normalizeTodos(items: TodoItem[]): TodoItem[] {
  return items.map((t) => ({
    ...t,
    status: normalizeTodoStatus(t.status),
  }));
}

export function useTodos(sessionId: string | null) {
  const [todos, setTodos] = useState<TodoItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);

  // Fetch todos when session changes
  useEffect(() => {
    if (!sessionId) return;

    console.log("[useTodos] Fetching todos for session:", sessionId);
    // Avoid synchronous setState in effect body (eslint rule: react-hooks/set-state-in-effect)
    Promise.resolve().then(() => setIsLoading(true));
    getSessionTodos(sessionId)
      .then((fetchedTodos) => {
        console.log("[useTodos] Fetched todos:", fetchedTodos);
        setTodos(normalizeTodos(fetchedTodos));
      })
      .catch((error) => {
        console.error("[useTodos] Failed to fetch todos:", error);
        setTodos([]);
      })
      .finally(() => setIsLoading(false));
  }, [sessionId]);

  // Listen for todo_updated events from the Rust backend
  useEffect(() => {
    if (!sessionId) return;

    console.log("[useTodos] Setting up SSE listener for session:", sessionId);
    let unlisten: UnlistenFn | undefined;

    const setupListener = async () => {
      unlisten = await listen<StreamEventPayload>("sidecar_event", (event) => {
        const payload = event.payload;

        // Check if this is a todo_updated event for our session
        // The Rust StreamEvent uses #[serde(tag = "type", rename_all = "snake_case")]
        if (payload.type === "todo_updated") {
          const typedPayload = payload as TodoUpdatedPayload;
          console.log(
            "[useTodos] todo_updated event for session:",
            typedPayload.session_id,
            "current:",
            sessionId
          );
          if (typedPayload.session_id === sessionId && typedPayload.todos) {
            console.log("[useTodos] Updating todos:", typedPayload.todos);
            setTodos(normalizeTodos(typedPayload.todos));
          }
        }
      });
    };

    setupListener();

    return () => {
      if (unlisten) {
        console.log("[useTodos] Cleaning up listener");
        unlisten();
      }
    };
  }, [sessionId]);

  // If there's no active session, present an empty view without mutating state in an effect.
  const effectiveTodos = sessionId ? todos : [];

  // Categorize todos by status
  const pending = effectiveTodos.filter((t) => t.status === "pending");
  const inProgress = effectiveTodos.filter((t) => t.status === "in_progress");
  const completed = effectiveTodos.filter((t) => t.status === "completed");
  const cancelled = effectiveTodos.filter((t) => t.status === "cancelled");

  return {
    todos: effectiveTodos,
    pending,
    inProgress,
    completed,
    cancelled,
    isLoading: sessionId ? isLoading : false,
  };
}
