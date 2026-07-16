import { useCallback, useEffect, useState } from "react";

export type ActiveTab = "create" | "calendar" | "list" | "running";
export type CreateMode = "simple" | "advanced" | "composer";

const AUTOMATIONS_STUDIO_HANDOFF_KEY = "tandem.automations.studioHandoff";
const AUTOMATION_COMPOSER_FEATURE_KEY = "tandem.automations.composer.enabled";

function findComposerEnabledFromUrl() {
  if (typeof window === "undefined") return false;
  try {
    const current = new URL(window.location.href);
    const q = current.searchParams.get("composer");
    if (!q) return false;
    const enabled = q.trim().toLowerCase();
    return enabled === "1" || enabled === "true" || enabled === "on" || enabled === "yes";
  } catch {
    return false;
  }
}

function composerEnabledFromStorage() {
  if (typeof window === "undefined" || typeof localStorage === "undefined") return false;
  try {
    return localStorage.getItem(AUTOMATION_COMPOSER_FEATURE_KEY) === "1";
  } catch {
    return false;
  }
}

function automationRunIdFromHash() {
  if (typeof window === "undefined") return "";
  const [, query = ""] = String(window.location.hash || "").split("?");
  return String(new URLSearchParams(query).get("run") || "").trim();
}

function replaceAutomationRunHash(runId: string) {
  if (typeof window === "undefined") return;
  const route = String(window.location.hash || "").replace(/^#\//, "").split("?")[0];
  if (route !== "automations") return;
  const hash = runId ? `#/automations?run=${encodeURIComponent(runId)}` : "#/automations";
  window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}${hash}`);
}

function initialCreateMode(isComposerEnabled: boolean) {
  if (!isComposerEnabled) return "simple" as const;
  if (typeof window === "undefined") return "simple" as const;
  try {
    const fromHash = window.location.hash.includes("composer=true");
    if (fromHash) return "composer" as const;
  } catch {
    // no-op
  }
  return "simple" as const;
}

export function useAutomationsPageState() {
  const [tab, setTab] = useState<ActiveTab>(() =>
    automationRunIdFromHash() ? "running" : findComposerEnabledFromUrl() ? "create" : "calendar"
  );
  const [createMode, setCreateMode] = useState<CreateMode>(() =>
    initialCreateMode(composerEnabledFromStorage() || findComposerEnabledFromUrl())
  );
  const [selectedRunId, setSelectedRunIdState] = useState<string>(automationRunIdFromHash);
  const setSelectedRunId = useCallback((runId: string) => {
    const normalized = String(runId || "").trim();
    setSelectedRunIdState(normalized);
    replaceAutomationRunHash(normalized);
  }, []);
  const [advancedEditAutomation, setAdvancedEditAutomation] = useState<any | null>(null);
  const isComposerEnabled = composerEnabledFromStorage() || findComposerEnabledFromUrl();

  useEffect(() => {
    const syncRunFromHash = () => {
      const runId = automationRunIdFromHash();
      setSelectedRunIdState(runId);
      if (runId) setTab("running");
    };
    window.addEventListener("hashchange", syncRunFromHash);
    return () => window.removeEventListener("hashchange", syncRunFromHash);
  }, []);

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
    composerEnabled: isComposerEnabled,
  };
}
