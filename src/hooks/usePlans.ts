import { useState, useEffect, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { listPlans, readPlanContent } from "@/lib/tauri-plans";

export interface Plan {
  sessionName: string;
  fileName: string;
  fullPath: string;
  content: string;
  lastModified: Date;
}

interface UsePlansReturn {
  plans: Plan[];
  activePlan: Plan | null;
  setActivePlan: (plan: Plan | null) => void;
  refreshPlans: () => Promise<void>;
  isLoading: boolean;
  error: string | null;
}

/**
 * Hook to manage plan files in the workspace
 * Watches .opencode/plans/ directory and provides plan state
 */
export function usePlans(workspacePath: string | null): UsePlansReturn {
  const [plans, setPlans] = useState<Plan[]>([]);
  const [activePlan, setActivePlan] = useState<Plan | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshPlans = useCallback(async () => {
    if (!workspacePath) {
      setPlans([]);
      return;
    }

    setIsLoading(true);
    setError(null);

    try {
      const planInfos = await listPlans();

      // Load content for each plan
      const plansWithContent = await Promise.all(
        planInfos.map(async (info) => {
          try {
            const content = await readPlanContent(info.fullPath);
            return {
              sessionName: info.sessionName,
              fileName: info.fileName,
              fullPath: info.fullPath,
              content,
              lastModified: new Date(info.lastModified),
            };
          } catch (err) {
            console.error(`Failed to read plan ${info.fileName}:`, err);
            return null;
          }
        })
      );

      const validPlans = plansWithContent.filter((p): p is Plan => p !== null);
      setPlans(validPlans);

      // If active plan was deleted or changed, update it
      if (activePlan) {
        const updatedActivePlan = validPlans.find((p) => p.fullPath === activePlan.fullPath);
        if (updatedActivePlan) {
          setActivePlan(updatedActivePlan);
        } else {
          setActivePlan(null);
        }
      }
    } catch (err) {
      console.error("Failed to load plans:", err);
      setError(err instanceof Error ? err.message : "Failed to load plans");
    } finally {
      setIsLoading(false);
    }
  }, [workspacePath, activePlan]);

  // Initial load
  useEffect(() => {
    refreshPlans();
  }, [refreshPlans]);

  // Listen for file changes from the file watcher
  useEffect(() => {
    const unlisten = listen<string[]>("plan-file-changed", (event) => {
      console.log("[usePlans] Plan file changed:", event.payload);
      // Refresh plans when files change
      refreshPlans();
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refreshPlans]);

  return {
    plans,
    activePlan,
    setActivePlan,
    refreshPlans,
    isLoading,
    error,
  };
}
