import { useEffect, useState } from "react";

export type ActiveTab = "create" | "calendar" | "list" | "running" | "optimize" | "approvals";
export type CreateMode = "simple" | "advanced";

const AUTOMATIONS_STUDIO_HANDOFF_KEY = "tandem.automations.studioHandoff";

export function useAutomationsPageState() {
  const [tab, setTab] = useState<ActiveTab>("calendar");
  const [createMode, setCreateMode] = useState<CreateMode>("simple");
  const [selectedRunId, setSelectedRunId] = useState<string>("");
  const [advancedEditAutomation, setAdvancedEditAutomation] = useState<any | null>(null);

  useEffect(() => {
    try {
      const raw = sessionStorage.getItem(AUTOMATIONS_STUDIO_HANDOFF_KEY);
      if (!raw) return;
      sessionStorage.removeItem(AUTOMATIONS_STUDIO_HANDOFF_KEY);
      const handoff = JSON.parse(raw || "{}");
      if (handoff?.tab === "running") setTab("running");
      const runId = String(handoff?.runId || "").trim();
      if (runId) setSelectedRunId(runId);
    } catch {
      return;
    }
  }, []);

  return {
    tab,
    setTab,
    createMode,
    setCreateMode,
    selectedRunId,
    setSelectedRunId,
    advancedEditAutomation,
    setAdvancedEditAutomation,
  };
}
