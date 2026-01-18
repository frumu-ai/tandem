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

export function useTodos(sessionId: string | null) {
  const [todos, setTodos] = useState<TodoItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);

  // Fetch todos when session changes
  useEffect(() => {
    if (!sessionId) {
      console.log("[useTodos] No session, clearing todos");
      setTodos([]);
      return;
    }

    console.log("[useTodos] Fetching todos for session:", sessionId);
    setIsLoading(true);
    getSessionTodos(sessionId)
      .then((fetchedTodos) => {
        console.log("[useTodos] Fetched todos:", fetchedTodos);
        setTodos(fetchedTodos);
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
          console.log("[useTodos] todo_updated event for session:", typedPayload.session_id, "current:", sessionId);
          if (typedPayload.session_id === sessionId && typedPayload.todos) {
            console.log("[useTodos] Updating todos:", typedPayload.todos);
            setTodos(typedPayload.todos);
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

  // Categorize todos by status
  const pending = todos.filter((t) => t.status === "pending");
  const inProgress = todos.filter((t) => t.status === "in_progress");
  const completed = todos.filter((t) => t.status === "completed");
  const cancelled = todos.filter((t) => t.status === "cancelled");

  return {
    todos,
    pending,
    inProgress,
    completed,
    cancelled,
    isLoading,
  };
}
