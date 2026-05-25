import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  clearProjectFileIndex,
  indexWorkspace,
  type ClearFileIndexResult,
  type IndexingComplete,
  type IndexingProgress,
  type IndexingStart,
  type IndexingStats,
} from "@/lib/tauri";

export type IndexingStatus = "idle" | "indexing" | "done" | "error";

export interface ProjectIndexingState {
  status: IndexingStatus;
  start?: IndexingStart;
  progress?: IndexingProgress;
  complete?: IndexingComplete;
  error?: string;
}

export interface MemoryIndexingContextValue {
  projects: Record<string, ProjectIndexingState>;
  startIndex: (projectId: string) => Promise<IndexingStats>;
  clearFileIndex: (projectId: string, vacuum: boolean) => Promise<ClearFileIndexResult>;
}

const MemoryIndexingContext = createContext<MemoryIndexingContextValue | null>(null);

export function MemoryIndexingProvider({ children }: { children: React.ReactNode }) {
  const [projects, setProjects] = useState<Record<string, ProjectIndexingState>>({});
  const inFlightRef = useRef<Map<string, Promise<IndexingStats>>>(new Map());

  useEffect(() => {
    let unlistenStart: UnlistenFn | undefined;
    let unlistenProgress: UnlistenFn | undefined;
    let unlistenComplete: UnlistenFn | undefined;

    const setup = async () => {
      unlistenStart = await listen<IndexingStart>("indexing-start", (event) => {
        const pid = event.payload.project_id;
        setProjects((prev) => ({
          ...prev,
          [pid]: {
            status: "indexing",
            start: event.payload,
            progress: undefined,
            error: undefined,
          },
        }));
      });

      unlistenProgress = await listen<IndexingProgress>("indexing-progress", (event) => {
        const pid = event.payload.project_id;
        setProjects((prev) => ({
          ...prev,
          [pid]: {
            ...(prev[pid] ?? { status: "indexing" }),
            status: "indexing",
            progress: event.payload,
          },
        }));
      });

      unlistenComplete = await listen<IndexingComplete>("indexing-complete", (event) => {
        const pid = event.payload.project_id;
        setProjects((prev) => ({
          ...prev,
          [pid]: { ...(prev[pid] ?? { status: "done" }), status: "done", complete: event.payload },
        }));
      });
    };

    setup().catch((e) => {
      console.error("Failed to setup indexing listeners:", e);
    });

    return () => {
      if (unlistenStart) unlistenStart();
      if (unlistenProgress) unlistenProgress();
      if (unlistenComplete) unlistenComplete();
    };
  }, []);

  const startIndex = useCallback(async (projectId: string) => {
    const existing = inFlightRef.current.get(projectId);
    if (existing) return existing;

    const p = indexWorkspace(projectId)
      .then((res) => {
        return res;
      })
      .catch((err) => {
        setProjects((prev) => ({
          ...prev,
          [projectId]: {
            ...(prev[projectId] ?? { status: "error" }),
            status: "error",
            error: err instanceof Error ? err.message : String(err),
          },
        }));
        throw err;
      })
      .finally(() => {
        inFlightRef.current.delete(projectId);
      });

    inFlightRef.current.set(projectId, p);
    return p;
  }, []);

  const clearFileIndex = useCallback(async (projectId: string, vacuum: boolean) => {
    const res = await clearProjectFileIndex(projectId, vacuum);
    setProjects((prev) => ({
      ...prev,
      [projectId]: { status: "idle" },
    }));
    return res;
  }, []);

  const value = useMemo<MemoryIndexingContextValue>(() => {
    return { projects, startIndex, clearFileIndex };
  }, [projects, startIndex, clearFileIndex]);

  return <MemoryIndexingContext.Provider value={value}>{children}</MemoryIndexingContext.Provider>;
}

export function useMemoryIndexing(): MemoryIndexingContextValue {
  const ctx = useContext(MemoryIndexingContext);
  if (!ctx) {
    throw new Error("useMemoryIndexing must be used within MemoryIndexingProvider");
  }
  return ctx;
}
