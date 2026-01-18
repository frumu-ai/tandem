import { useState, useEffect, useCallback } from "react";
import {
  getStagedOperations,
  stageToolOperation,
  removeStagedOperation,
  executeStagedPlan,
  clearStagingArea,
  type StagedOperation,
} from "@/lib/tauri";

export interface UseStagingAreaReturn {
  stagedOperations: StagedOperation[];
  stagedCount: number;
  isExecuting: boolean;
  stageOperation: (
    requestId: string,
    sessionId: string,
    tool: string,
    args: Record<string, unknown>,
    messageId?: string
  ) => Promise<void>;
  removeOperation: (operationId: string) => Promise<void>;
  executePlan: () => Promise<string[]>;
  clearStaging: () => Promise<void>;
  refreshStaging: () => Promise<void>;
}

export function useStagingArea(): UseStagingAreaReturn {
  const [stagedOperations, setStagedOperations] = useState<StagedOperation[]>([]);
  const [stagedCount, setStagedCount] = useState(0);
  const [isExecuting, setIsExecuting] = useState(false);

  const refreshStaging = useCallback(async () => {
    try {
      const operations = await getStagedOperations();
      setStagedOperations(operations);
      setStagedCount(operations.length);
    } catch (error) {
      console.error("Failed to refresh staged operations:", error);
    }
  }, []);

  // Load staged operations on mount
  useEffect(() => {
    refreshStaging();
  }, [refreshStaging]);

  const stageOperation = useCallback(
    async (
      requestId: string,
      sessionId: string,
      tool: string,
      args: Record<string, unknown>,
      messageId?: string
    ) => {
      try {
        await stageToolOperation(requestId, sessionId, tool, args, messageId);
        await refreshStaging();
      } catch (error) {
        console.error("Failed to stage operation:", error);
        throw error;
      }
    },
    [refreshStaging]
  );

  const removeOperation = useCallback(
    async (operationId: string) => {
      try {
        await removeStagedOperation(operationId);
        await refreshStaging();
      } catch (error) {
        console.error("Failed to remove staged operation:", error);
        throw error;
      }
    },
    [refreshStaging]
  );

  const executePlan = useCallback(async () => {
    setIsExecuting(true);
    try {
      const executedIds = await executeStagedPlan();
      await refreshStaging();
      return executedIds;
    } catch (error) {
      console.error("Failed to execute staged plan:", error);
      throw error;
    } finally {
      setIsExecuting(false);
    }
  }, [refreshStaging]);

  const clearStaging = useCallback(async () => {
    try {
      await clearStagingArea();
      await refreshStaging();
    } catch (error) {
      console.error("Failed to clear staging area:", error);
      throw error;
    }
  }, [refreshStaging]);

  return {
    stagedOperations,
    stagedCount,
    isExecuting,
    stageOperation,
    removeOperation,
    executePlan,
    clearStaging,
    refreshStaging,
  };
}
