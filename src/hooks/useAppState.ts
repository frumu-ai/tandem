import { useState, useEffect, useCallback } from "react";
import { getAppState, type AppStateInfo } from "@/lib/tauri";

export function useAppState() {
  const [state, setState] = useState<AppStateInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setLoading(true);
      const appState = await getAppState();
      setState(appState);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load app state");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return { state, loading, error, refresh };
}
