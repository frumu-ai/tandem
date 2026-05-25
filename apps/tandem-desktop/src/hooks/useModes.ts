import { useCallback, useEffect, useMemo, useState } from "react";
import { listModes, type ModeDefinition } from "@/lib/tauri";

interface UseModesResult {
  modes: ModeDefinition[];
  isLoading: boolean;
  error: string | null;
  refreshModes: () => Promise<void>;
}

export function useModes(): UseModesResult {
  const [modes, setModes] = useState<ModeDefinition[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshModes = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const loaded = await listModes();
      const sorted = [...loaded].sort((a, b) => a.label.localeCompare(b.label));
      setModes(sorted);
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to load modes";
      setError(message);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshModes();
  }, [refreshModes]);

  return useMemo(
    () => ({
      modes,
      isLoading,
      error,
      refreshModes,
    }),
    [modes, isLoading, error, refreshModes]
  );
}
