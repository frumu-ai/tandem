import { useState, useEffect, useCallback, useRef } from "react";
import { getAppState, type AppStateInfo } from "@/lib/tauri";

export function useAppState() {
  const [state, setState] = useState<AppStateInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const isInitialLoad = useRef(true);

  const refresh = useCallback(async () => {
    try {
      if (isInitialLoad.current) {
        setLoading(true);
      }
      const appState = await getAppState();
      setState(appState);
      isInitialLoad.current = false;
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
