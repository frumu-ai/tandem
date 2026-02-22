import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useTranslation } from "react-i18next";
import { Settings } from "@/components/settings";
import { About } from "@/components/about";
import { Chat } from "@/components/chat";
import { Extensions } from "@/components/extensions";
import { SidecarDownloader } from "@/components/sidecar";
import { SessionSidebar, type SessionInfo, type Project } from "@/components/sidebar";
import { TaskSidebar } from "@/components/tasks/TaskSidebar";
import { FileBrowser } from "@/components/files/FileBrowser";
import { FilePreview } from "@/components/files/FilePreview";
import { GitInitDialog } from "@/components/dialogs/GitInitDialog";
import { OrchestratorPanel } from "@/components/orchestrate/OrchestratorPanel";
import { CommandCenterPage } from "@/components/command-center/CommandCenterPage";
import { type RunSummary } from "@/components/orchestrate/types";
import { PacksPanel } from "@/components/packs";
import { AppUpdateOverlay } from "@/components/updates/AppUpdateOverlay";
import { WhatsNewOverlay } from "@/components/updates/WhatsNewOverlay";
import { StorageMigrationOverlay } from "@/components/migration/StorageMigrationOverlay";
import { useAppState } from "@/hooks/useAppState";
import { useTheme } from "@/hooks/useTheme";
import { useTodos } from "@/hooks/useTodos";
import { cn } from "@/lib/utils";
import { BrandMark } from "@/components/ui/BrandMark";
import { OnboardingWizard } from "@/components/onboarding/OnboardingWizard";
import { useMemoryIndexing } from "@/contexts/MemoryIndexingContext";
import { resolveSessionDirectory, sessionBelongsToWorkspace } from "@/lib/sessionScope";
import {
  listSessions,
  getSessionActiveRun,
  listProjects,
  deleteSession,
  deleteOrchestratorRun,
  getVaultStatus,
  getUserProjects,
  getActiveProject,
  setActiveProject,
  addProject,
  getSidecarStatus,
  getMemorySettings,
  getProjectMemoryStats,
  readFileContent,
  readFileText,
  readBinaryFile,
  checkGitStatus,
  initializeGitRepo,
  onSidecarEvent,
  onSidecarEventV2,
  getStorageMigrationStatus,
  onStorageMigrationComplete,
  onStorageMigrationProgress,
  runStorageMigration,
  engineAcquireLease,
  engineRenewLease,
  engineReleaseLease,
  type StorageMigrationProgressEvent,
  type StorageMigrationRunResult,
  type Session,
  type StreamEventEnvelopeV2,
  type VaultStatus,
  type UserProject,
  type FileEntry,
} from "@/lib/tauri";
import { type FileAttachment } from "@/components/chat/ChatInput";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import {
  Settings as SettingsIcon,
  MessageSquare,
  Shield,
  PanelLeftClose,
  PanelLeft,
  Info,
  ListTodo,
  Files,
  Palette,
  Sparkles,
  Rocket,
  Blocks,
  Loader2,
} from "lucide-react";
import whatsNewMarkdown from "../docs/WHATS_NEW_v0.3.0.md?raw";

const WHATS_NEW_VERSION = "v0.3.0-beta";
const WHATS_NEW_SEEN_KEY = "tandem_whats_new_seen_version";

type View =
  | "chat"
  | "command-center"
  | "extensions"
  | "settings"
  | "about"
  | "packs"
  | "onboarding"
  | "sidecar-setup";

// Hide the HTML splash screen once React is ready and vault is unlocked
function hideSplashScreen() {
  const splash = document.getElementById("splash-screen");
  if (splash) {
    splash.classList.add("hidden");
    // Clean up matrix animation
    if (window.__matrixInterval) {
      window.clearInterval(window.__matrixInterval);
    }
    // Remove splash after transition
    setTimeout(() => splash.remove(), 500);
  }
}

// Add type for the global properties
declare global {
  interface Window {
    __matrixInterval?: ReturnType<typeof window.setInterval>;
    __splashStartedAt?: number;
    __vaultUnlocked?: boolean;
  }
}

function App() {
  const { t } = useTranslation(["common", "chat", "settings"]);
  const { state, loading, refresh: refreshAppState } = useAppState();
  const { cycleTheme } = useTheme();
  const { startIndex } = useMemoryIndexing();
  const [sidecarReady, setSidecarReady] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [taskSidebarOpen, setTaskSidebarOpen] = useState(false);
  const [usePlanMode, setUsePlanMode] = useState(false);
  const [selectedAgent, setSelectedAgent] = useState<string | undefined>(undefined);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [draftMessage, setDraftMessage] = useState<string | null>(null);
  const [settingsInitialSection, setSettingsInitialSection] = useState<
    "providers" | "projects" | null
  >(null);
  const [extensionsInitialTab, setExtensionsInitialTab] = useState<
    "skills" | "plugins" | "mcp" | "modes" | null
  >(null);
  const [postAddProjectView, setPostAddProjectView] = useState<View | null>(null);
  // Initialize currentSessionId from localStorage to persist state across reloads/rebuilds
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(() => {
    if (typeof window !== "undefined") {
      return localStorage.getItem("tandem_current_session_id");
    }
    return null;
  });
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyOverlayOpen, setHistoryOverlayOpen] = useState(false);
  const historyOverlayDelayRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const historyRefreshDebounceRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const loadHistoryRef = useRef<() => Promise<void>>(async () => {});
  const [vaultUnlocked, setVaultUnlocked] = useState(false);
  const [executePendingTrigger, setExecutePendingTrigger] = useState(0);
  const [isExecutingTasks, setIsExecutingTasks] = useState(false);
  const [runningSessionIds, setRunningSessionIds] = useState<Set<string>>(() => new Set());
  const autoIndexDebounceRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const autoIndexLastRunRef = useRef<number>(0);

  // Persist currentSessionId to localStorage whenever it changes
  useEffect(() => {
    if (currentSessionId) {
      localStorage.setItem("tandem_current_session_id", currentSessionId);
    } else {
      localStorage.removeItem("tandem_current_session_id");
    }
  }, [currentSessionId]);

  useEffect(() => {
    let disposed = false;

    const clearRenewTimer = () => {
      if (engineLeaseTimerRef.current) {
        globalThis.clearInterval(engineLeaseTimerRef.current);
        engineLeaseTimerRef.current = null;
      }
    };

    const releaseLease = async () => {
      clearRenewTimer();
      const leaseId = engineLeaseIdRef.current;
      if (!leaseId) return;
      engineLeaseIdRef.current = null;
      try {
        await engineReleaseLease(leaseId);
      } catch (err) {
        console.warn("[EngineLease] Failed to release lease:", err);
      }
    };

    const acquireLease = async () => {
      try {
        const lease = await engineAcquireLease("desktop-ui", "desktop", 60_000);
        if (disposed) return;
        engineLeaseIdRef.current = lease.lease_id;
        clearRenewTimer();
        engineLeaseTimerRef.current = globalThis.setInterval(async () => {
          const id = engineLeaseIdRef.current;
          if (!id) return;
          try {
            const ok = await engineRenewLease(id);
            if (!ok) {
              console.warn("[EngineLease] Renewal failed, reacquiring lease");
              engineLeaseIdRef.current = null;
              await acquireLease();
            }
          } catch (err) {
            console.warn("[EngineLease] Failed to renew lease:", err);
          }
        }, 20_000);
      } catch (err) {
        console.warn("[EngineLease] Failed to acquire lease:", err);
      }
    };

    if (vaultUnlocked) {
      void acquireLease();
    } else {
      void releaseLease();
    }

    return () => {
      disposed = true;
      void releaseLease();
    };
  }, [vaultUnlocked]);

  // File browser state
  const [sidebarTab, setSidebarTab] = useState<"sessions" | "files">("sessions");
  const [selectedFile, setSelectedFile] = useState<FileEntry | null>(null);
  const [filePreviewDockEl, setFilePreviewDockEl] = useState<HTMLDivElement | null>(null);
  const [fileToAttach, setFileToAttach] = useState<FileAttachment | null>(null);

  // Project management state
  const [userProjects, setUserProjects] = useState<UserProject[]>([]);
  const [activeProject, setActiveProjectState] = useState<UserProject | null>(null);
  const [projectSwitcherLoading, setProjectSwitcherLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [migrationOverlayOpen, setMigrationOverlayOpen] = useState(false);
  const [migrationRunning, setMigrationRunning] = useState(false);
  const [migrationProgress, setMigrationProgress] = useState<StorageMigrationProgressEvent | null>(
    null
  );
  const [migrationResult, setMigrationResult] = useState<StorageMigrationRunResult | null>(null);
  const [showWhatsNew, setShowWhatsNew] = useState(false);
  const migrationCheckedRef = useRef(false);
  const engineLeaseIdRef = useRef<string | null>(null);
  const engineLeaseTimerRef = useRef<ReturnType<typeof globalThis.setInterval> | null>(null);

  useEffect(() => {
    if (!vaultUnlocked) return;
    const seenVersion = localStorage.getItem(WHATS_NEW_SEEN_KEY);
    if (seenVersion !== WHATS_NEW_VERSION) {
      setShowWhatsNew(true);
    }
  }, [vaultUnlocked]);

  const dismissWhatsNew = useCallback(() => {
    localStorage.setItem(WHATS_NEW_SEEN_KEY, WHATS_NEW_VERSION);
    setShowWhatsNew(false);
  }, []);

  // Auto-index workspace files when a project becomes active (if enabled in settings).
  useEffect(() => {
    let cancelled = false;

    const maybeAutoIndex = async () => {
      if (!activeProject?.id) return;

      try {
        const settings = await getMemorySettings();
        if (!settings.auto_index_on_project_load) return;

        // Cooldown: don't auto-start if we just indexed recently.
        const stats = await getProjectMemoryStats(activeProject.id);
        if (stats.last_indexed_at) {
          const last = new Date(stats.last_indexed_at).getTime();
          if (Number.isFinite(last) && Date.now() - last < 5 * 60 * 1000) return;
        }

        if (cancelled) return;
        await startIndex(activeProject.id);
      } catch (e) {
        console.warn("[AutoIndex] Failed to auto-index:", e);
      }
    };

    maybeAutoIndex();
    return () => {
      cancelled = true;
    };
  }, [activeProject?.id, startIndex]);

  // Incremental auto-index when AI creates/edits files.
  // This keeps vector memory fresh after tool-based changes, not just on project load.
  useEffect(() => {
    if (!activeProject?.id) return;

    let disposed = false;
    let unlisten: (() => void) | null = null;

    const isFileWriteTool = (tool: string) =>
      tool === "write" ||
      tool === "write_file" ||
      tool === "create_file" ||
      tool === "delete" ||
      tool === "delete_file" ||
      tool === "edit" ||
      tool === "patch";

    const triggerIndex = async () => {
      if (disposed) return;
      try {
        const settings = await getMemorySettings();
        if (!settings.auto_index_on_project_load) return;

        // Cooldown to avoid excessive indexing bursts during large edit batches.
        const now = Date.now();
        if (now - autoIndexLastRunRef.current < 15_000) return;

        await startIndex(activeProject.id);
        autoIndexLastRunRef.current = now;
      } catch (e) {
        console.warn("[AutoIndex] Failed to refresh index after file change:", e);
      }
    };

    const scheduleIndex = () => {
      if (autoIndexDebounceRef.current) {
        globalThis.clearTimeout(autoIndexDebounceRef.current);
      }
      autoIndexDebounceRef.current = globalThis.setTimeout(() => {
        void triggerIndex();
      }, 1500);
    };

    const setup = async () => {
      try {
        unlisten = await onSidecarEvent((event) => {
          if (event.type === "file_edited") {
            scheduleIndex();
            return;
          }
          if (event.type === "tool_end" && isFileWriteTool(event.tool) && !event.error) {
            scheduleIndex();
          }
        });
      } catch (e) {
        console.warn("[AutoIndex] Failed to subscribe to file change events:", e);
      }
    };

    void setup();

    return () => {
      disposed = true;
      if (autoIndexDebounceRef.current) {
        globalThis.clearTimeout(autoIndexDebounceRef.current);
        autoIndexDebounceRef.current = null;
      }
      try {
        unlisten?.();
      } catch {
        // ignore
      }
    };
  }, [activeProject?.id, startIndex]);

  // Git initialization dialog state
  const [showGitDialog, setShowGitDialog] = useState(false);
  const [pendingProjectPath, setPendingProjectPath] = useState<string | null>(null);
  const [gitStatus, setGitStatus] = useState<{ git_installed: boolean; is_repo: boolean } | null>(
    null
  );

  // Orchestrator panel state
  const [orchestratorOpen, setOrchestratorOpen] = useState(false);
  const [currentOrchestratorRunId, setCurrentOrchestratorRunId] = useState<string | null>(null);
  const [commandCenterRunId, setCommandCenterRunId] = useState<string | null>(null);
  const [orchestratorRuns, setOrchestratorRuns] = useState<RunSummary[]>([]);
  const skipOrchestratorAutoResumeRef = useRef(false);

  // Poll for orchestrator runs
  useEffect(() => {
    // Initial fetch
    invoke<RunSummary[]>("orchestrator_list_runs").then(setOrchestratorRuns).catch(console.error);

    const interval = setInterval(() => {
      invoke<RunSummary[]>("orchestrator_list_runs").then(setOrchestratorRuns).catch(console.error);
    }, 5000);
    return () => clearInterval(interval);
  }, [activeProject]); // Re-fetch when project changes

  function scheduleHistoryRefresh() {
    if (historyRefreshDebounceRef.current) {
      globalThis.clearTimeout(historyRefreshDebounceRef.current);
      historyRefreshDebounceRef.current = null;
    }
    historyRefreshDebounceRef.current = globalThis.setTimeout(() => {
      historyRefreshDebounceRef.current = null;
      void loadHistoryRef.current();
    }, 350);
  }

  // Track running sessions globally from sidecar stream events so indicators remain accurate
  // even when the user switches away from the active chat tab/session.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    const setup = async () => {
      try {
        unlisten = await onSidecarEventV2((envelope: StreamEventEnvelopeV2) => {
          const payload = envelope.payload;
          const sid = "session_id" in payload ? payload.session_id : undefined;
          if (!sid) return;

          if (
            payload.type === "session_idle" ||
            payload.type === "session_error" ||
            payload.type === "run_finished"
          ) {
            setRunningSessionIds((prev) => {
              if (!prev.has(sid)) return prev;
              const next = new Set(prev);
              next.delete(sid);
              return next;
            });
            scheduleHistoryRefresh();
            return;
          }

          if (payload.type === "run_started") {
            setRunningSessionIds((prev) => {
              if (prev.has(sid)) return prev;
              const next = new Set(prev);
              next.add(sid);
              return next;
            });
            return;
          }

          if (payload.type === "session_status") {
            const terminal = [
              "idle",
              "completed",
              "failed",
              "error",
              "cancelled",
              "timeout",
            ].includes(payload.status);
            const running = ["running", "in_progress", "executing"].includes(payload.status);
            setRunningSessionIds((prev) => {
              const has = prev.has(sid);
              if (terminal && has) {
                const next = new Set(prev);
                next.delete(sid);
                return next;
              }
              if (running && !has) {
                const next = new Set(prev);
                next.add(sid);
                return next;
              }
              return prev;
            });
            if (terminal) {
              scheduleHistoryRefresh();
            }
            return;
          }
        });
      } catch (e) {
        console.error("Failed to subscribe to sidecar events in App:", e);
      }
    };
    setup();
    return () => {
      try {
        unlisten?.();
      } catch {
        // ignore
      }
    };
  }, []);

  // If panel opens with no explicit run selected, resume only active runs.
  // Do not auto-open completed/failed/cancelled history; let users start fresh by default.
  useEffect(() => {
    if (!orchestratorOpen || currentOrchestratorRunId) return;
    if (skipOrchestratorAutoResumeRef.current) {
      skipOrchestratorAutoResumeRef.current = false;
      return;
    }
    invoke<RunSummary[]>("orchestrator_list_runs")
      .then((runs) => {
        if (!runs || runs.length === 0) return;
        const preferred = runs.find(
          (r) =>
            r.source !== "command_center" &&
            ["planning", "awaiting_approval", "executing", "paused"].includes(r.status)
        );
        if (!preferred) return;
        setCurrentOrchestratorRunId(preferred.run_id);
      })
      .catch(console.error);
  }, [orchestratorOpen, currentOrchestratorRunId]);

  // When the active project changes, clear stale orchestrator selection that does not
  // belong to the newly active workspace.
  useEffect(() => {
    if (!currentOrchestratorRunId) return;
    const existsInActiveWorkspace = orchestratorRuns.some(
      (run) => run.run_id === currentOrchestratorRunId
    );
    if (!existsInActiveWorkspace) {
      setCurrentOrchestratorRunId(null);
    }
  }, [activeProject?.id, orchestratorRuns, currentOrchestratorRunId]);

  useEffect(() => {
    if (!commandCenterRunId) return;
    const existsInActiveWorkspace = orchestratorRuns.some(
      (run) => run.run_id === commandCenterRunId
    );
    if (!existsInActiveWorkspace) {
      setCommandCenterRunId(null);
    }
  }, [activeProject?.id, orchestratorRuns, commandCenterRunId]);

  // Todos for task sidebar
  const todosData = useTodos(currentSessionId);

  // Start with sidecar setup, then onboarding if no workspace, otherwise chat
  const [view, setView] = useState<View>(() => "sidecar-setup");

  // Check if any provider is fully configured (enabled + has key)
  const hasConfiguredProvider =
    !!state?.providers_config &&
    ((state.providers_config.openrouter?.enabled && state.providers_config.openrouter?.has_key) ||
      (state.providers_config.anthropic?.enabled && state.providers_config.anthropic?.has_key) ||
      (state.providers_config.openai?.enabled && state.providers_config.openai?.has_key) ||
      state.providers_config.opencode_zen?.enabled ||
      (state.providers_config.ollama?.enabled && state.providers_config.ollama?.has_key) ||
      // If the user explicitly selected a provider+model from the sidecar catalog, treat as configured.
      (!!state.providers_config.selected_model?.provider_id?.trim() &&
        !!state.providers_config.selected_model?.model_id?.trim()) ||
      // Custom providers (e.g. LM Studio / OpenAI-compatible endpoints) often don't require a key.
      (state.providers_config.custom?.some((c) => c.enabled && !!c.endpoint?.trim()) ?? false));

  const activeProviderInfo = useMemo(() => {
    const config = state?.providers_config;
    if (!config) return null;

    const labelForProviderId = (id: string) => {
      switch (id) {
        case "openrouter":
          return "OpenRouter";
        case "opencode_zen":
          return "Opencode Zen";
        case "anthropic":
          return "Anthropic";
        case "openai":
          return "OpenAI";
        case "ollama":
          return "Ollama";
        default:
          return id.charAt(0).toUpperCase() + id.slice(1);
      }
    };

    // Prefer the explicitly selected model/provider (supports Tandem custom providers).
    if (config.selected_model?.provider_id?.trim() && config.selected_model?.model_id?.trim()) {
      const providerId =
        config.selected_model.provider_id === "opencode"
          ? "opencode_zen"
          : config.selected_model.provider_id;
      return {
        providerId,
        providerLabel: labelForProviderId(providerId),
        modelLabel: config.selected_model.model_id,
      };
    }

    const enabledCustom = (config.custom ?? []).find((c) => c.enabled);
    const candidates = [
      { id: "openrouter", label: "OpenRouter", config: config.openrouter },
      { id: "opencode_zen", label: "Opencode Zen", config: config.opencode_zen },
      { id: "anthropic", label: "Anthropic", config: config.anthropic },
      { id: "openai", label: "OpenAI", config: config.openai },
      { id: "ollama", label: "Ollama", config: config.ollama },
      ...(enabledCustom ? [{ id: "custom", label: "Custom", config: enabledCustom }] : []),
    ];

    const preferred =
      candidates.find((c) => c.config.enabled && c.config.default) ||
      candidates.find((c) => c.config.enabled);

    if (!preferred) return null;
    return {
      providerId: preferred.id,
      providerLabel: preferred.label,
      modelLabel: preferred.config.model ?? null,
    };
  }, [state?.providers_config]);

  const activeProviderId = activeProviderInfo?.providerId || null;

  // Update view based on workspace state after loading
  const effectiveView =
    loading || !vaultUnlocked
      ? "sidecar-setup"
      : !sidecarReady
        ? "sidecar-setup"
        : (!state?.has_workspace || !hasConfiguredProvider) &&
            view !== "settings" &&
            view !== "about" &&
            view !== "packs" &&
            view !== "extensions" &&
            view !== "command-center"
          ? "onboarding"
          : view;

  const shouldShowWhatsNew =
    showWhatsNew &&
    sidecarReady &&
    !historyLoading &&
    !historyOverlayOpen &&
    !migrationOverlayOpen &&
    effectiveView === "chat";

  // Auto-open task sidebar when tasks are created (but not on initial load).
  // Only do this while in chat view.
  const previousTaskCountRef = useRef(0);
  useEffect(() => {
    const currentTaskCount = todosData.todos.length;

    // If tasks increased (new tasks created) and we're not already open
    if (
      effectiveView === "chat" &&
      currentTaskCount > previousTaskCountRef.current &&
      currentTaskCount > 0 &&
      !taskSidebarOpen
    ) {
      console.log(`[TaskSidebar] Auto-opening: ${currentTaskCount} tasks detected`);
      setTaskSidebarOpen(true);
    }

    previousTaskCountRef.current = currentTaskCount;
  }, [effectiveView, todosData.todos.length, taskSidebarOpen]);

  // Ensure the task sidebar is only visible in chat mode.
  useEffect(() => {
    if (effectiveView !== "chat" && taskSidebarOpen) {
      setTaskSidebarOpen(false);
    }
  }, [effectiveView, taskSidebarOpen]);

  // Check vault status and wait for unlock
  useEffect(() => {
    let cancelled = false;

    const checkVault = async () => {
      // Poll for vault unlock status
      while (!cancelled) {
        try {
          const status: VaultStatus = await getVaultStatus();
          if (status === "unlocked") {
            setVaultUnlocked(true);
            // Refresh state to get updated has_key status now that vault is unlocked
            await refreshAppState();
            return;
          }
          // Also check the global flag set by splash screen
          if (window.__vaultUnlocked) {
            setVaultUnlocked(true);
            // Refresh state to get updated has_key status now that vault is unlocked
            await refreshAppState();
            return;
          }
        } catch (e) {
          console.error("Failed to check vault status:", e);
        }
        // Wait a bit before checking again
        await new Promise((resolve) => setTimeout(resolve, 500));
      }
    };

    checkVault();

    return () => {
      cancelled = true;
    };
  }, [refreshAppState]);

  // Hide splash screen once vault is unlocked and app state is loaded
  useEffect(() => {
    if (!loading && vaultUnlocked) {
      hideSplashScreen();
    }
  }, [loading, vaultUnlocked]);

  // Load sessions and projects when sidecar is ready
  const loadHistory = useCallback(async () => {
    setHistoryLoading(true);
    if (historyOverlayDelayRef.current) {
      globalThis.clearTimeout(historyOverlayDelayRef.current);
      historyOverlayDelayRef.current = null;
    }
    historyOverlayDelayRef.current = globalThis.setTimeout(() => {
      setHistoryOverlayOpen(true);
    }, 300);
    try {
      // On initial app load, "sidecarReady" can be true before the engine is actually running.
      // If we query too early, the list calls fail and the UI stays empty until a manual refresh.
      const status = await getSidecarStatus();
      if (status !== "running") return;

      const [sessionsData, projectsData] = await Promise.all([listSessions(), listProjects()]);
      const activeWorkspacePath = activeProject?.path || state?.workspace_path || null;

      // Convert Session to SessionInfo format
      const sessionInfos: SessionInfo[] = sessionsData.map((s: Session) => ({
        id: s.id,
        slug: s.slug,
        version: s.version,
        projectID: s.projectID || activeProject?.id || "",
        directory: resolveSessionDirectory(s.directory, activeWorkspacePath),
        title: s.title || "New Chat",
        time: s.time || { created: Date.now(), updated: Date.now() },
        summary: s.summary,
      }));

      const matchedToWorkspace = sessionInfos.filter((session) =>
        sessionBelongsToWorkspace(session, activeWorkspacePath)
      ).length;
      console.info(
        "[SessionScope] Loaded sessions:",
        sessionInfos.length,
        "workspace matches:",
        matchedToWorkspace,
        "workspace:",
        activeWorkspacePath ?? "(none)"
      );

      setSessions(sessionInfos);
      setProjects(projectsData);
      const runChecks = await Promise.allSettled(
        sessionInfos.map(async (session) => {
          const activeRun = await getSessionActiveRun(session.id);
          return { sessionId: session.id, running: !!activeRun };
        })
      );
      const activeIds = new Set<string>();
      for (const result of runChecks) {
        if (result.status === "fulfilled" && result.value.running) {
          activeIds.add(result.value.sessionId);
        }
      }
      setRunningSessionIds(activeIds);
    } catch (e) {
      console.error("Failed to load history:", e);
    } finally {
      if (historyOverlayDelayRef.current) {
        globalThis.clearTimeout(historyOverlayDelayRef.current);
        historyOverlayDelayRef.current = null;
      }
      setHistoryOverlayOpen(false);
      setHistoryLoading(false);
    }
  }, [activeProject?.id, activeProject?.path, state?.workspace_path]);

  useEffect(() => {
    loadHistoryRef.current = loadHistory;
  }, [loadHistory]);

  useEffect(() => {
    return () => {
      if (historyOverlayDelayRef.current) {
        globalThis.clearTimeout(historyOverlayDelayRef.current);
        historyOverlayDelayRef.current = null;
      }
      if (historyRefreshDebounceRef.current) {
        globalThis.clearTimeout(historyRefreshDebounceRef.current);
        historyRefreshDebounceRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    loadHistory();
  }, [loadHistory]);

  useEffect(() => {
    if (sessions.length === 0) return;
    let cancelled = false;
    const reconcile = async () => {
      try {
        const checks = await Promise.allSettled(
          sessions.map(async (session) => {
            const activeRun = await getSessionActiveRun(session.id);
            return { sessionId: session.id, running: !!activeRun };
          })
        );
        if (cancelled) return;
        const activeIds = new Set<string>();
        for (const result of checks) {
          if (result.status === "fulfilled" && result.value.running) {
            activeIds.add(result.value.sessionId);
          }
        }
        setRunningSessionIds(activeIds);
      } catch (e) {
        console.warn("Session running-state reconcile failed:", e);
      }
    };
    const interval = setInterval(reconcile, 15000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [sessions]);

  const runMigration = useCallback(
    async (force = false) => {
      setMigrationOverlayOpen(true);
      setMigrationRunning(true);
      setMigrationProgress(null);
      setMigrationResult(null);

      let unlistenProgress: (() => void) | null = null;
      let unlistenComplete: (() => void) | null = null;
      try {
        unlistenProgress = await onStorageMigrationProgress((event) => {
          setMigrationProgress(event);
        });
        unlistenComplete = await onStorageMigrationComplete((result) => {
          setMigrationResult(result);
        });
        const result = await runStorageMigration({
          force,
          includeWorkspaceScan: true,
          dryRun: false,
        });
        setMigrationResult(result);
        await refreshAppState();
        await loadHistory();
      } catch (e) {
        console.error("Migration run failed:", e);
        setMigrationResult({
          status: "failed",
          started_at_ms: Date.now(),
          ended_at_ms: Date.now(),
          duration_ms: 0,
          sources_detected: [],
          copied: [],
          skipped: [],
          errors: [e instanceof Error ? e.message : String(e)],
          sessions_imported: 0,
          sessions_repaired: 0,
          messages_recovered: 0,
          parts_recovered: 0,
          conflicts_merged: 0,
          tool_rows_upserted: 0,
          report_path: "",
          reason: "migration_failed",
          dry_run: false,
        });
      } finally {
        setMigrationRunning(false);
        try {
          unlistenProgress?.();
          unlistenComplete?.();
        } catch {
          // ignore unlisten errors
        }
      }
    },
    [loadHistory, refreshAppState]
  );

  useEffect(() => {
    if (loading || !vaultUnlocked || migrationCheckedRef.current) {
      return;
    }
    migrationCheckedRef.current = true;
    let cancelled = false;
    const checkAndRun = async () => {
      try {
        const status = await getStorageMigrationStatus();
        if (cancelled) return;
        if (status.migration_needed) {
          await runMigration(false);
        }
      } catch (e) {
        console.warn("Storage migration status check failed:", e);
      }
    };
    void checkAndRun();
    return () => {
      cancelled = true;
    };
  }, [loading, vaultUnlocked, runMigration]);

  // Load user projects
  const loadUserProjects = useCallback(async () => {
    try {
      const [projects, active] = await Promise.all([getUserProjects(), getActiveProject()]);
      setUserProjects(projects);
      setActiveProjectState(active);
    } catch (e) {
      console.error("Failed to load user projects:", e);
    }
  }, []);

  // Check vault status and wait for unlock
  useEffect(() => {
    if (vaultUnlocked) {
      loadUserProjects();
    }
  }, [vaultUnlocked, loadUserProjects]);

  const handleSidecarReady = useCallback(() => {
    setSidecarReady(true);
    // Navigate to appropriate view
    if (state?.has_workspace) {
      // TODO(startup-routing): Revisit last-open-area restore once we have a proper
      // starter/boot page that can coordinate sidecar readiness checks for all views.
      // For now, always land in chat on startup to avoid command-center-first boot races.
      setView("chat");

      // If there's a saved session, trigger Chat to reload it now that sidecar is ready
      // We do this by briefly clearing and restoring the session ID
      if (currentSessionId) {
        // Check if this session ID belongs to an orchestrator run
        // If so, switch to orchestrator mode instead
        invoke<RunSummary[]>("orchestrator_list_runs")
          .then((runs) => {
            const run = runs.find((r) => r.session_id === currentSessionId);
            if (run) {
              if (run.source === "command_center") {
                setOrchestratorOpen(false);
                setCurrentOrchestratorRunId(null);
                setCommandCenterRunId(run.run_id);
                setView("command-center");
              } else {
                setCommandCenterRunId(null);
                setCurrentOrchestratorRunId(run.run_id);
                setOrchestratorOpen(true);
                setView("chat");
              }
              setCurrentSessionId(null); // Clear session ID as we're in orchestrator mode
            } else {
              const savedId = currentSessionId;
              setCurrentSessionId(null);
              // Use setTimeout to ensure state update is processed before restoring
              setTimeout(() => setCurrentSessionId(savedId), 50);
            }
          })
          .catch((e) => {
            console.error("Failed to check orchestrator runs:", e);
            // Fallback to chat logic
            const savedId = currentSessionId;
            setCurrentSessionId(null);
            setTimeout(() => setCurrentSessionId(savedId), 50);
          });
      }
    } else {
      setView("onboarding");
    }
  }, [state?.has_workspace, currentSessionId]);

  const handleSwitchProject = async (projectId: string) => {
    setProjectSwitcherLoading(true);

    // Clear current session FIRST to reset the chat view
    setCurrentSessionId(null);
    setCurrentOrchestratorRunId(null);
    setOrchestratorOpen(false);

    try {
      await setActiveProject(projectId);

      // Wait for sidecar to restart with polling
      let attempts = 0;
      const maxAttempts = 10;
      while (attempts < maxAttempts) {
        await new Promise((resolve) => setTimeout(resolve, 500));
        const sidecarStatus = await getSidecarStatus();
        if (sidecarStatus === "running") {
          console.log("[SwitchProject] Sidecar ready after", attempts + 1, "attempts");
          break;
        }
        attempts++;
      }

      // Reload everything
      await refreshAppState();
      await loadUserProjects();
      await loadHistory();
    } catch (e) {
      console.error("Failed to switch project:", e);
    } finally {
      setProjectSwitcherLoading(false);
    }
  };

  const beginAddProject = async (path: string) => {
    setError(null);

    try {
      // Check Git status with timeout
      // On macOS, git check can hang if it triggers the "install command line tools" prompt
      // We set a short timeout to prevent the UI from freezing
      try {
        const checkPromise = checkGitStatus(path);
        const timeoutPromise = new Promise<{
          git_installed: boolean;
          is_repo: boolean;
          can_enable_undo: boolean;
        }>((_, reject) => setTimeout(() => reject(new Error("Git check timed out")), 2000));

        const status = await Promise.race([checkPromise, timeoutPromise]);

        if (status.can_enable_undo) {
          // Git is installed but folder isn't a repo - prompt user
          setPendingProjectPath(path);
          setGitStatus(status);
          setShowGitDialog(true);
          return; // Wait for dialog response
        } else if (!status.git_installed) {
          // Git not installed - show warning but allow continuing
          setPendingProjectPath(path);
          setGitStatus(status);
          setShowGitDialog(true);
          return;
        }
      } catch (e) {
        console.warn("Git check failed or timed out:", e);
        // If git check fails or times out, proceed without git features
        // This prevents onboarding from getting stuck
      }

      // Git is already set up, user doesn't want it, or check failed - proceed
      await finalizeAddProject(path);
    } catch (e) {
      console.error("Failed to add project:", e);
      setError(e instanceof Error ? e.message : "Failed to add project");
    }
  };

  const handleAddProject = async () => {
    // Clear current session FIRST to reset the chat view
    setCurrentSessionId(null);
    setCurrentOrchestratorRunId(null);
    setOrchestratorOpen(false);
    setError(null);

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Folder",
      });

      if (selected && typeof selected === "string") await beginAddProject(selected);
    } catch (e) {
      console.error("Failed to add project:", e);
      setError(e instanceof Error ? e.message : "Failed to add project");
    }
  };

  // New helper function to complete project addition
  const finalizeAddProject = async (path: string) => {
    setError(null);
    try {
      setProjectSwitcherLoading(true);
      setCurrentOrchestratorRunId(null);
      setOrchestratorOpen(false);
      const project = await addProject(path);
      await setActiveProject(project.id);

      // Wait for sidecar to restart with polling
      let attempts = 0;
      const maxAttempts = 10;
      while (attempts < maxAttempts) {
        await new Promise((resolve) => setTimeout(resolve, 500));
        const sidecarStatus = await getSidecarStatus();
        if (sidecarStatus === "running") {
          console.log("[AddProject] Sidecar ready after", attempts + 1, "attempts");
          break;
        }
        attempts++;
      }

      // Reload everything
      await refreshAppState();
      await loadUserProjects();
      await loadHistory();

      if (postAddProjectView) {
        setView(postAddProjectView);
        setPostAddProjectView(null);
      }
    } catch (e) {
      console.error("Failed to finalize project:", e);
      setError(e instanceof Error ? e.message : "Failed to setup project");
    } finally {
      setProjectSwitcherLoading(false);
    }
  };

  // Handle Git initialization from dialog
  const handleGitInitialize = async () => {
    if (!pendingProjectPath) return;

    try {
      await initializeGitRepo(pendingProjectPath);
      console.log("Git initialized successfully");
    } catch (e) {
      console.error("Failed to initialize Git:", e);
    }

    setShowGitDialog(false);
    await finalizeAddProject(pendingProjectPath);
    setPendingProjectPath(null);
    setGitStatus(null);
  };

  // Handle skipping Git initialization
  const handleGitSkip = async () => {
    if (!pendingProjectPath) return;

    setShowGitDialog(false);
    await finalizeAddProject(pendingProjectPath);
    setPendingProjectPath(null);
    setGitStatus(null);
  };

  const handleManageProjects = () => {
    setView("settings");
  };

  const handleOpenPacks = () => {
    setView("packs");
  };

  const handleOpenInstalledPack = async (installedPath: string) => {
    setDraftMessage("Open `START_HERE.md` and follow it step-by-step.");
    setPostAddProjectView("chat");
    setSidebarTab("sessions");
    setSidebarOpen(true);
    await beginAddProject(installedPath);
  };

  const handleStartModeBuilderChat = (seedPrompt: string) => {
    setDraftMessage(seedPrompt);
    setExtensionsInitialTab("modes");
    setView("chat");
    setSidebarTab("sessions");
    setSidebarOpen(true);
  };

  const handleSettingsClose = async () => {
    setView("chat");
    // Reload everything when settings closes (in case projects were modified)
    await refreshAppState();
    await loadUserProjects();
    await loadHistory();
  };

  const handleSelectSession = (sessionId: string) => {
    const run = orchestratorRuns.find((r) => r.session_id === sessionId);
    if (run) {
      setCurrentSessionId(null);
      if (run.source === "command_center") {
        setCurrentOrchestratorRunId(null);
        setCommandCenterRunId(run.run_id);
        setOrchestratorOpen(false);
        setView("command-center");
      } else {
        setCommandCenterRunId(null);
        setCurrentOrchestratorRunId(run.run_id);
        setOrchestratorOpen(true);
        setView("chat");
      }
      return;
    }
    setView("chat");
    setCommandCenterRunId(null);
    setOrchestratorOpen(false);
    setCurrentOrchestratorRunId(null);
    // If the user clicks the already-selected session, React won't emit a state change,
    // and Chat won't reload history. Force a reload by briefly clearing then restoring.
    if (sessionId === currentSessionId) {
      setCurrentSessionId(null);
      setTimeout(() => setCurrentSessionId(sessionId), 50);
      return;
    }
    setCurrentSessionId(sessionId);
  };

  const handleNewChat = () => {
    skipOrchestratorAutoResumeRef.current = true;
    setOrchestratorOpen(false);
    setCurrentOrchestratorRunId(null);
    setCommandCenterRunId(null);
    setCurrentSessionId(null);
    setView("chat");
  };

  const handleDeleteSession = async (sessionId: string) => {
    console.log("[App] Deleting session:", sessionId);
    try {
      await deleteSession(sessionId);
      console.log("[App] Session deleted successfully");
      setSessions((prev) => prev.filter((s) => s.id !== sessionId));
      if (currentSessionId === sessionId) {
        setCurrentSessionId(null);
      }
    } catch (e) {
      console.error("Failed to delete session:", e);
    }
  };

  const handleDeleteOrchestratorRun = async (runId: string) => {
    console.log("[App] Deleting orchestrator run:", runId);
    const run = orchestratorRuns.find((r) => r.run_id === runId);
    try {
      await deleteOrchestratorRun(runId);
      setOrchestratorRuns((prev) => prev.filter((r) => r.run_id !== runId));

      // Remove the corresponding sidecar session from local state so it doesn't reappear as a chat
      // item until the next history refresh.
      if (run?.session_id) {
        setSessions((prev) => prev.filter((s) => s.id !== run.session_id));
        if (currentSessionId === run.session_id) {
          setCurrentSessionId(null);
        }
      }

      if (currentOrchestratorRunId === runId) {
        setCurrentOrchestratorRunId(null);
      }
      if (commandCenterRunId === runId) {
        setCommandCenterRunId(null);
      }
    } catch (e) {
      console.error("Failed to delete orchestrator run:", e);
    }
  };

  const handleExecutePendingTasks = () => {
    // Switch to immediate mode and trigger execution in Chat
    setUsePlanMode(false);
    setSelectedAgent(undefined);
    setExecutePendingTrigger((prev) => prev + 1);
  };

  // Handle agent selection changes - open orchestrator panel when orchestrate is selected
  const handleAgentChange = (agent: string | undefined) => {
    setSelectedAgent(agent);
    if (agent === "orchestrate") {
      skipOrchestratorAutoResumeRef.current = true;
      setCurrentSessionId(null);
      setCurrentOrchestratorRunId(null);
      setOrchestratorOpen(true);
    }
  };

  const handleAddFileToChat = async (file: FileEntry) => {
    // Read file content and create attachment
    try {
      console.log("Add to chat:", file.path);

      // Helper to detect binary files (images and PDFs)
      const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico"]);
      const EXTRACTABLE_EXTENSIONS = new Set([
        "pdf",
        "docx",
        "pptx",
        "xlsx",
        "xls",
        "ods",
        "xlsb",
        "rtf",
      ]);
      const isImage = file.extension && IMAGE_EXTENSIONS.has(file.extension.toLowerCase());
      const extLower = file.extension?.toLowerCase();
      const isExtractable = !!extLower && EXTRACTABLE_EXTENSIONS.has(extLower);
      const isBinary = isImage; // PDFs/docs are handled via Rust text extraction when possible

      // Use standard MIME types
      const getMimeType = (ext: string | undefined): string => {
        switch (ext?.toLowerCase()) {
          // Images
          case "png":
            return "image/png";
          case "jpg":
          case "jpeg":
            return "image/jpeg";
          case "gif":
            return "image/gif";
          case "svg":
            return "image/svg+xml";
          case "webp":
            return "image/webp";
          case "bmp":
            return "image/bmp";
          case "ico":
            return "image/x-icon";
          // PDF
          case "pdf":
            return "application/pdf";
          // Text files
          case "ts":
          case "tsx":
            return "text/typescript";
          case "js":
          case "jsx":
            return "text/javascript";
          case "json":
            return "application/json";
          case "md":
            return "text/markdown";
          case "txt":
            return "text/plain";
          case "log":
            return "text/plain";
          case "html":
            return "text/html";
          default:
            return "text/plain";
        }
      };

      const mimeType = getMimeType(file.extension);
      let base64Content: string;
      let size: number;

      if (isBinary) {
        // Read binary file (images, PDFs) directly as base64
        base64Content = await readBinaryFile(file.path);
        // Estimate size from base64 (base64 is ~33% larger than original)
        size = Math.floor((base64Content.length * 3) / 4);
      } else {
        // Read text-like content. For common document formats (PDF/DOCX/XLSX/etc),
        // extract plain text on the Rust side so the AI can actually use it.
        const content = isExtractable
          ? await readFileText(file.path, 25 * 1024 * 1024, 200_000)
          : await readFileContent(file.path, 1024 * 1024); // 1MB limit
        // Encode text content to base64 using browser's btoa

        base64Content = window.btoa(unescape(encodeURIComponent(content)));
        size = content.length;
      }

      const dataUrl = `data:${mimeType};base64,${base64Content}`;

      // Create a FileAttachment with data URL - omit filename to force data URL usage
      const attachment: FileAttachment = {
        id: `file_${Date.now()}`,
        type: "file",
        name: file.name,
        mime: mimeType,
        url: dataUrl, // Use data URL instead of file path
        size: size,
      };

      setFileToAttach(attachment);
      setSelectedFile(null); // Close preview
    } catch (err) {
      console.error("Failed to add file to chat:", err);
    }
  };

  const openFilePreview = useCallback((file: FileEntry) => {
    setSelectedFile(file);
    setSidebarTab("files");
  }, []);

  const openFilePreviewFromPath = useCallback(
    (filePath: string) => {
      const workspacePath = activeProject?.path || state?.workspace_path;
      let absolutePath = filePath;

      if (workspacePath && !filePath.match(/^([a-zA-Z]:[\\/]|\/)/)) {
        absolutePath = `${workspacePath}/${filePath}`.replace(/\\/g, "/");
      }

      const fileName = absolutePath.split(/[\\/]/).pop() || absolutePath;
      openFilePreview({
        path: absolutePath,
        name: fileName,
        is_directory: false,
        extension: fileName.includes(".") ? fileName.split(".").pop() : undefined,
      });
    },
    [activeProject?.path, state?.workspace_path, openFilePreview]
  );

  const visibleChatSessionIds = useMemo(() => {
    const runBaseSessionIds = new Set(orchestratorRuns.map((r) => r.session_id));
    return new Set(
      sessions
        .filter((s) => {
          if (runBaseSessionIds.has(s.id) && s.title?.startsWith("Orchestrator Run:")) return false;
          if (s.title?.startsWith("Orchestrator Task ")) return false;
          if (s.title?.startsWith("Orchestrator Resume:")) return false;
          return true;
        })
        .map((s) => s.id)
    );
  }, [sessions, orchestratorRuns]);

  const activeOrchestrationCount = useMemo(
    () =>
      orchestratorRuns.filter((run) => run.status === "planning" || run.status === "executing")
        .length,
    [orchestratorRuns]
  );
  const currentOrchestratorRunSessionId = useMemo(
    () =>
      currentOrchestratorRunId
        ? (orchestratorRuns.find((run) => run.run_id === currentOrchestratorRunId)?.session_id ??
          null)
        : null,
    [orchestratorRuns, currentOrchestratorRunId]
  );
  const activeChatRunningCount = useMemo(
    () => Array.from(runningSessionIds).filter((sid) => visibleChatSessionIds.has(sid)).length,
    [runningSessionIds, visibleChatSessionIds]
  );

  return (
    <div className="h-screen w-screen app-background">
      <div className="custom-bg-layer" aria-hidden="true" />
      <div className="app-shell flex h-screen">
        {/* Icon Sidebar */}
        {effectiveView !== "onboarding" && effectiveView !== "sidecar-setup" && (
          <motion.aside
            className="flex w-16 flex-col items-center border-r border-border bg-surface py-4 z-20"
            initial={{ x: -64 }}
            animate={{ x: 0 }}
            transition={{ duration: 0.3 }}
          >
            {/* Brand mark */}
            <div className="mb-8">
              <BrandMark size="md" />
            </div>

            {/* Navigation */}
            <nav className="flex flex-1 flex-col items-center gap-2">
              {/* Toggle sidebar button */}
              <button
                onClick={() => setSidebarOpen(!sidebarOpen)}
                className="flex h-10 w-10 items-center justify-center rounded-lg text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                title={
                  sidebarOpen
                    ? t("history.hide", { ns: "common" })
                    : t("history.show", { ns: "common" })
                }
              >
                {sidebarOpen ? (
                  <PanelLeftClose className="h-5 w-5" />
                ) : (
                  <PanelLeft className="h-5 w-5" />
                )}
              </button>

              <button
                onClick={() => setView("chat")}
                className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
                  effectiveView === "chat"
                    ? "bg-primary/20 text-primary"
                    : "text-text-muted hover:bg-surface-elevated hover:text-text"
                }`}
                title={t("navigation.chat", { ns: "common" })}
              >
                <MessageSquare className="h-5 w-5" />
              </button>
              <button
                onClick={() => setView("command-center")}
                className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
                  effectiveView === "command-center"
                    ? "bg-primary/20 text-primary"
                    : "text-text-muted hover:bg-surface-elevated hover:text-text"
                }`}
                title="Command Center (beta)"
              >
                <Rocket className="h-5 w-5" />
              </button>
              <button
                onClick={handleOpenPacks}
                className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
                  effectiveView === "packs"
                    ? "bg-primary/20 text-primary"
                    : "text-text-muted hover:bg-surface-elevated hover:text-text"
                }`}
                title={t("navigation.starterPacks", { ns: "common" })}
              >
                <Sparkles className="h-5 w-5" />
              </button>
              <button
                onClick={() => setView("extensions")}
                className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
                  effectiveView === "extensions"
                    ? "bg-primary/20 text-primary"
                    : "text-text-muted hover:bg-surface-elevated hover:text-text"
                }`}
                title={t("extensions.title", { ns: "common" })}
              >
                <Blocks className="h-5 w-5" />
              </button>
              <button
                onClick={() => setView("settings")}
                className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
                  effectiveView === "settings"
                    ? "bg-primary/20 text-primary"
                    : "text-text-muted hover:bg-surface-elevated hover:text-text"
                }`}
                title={t("title", { ns: "settings" })}
              >
                <SettingsIcon className="h-5 w-5" />
              </button>

              {/* Theme quick toggle */}
              <button
                onClick={() => cycleTheme()}
                className="flex h-10 w-10 items-center justify-center rounded-lg text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                title={t("theme.switch", { ns: "common" })}
              >
                <Palette className="h-5 w-5" />
              </button>
              <button
                onClick={() => setView("about")}
                className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
                  effectiveView === "about"
                    ? "bg-primary/20 text-primary"
                    : "text-text-muted hover:bg-surface-elevated hover:text-text"
                }`}
                title={t("navigation.about", { ns: "common" })}
              >
                <Info className="h-5 w-5" />
              </button>

              {/* Task sidebar toggle - only visible in chat */}
              {effectiveView === "chat" && (usePlanMode || todosData.todos.length > 0) && (
                <button
                  onClick={() => setTaskSidebarOpen(!taskSidebarOpen)}
                  className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
                    taskSidebarOpen
                      ? "bg-primary/20 text-primary"
                      : "text-text-muted hover:bg-surface-elevated hover:text-text"
                  }`}
                  title={t("navigation.tasks", { ns: "common" })}
                >
                  <ListTodo className="h-5 w-5" />
                  {todosData.todos.length > 0 && (
                    <span className="absolute top-1 right-1 h-2 w-2 rounded-full bg-primary" />
                  )}
                </button>
              )}
            </nav>

            {/* Security indicator */}
            <div className="mt-auto" title={t("security.zeroTrustEnabled", { ns: "common" })}>
              <Shield className="h-4 w-4 text-success" />
            </div>
          </motion.aside>
        )}

        {/* Tabbed Sidebar (Sessions / Files) */}
        {effectiveView === "chat" && (
          <motion.div
            className="flex h-full flex-col border-r border-border bg-surface z-10 overflow-hidden"
            initial={false}
            animate={{ width: sidebarOpen ? 320 : 0, opacity: sidebarOpen ? 1 : 0 }}
            transition={{ duration: 0.25, ease: "easeOut" }}
            style={{ pointerEvents: sidebarOpen ? "auto" : "none" }}
          >
            {/* Tab Switcher */}
            <div className="flex border-b border-border">
              <button
                onClick={() => setSidebarTab("sessions")}
                className={cn(
                  "flex-1 px-4 py-3 text-sm font-medium transition-colors flex items-center justify-center gap-2",
                  sidebarTab === "sessions"
                    ? "border-b-2 border-primary text-primary"
                    : "text-text-muted hover:text-text hover:bg-surface-elevated"
                )}
              >
                <MessageSquare className="h-4 w-4" />
                {t("navigation.sessions", { ns: "common" })}
              </button>
              <button
                onClick={() => setSidebarTab("files")}
                className={cn(
                  "flex-1 px-4 py-3 text-sm font-medium transition-colors flex items-center justify-center gap-2",
                  sidebarTab === "files"
                    ? "border-b-2 border-primary text-primary"
                    : "text-text-muted hover:text-text hover:bg-surface-elevated"
                )}
              >
                <Files className="h-4 w-4" />
                {t("navigation.files", { ns: "common" })}
              </button>
            </div>

            {/* Tab Content */}
            <div className="flex-1 overflow-hidden flex flex-col">
              {sidebarTab === "sessions" ? (
                <>
                  {/* Unified Sessions List */}
                  <div className="flex-1 overflow-hidden">
                    <SessionSidebar
                      isOpen={true}
                      onToggle={() => setSidebarOpen(!sidebarOpen)}
                      sessions={sessions.filter((session) =>
                        sessionBelongsToWorkspace(session, activeProject?.path || null)
                      )}
                      runs={orchestratorRuns}
                      projects={projects}
                      currentSessionId={currentSessionId}
                      currentRunId={currentOrchestratorRunId}
                      currentCommandCenterRunId={commandCenterRunId}
                      activeChatSessionIds={Array.from(runningSessionIds)}
                      onSelectSession={handleSelectSession}
                      onSelectRun={(runId, runType) => {
                        if (runType === "command-center") {
                          setCurrentOrchestratorRunId(null);
                          setCommandCenterRunId(runId);
                          setOrchestratorOpen(false);
                          setView("command-center");
                          return;
                        }
                        setCommandCenterRunId(null);
                        setCurrentOrchestratorRunId(runId);
                        setOrchestratorOpen(true);
                        setView("chat");
                      }}
                      onNewChat={handleNewChat}
                      onOpenPacks={() => setView("packs")}
                      onDeleteSession={handleDeleteSession}
                      onDeleteRun={handleDeleteOrchestratorRun}
                      isLoading={historyLoading}
                      userProjects={userProjects}
                      activeProject={activeProject}
                      onSwitchProject={handleSwitchProject}
                      onAddProject={handleAddProject}
                      onManageProjects={handleManageProjects}
                      projectSwitcherLoading={projectSwitcherLoading}
                    />
                  </div>
                </>
              ) : (
                <FileBrowser
                  rootPath={activeProject?.path || null}
                  onFileSelect={(file) => setSelectedFile(file)}
                  selectedPath={selectedFile?.path}
                />
              )}
            </div>
          </motion.div>
        )}

        {/* Main Content */}
        <main className="flex-1 overflow-hidden relative flex">
          {effectiveView === "sidecar-setup" ? (
            <motion.div
              key="sidecar-setup"
              className="flex h-full w-full items-center justify-center app-background"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.8, ease: "easeOut" }}
            >
              <SidecarDownloader onComplete={handleSidecarReady} />
            </motion.div>
          ) : effectiveView === "onboarding" ? (
            <OnboardingWizard
              hasConfiguredProvider={hasConfiguredProvider}
              hasWorkspace={!!state?.has_workspace}
              error={error}
              onChooseFolder={handleAddProject}
              onOpenProviders={() => {
                setSettingsInitialSection("providers");
                setView("settings");
              }}
              onBrowsePacks={() => setView("packs")}
              onSkip={() => {
                setSettingsInitialSection("projects");
                setView("settings");
              }}
            />
          ) : effectiveView === "packs" ? (
            <PacksPanel
              activeProjectPath={activeProject?.path || state?.workspace_path || undefined}
              onOpenInstalledPack={handleOpenInstalledPack}
              onOpenSkills={() => {
                setExtensionsInitialTab("skills");
                setView("extensions");
              }}
            />
          ) : effectiveView === "settings" ? (
            <motion.div
              key="settings"
              className="h-full w-full app-background"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.3, ease: "easeOut" }}
            >
              <Settings
                onClose={handleSettingsClose}
                onProjectChange={loadUserProjects}
                onProviderChange={refreshAppState}
                initialSection={settingsInitialSection ?? undefined}
                onInitialSectionConsumed={() => setSettingsInitialSection(null)}
              />
            </motion.div>
          ) : effectiveView === "extensions" ? (
            <motion.div
              key="extensions"
              className="h-full w-full app-background"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.3, ease: "easeOut" }}
            >
              <Extensions
                workspacePath={activeProject?.path || state?.workspace_path || null}
                initialTab={extensionsInitialTab ?? undefined}
                onInitialTabConsumed={() => setExtensionsInitialTab(null)}
                onClose={() => setView("chat")}
                onStartModeBuilderChat={handleStartModeBuilderChat}
              />
            </motion.div>
          ) : effectiveView === "about" ? (
            <motion.div
              key="about"
              className="h-full w-full app-background relative"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.3, ease: "easeOut" }}
            >
              <div className="absolute right-4 top-4 z-10">
                <button
                  type="button"
                  onClick={() => setView("chat")}
                  className="rounded-lg border border-border bg-surface/70 px-3 py-2 text-sm text-text transition-colors hover:bg-surface-elevated"
                >
                  {t("actions.close", { ns: "common" })}
                </button>
              </div>
              <About />
            </motion.div>
          ) : effectiveView === "command-center" ? (
            <motion.div
              key="command-center"
              className="h-full w-full app-background"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2, ease: "easeOut" }}
            >
              <div className={cn("h-full w-full flex", selectedFile && "gap-0")}>
                <div className={cn("flex-1 min-w-0", selectedFile && "w-1/2")}>
                  <CommandCenterPage
                    userProjects={userProjects}
                    activeProject={activeProject}
                    onSwitchProject={handleSwitchProject}
                    onAddProject={handleAddProject}
                    onManageProjects={handleManageProjects}
                    onFileOpen={openFilePreview}
                    projectSwitcherLoading={projectSwitcherLoading}
                    initialRunId={commandCenterRunId}
                  />
                </div>
                <AnimatePresence>
                  {selectedFile && (
                    <motion.div
                      initial={{ width: 0, opacity: 0 }}
                      animate={{ width: "50%", opacity: 1 }}
                      exit={{ width: 0, opacity: 0 }}
                      transition={{ duration: 0.3 }}
                      className="overflow-hidden"
                    >
                      <div ref={setFilePreviewDockEl} className="h-full" />
                      {filePreviewDockEl && (
                        <FilePreview
                          file={selectedFile}
                          dockEl={filePreviewDockEl}
                          onClose={() => setSelectedFile(null)}
                          onAddToChat={handleAddFileToChat}
                        />
                      )}
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </motion.div>
          ) : (
            <>
              {/* Chat Area */}
              <div className={cn("flex-1 overflow-hidden relative", selectedFile && "w-1/2")}>
                {orchestratorOpen ? (
                  // Orchestrator as main view
                  <OrchestratorPanel
                    runId={currentOrchestratorRunId}
                    runSessionIdHint={currentOrchestratorRunSessionId}
                    onClose={() => {
                      setOrchestratorOpen(false);
                      // Reset to default agent when closing
                      setSelectedAgent(undefined);
                    }}
                  />
                ) : (
                  // Chat view
                  <Chat
                    key={activeProject?.id || "no-project"}
                    workspacePath={activeProject?.path || state?.workspace_path || null}
                    sessionId={currentSessionId}
                    onSessionCreated={async (id) => {
                      setCurrentSessionId(id);
                      await loadHistory();
                    }}
                    onSidecarConnected={() => {
                      // Ensure we load history after the engine is actually running (especially on startup).
                      setSidecarReady(true);
                      void loadHistory();
                    }}
                    usePlanMode={usePlanMode}
                    onPlanModeChange={setUsePlanMode}
                    onToggleTaskSidebar={() => setTaskSidebarOpen(!taskSidebarOpen)}
                    executePendingTasksTrigger={executePendingTrigger}
                    onGeneratingChange={setIsExecutingTasks}
                    pendingTasks={todosData.todos}
                    fileToAttach={fileToAttach || undefined}
                    onFileAttached={() => setFileToAttach(null)}
                    selectedAgent={selectedAgent}
                    onAgentChange={handleAgentChange}
                    hasConfiguredProvider={hasConfiguredProvider}
                    activeProviderId={activeProviderId || undefined}
                    activeProviderLabel={activeProviderInfo?.providerLabel || undefined}
                    activeModelLabel={activeProviderInfo?.modelLabel || undefined}
                    onOpenSettings={() => setView("settings")}
                    onOpenPacks={() => setView("packs")}
                    onOpenExtensions={(tab) => {
                      setExtensionsInitialTab(tab ?? "skills");
                      setView("extensions");
                    }}
                    onProviderChange={refreshAppState}
                    draftMessage={draftMessage ?? undefined}
                    onDraftMessageConsumed={() => setDraftMessage(null)}
                    activeChatRunningCount={activeChatRunningCount}
                    activeOrchestrationCount={activeOrchestrationCount}
                    onFileOpen={(filePath) => openFilePreviewFromPath(filePath)}
                  />
                )}
              </div>

              {/* File Preview Panel */}
              <AnimatePresence>
                {selectedFile && (
                  <motion.div
                    initial={{ width: 0, opacity: 0 }}
                    animate={{ width: "50%", opacity: 1 }}
                    exit={{ width: 0, opacity: 0 }}
                    transition={{ duration: 0.3 }}
                    className="overflow-hidden"
                  >
                    <div ref={setFilePreviewDockEl} className="h-full" />
                    {filePreviewDockEl && (
                      <FilePreview
                        file={selectedFile}
                        dockEl={filePreviewDockEl}
                        onClose={() => setSelectedFile(null)}
                        onAddToChat={handleAddFileToChat}
                      />
                    )}
                  </motion.div>
                )}
              </AnimatePresence>

              {/* Task Sidebar - only show in chat mode */}
              {!orchestratorOpen && (
                <TaskSidebar
                  isOpen={taskSidebarOpen}
                  onClose={() => setTaskSidebarOpen(false)}
                  todos={todosData.todos}
                  pending={todosData.pending}
                  inProgress={todosData.inProgress}
                  completed={todosData.completed}
                  isLoading={todosData.isLoading}
                  onExecutePending={handleExecutePendingTasks}
                  isExecuting={isExecutingTasks}
                />
              )}
            </>
          )}
        </main>
        <GitInitDialog
          isOpen={showGitDialog}
          onClose={handleGitSkip}
          onInitialize={handleGitInitialize}
          gitInstalled={gitStatus?.git_installed ?? false}
          folderPath={pendingProjectPath ?? ""}
        />
        <AppUpdateOverlay />
        <WhatsNewOverlay
          open={shouldShowWhatsNew}
          version={WHATS_NEW_VERSION}
          markdown={whatsNewMarkdown}
          onClose={dismissWhatsNew}
        />
        <StorageMigrationOverlay
          open={migrationOverlayOpen}
          running={migrationRunning}
          progress={migrationProgress}
          result={migrationResult}
          onContinue={() => setMigrationOverlayOpen(false)}
          onRetry={() => void runMigration(true)}
          onViewDetails={() => {
            if (!migrationResult) return;
            const details = [
              `Status: ${migrationResult.status}`,
              `Reason: ${migrationResult.reason}`,
              `Copied: ${migrationResult.copied.length}`,
              `Skipped: ${migrationResult.skipped.length}`,
              `Errors: ${migrationResult.errors.length}`,
              `Report: ${migrationResult.report_path || "n/a"}`,
            ].join("\n");
            window.alert(details);
          }}
        />
        <AnimatePresence>
          {historyOverlayOpen && !migrationOverlayOpen && (
            <motion.div
              className="fixed inset-0 z-[70] flex items-center justify-center bg-black/55 backdrop-blur-sm"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <motion.div
                className="w-[min(560px,90vw)] rounded-2xl border border-primary/25 bg-surface-elevated/95 p-6 shadow-2xl"
                initial={{ scale: 0.97, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0.98, opacity: 0 }}
              >
                <div className="mb-3 flex items-center gap-3">
                  <div className="flex h-10 w-10 items-center justify-center rounded-xl border border-primary/30 bg-primary/10">
                    <Loader2 className="h-5 w-5 animate-spin text-primary" />
                  </div>
                  <div>
                    <h3 className="text-base font-semibold text-text">
                      {t("history.syncingTitle", { ns: "common" })}
                    </h3>
                    <p className="text-sm text-text-muted">
                      {t("history.syncingBody", { ns: "common" })}
                    </p>
                  </div>
                </div>
                <div className="h-2 w-full overflow-hidden rounded-full bg-surface">
                  <motion.div
                    className="h-full bg-gradient-to-r from-primary via-secondary to-primary"
                    initial={{ x: "-35%", width: "35%" }}
                    animate={{ x: "130%" }}
                    transition={{ duration: 1.1, repeat: Infinity, ease: "linear" }}
                  />
                </div>
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

export default App;
