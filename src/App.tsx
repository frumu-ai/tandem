import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Settings } from "@/components/settings";
import { About } from "@/components/about";
import { Chat } from "@/components/chat";
import { SidecarDownloader } from "@/components/sidecar";
import { SessionSidebar, type SessionInfo, type Project } from "@/components/sidebar";
import { TaskSidebar } from "@/components/tasks/TaskSidebar";
import { FileBrowser } from "@/components/files/FileBrowser";
import { FilePreview } from "@/components/files/FilePreview";
import { GitInitDialog } from "@/components/dialogs/GitInitDialog";
import { Button } from "@/components/ui";
import { UpdateToast } from "@/components/ui/UpdateToast";
import { useAppState } from "@/hooks/useAppState";
import { useTheme } from "@/hooks/useTheme";
import { useTodos } from "@/hooks/useTodos";
import { cn } from "@/lib/utils";
import { BrandMark } from "@/components/ui/BrandMark";
import {
  listSessions,
  listProjects,
  deleteSession,
  getVaultStatus,
  getUserProjects,
  getActiveProject,
  setActiveProject,
  addProject,
  getSidecarStatus,
  readFileContent,
  readBinaryFile,
  checkGitStatus,
  initializeGitRepo,
  type Session,
  type VaultStatus,
  type UserProject,
  type FileEntry,
} from "@/lib/tauri";
import { type FileAttachment } from "@/components/chat/ChatInput";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Settings as SettingsIcon,
  MessageSquare,
  FolderOpen,
  Shield,
  PanelLeftClose,
  PanelLeft,
  Info,
  ListTodo,
  Files,
  Palette,
  AlertCircle,
} from "lucide-react";

type View = "chat" | "settings" | "about" | "onboarding" | "sidecar-setup";

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
  const { state, loading, refresh: refreshAppState } = useAppState();
  const { cycleTheme } = useTheme();
  const [sidecarReady, setSidecarReady] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [taskSidebarOpen, setTaskSidebarOpen] = useState(false);
  const [usePlanMode, setUsePlanMode] = useState(false);
  const [selectedAgent, setSelectedAgent] = useState<string | undefined>(undefined);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [vaultUnlocked, setVaultUnlocked] = useState(false);
  const [executePendingTrigger, setExecutePendingTrigger] = useState(0);
  const [isExecutingTasks, setIsExecutingTasks] = useState(false);

  // File browser state
  const [sidebarTab, setSidebarTab] = useState<"sessions" | "files">("sessions");
  const [selectedFile, setSelectedFile] = useState<FileEntry | null>(null);
  const [fileToAttach, setFileToAttach] = useState<FileAttachment | null>(null);

  // Project management state
  const [userProjects, setUserProjects] = useState<UserProject[]>([]);
  const [activeProject, setActiveProjectState] = useState<UserProject | null>(null);
  const [projectSwitcherLoading, setProjectSwitcherLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Git initialization dialog state
  const [showGitDialog, setShowGitDialog] = useState(false);
  const [pendingProjectPath, setPendingProjectPath] = useState<string | null>(null);
  const [gitStatus, setGitStatus] = useState<{ git_installed: boolean; is_repo: boolean } | null>(
    null
  );

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
      (state.providers_config.opencode_zen?.enabled &&
        state.providers_config.opencode_zen?.has_key) ||
      (state.providers_config.ollama?.enabled && state.providers_config.ollama?.has_key));

  // Debug: Log provider state
  console.log(
    "[App] hasConfiguredProvider:",
    hasConfiguredProvider,
    "state:",
    state?.providers_config
  );

  const activeProviderInfo = useMemo(() => {
    const config = state?.providers_config;
    if (!config) return null;

    const candidates = [
      { id: "openrouter", label: "OpenRouter", config: config.openrouter },
      { id: "opencode_zen", label: "OpenCode Zen", config: config.opencode_zen },
      { id: "anthropic", label: "Anthropic", config: config.anthropic },
      { id: "openai", label: "OpenAI", config: config.openai },
      { id: "ollama", label: "Ollama", config: config.ollama },
    ];

    const preferred =
      candidates.find((c) => c.config.enabled && c.config.default) ||
      candidates.find((c) => c.config.enabled);

    if (!preferred) return null;
    return {
      providerLabel: preferred.label,
      modelLabel: preferred.config.model ?? null,
    };
  }, [state?.providers_config]);

  // Update view based on workspace state after loading
  const effectiveView =
    loading || !vaultUnlocked
      ? "sidecar-setup"
      : !sidecarReady
        ? "sidecar-setup"
        : (!state?.has_workspace || !hasConfiguredProvider) &&
            view !== "settings" &&
            view !== "about"
          ? "onboarding"
          : view;

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
    if (!sidecarReady) return;

    setHistoryLoading(true);
    try {
      const [sessionsData, projectsData] = await Promise.all([listSessions(), listProjects()]);

      // Convert Session to SessionInfo format
      const sessionInfos: SessionInfo[] = sessionsData.map((s: Session) => ({
        id: s.id,
        slug: s.slug,
        version: s.version,
        projectID: s.projectID || "",
        directory: s.directory || "",
        title: s.title || "New Chat",
        time: s.time || { created: Date.now(), updated: Date.now() },
        summary: s.summary,
      }));

      setSessions(sessionInfos);
      setProjects(projectsData);
    } catch (e) {
      console.error("Failed to load history:", e);
    } finally {
      setHistoryLoading(false);
    }
  }, [sidecarReady]);

  useEffect(() => {
    loadHistory();
  }, [loadHistory]);

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
      setView("chat");
    } else {
      setView("onboarding");
    }
  }, [state?.has_workspace]);

  const handleSwitchProject = async (projectId: string) => {
    setProjectSwitcherLoading(true);

    // Clear current session FIRST to reset the chat view
    setCurrentSessionId(null);

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

  const handleAddProject = async () => {
    // Clear current session FIRST to reset the chat view
    setCurrentSessionId(null);
    setError(null);

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Project Folder",
      });

      if (selected && typeof selected === "string") {
        // Check Git status with timeout
        // On macOS, git check can hang if it triggers the "install command line tools" prompt
        // We set a short timeout to prevent the UI from freezing
        try {
          const checkPromise = checkGitStatus(selected);
          const timeoutPromise = new Promise<{
            git_installed: boolean;
            is_repo: boolean;
            can_enable_undo: boolean;
          }>((_, reject) => setTimeout(() => reject(new Error("Git check timed out")), 2000));

          const status = await Promise.race([checkPromise, timeoutPromise]);

          if (status.can_enable_undo) {
            // Git is installed but folder isn't a repo - prompt user
            setPendingProjectPath(selected);
            setGitStatus(status);
            setShowGitDialog(true);
            return; // Wait for dialog response
          } else if (!status.git_installed) {
            // Git not installed - show warning but allow continuing
            setPendingProjectPath(selected);
            setGitStatus(status);
            setShowGitDialog(true);
            return;
          }
        } catch (e) {
          console.warn("Git check failed or timed out:", e);
          // If git check fails or times out, proceed without git features
          // This prevents the onboarding from getting stuck
        }

        // Git is already set up, user doesn't want it, or check failed - proceed
        await finalizeAddProject(selected);
      }
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

  const handleSettingsClose = async () => {
    setView("chat");
    // Reload everything when settings closes (in case projects were modified)
    await refreshAppState();
    await loadUserProjects();
    await loadHistory();
  };

  const handleSelectSession = (sessionId: string) => {
    setCurrentSessionId(sessionId);
    setView("chat");
  };

  const handleNewChat = () => {
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

  const handleSessionCreated = (sessionId: string) => {
    setCurrentSessionId(sessionId);
    // Refresh history to include the new session
    loadHistory();
  };

  const handleExecutePendingTasks = () => {
    // Switch to immediate mode and trigger execution in Chat
    setUsePlanMode(false);
    setSelectedAgent(undefined);
    setExecutePendingTrigger((prev) => prev + 1);
  };

  const handleAddFileToChat = async (file: FileEntry) => {
    // Read file content and create attachment
    try {
      console.log("Add to chat:", file.path);

      // Helper to detect binary files (images and PDFs)
      const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico"]);
      const PDF_EXTENSIONS = new Set(["pdf"]);
      const isImage = file.extension && IMAGE_EXTENSIONS.has(file.extension.toLowerCase());
      const isPdf = file.extension && PDF_EXTENSIONS.has(file.extension.toLowerCase());
      const isBinary = isImage || isPdf;

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
        // Read text file content
        const content = await readFileContent(file.path, 1024 * 1024); // 1MB limit
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

  return (
    <div className="flex h-screen bg-background">
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
              title={sidebarOpen ? "Hide history" : "Show history"}
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
              title="Chat"
            >
              <MessageSquare className="h-5 w-5" />
            </button>
            <button
              onClick={() => setView("settings")}
              className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
                effectiveView === "settings"
                  ? "bg-primary/20 text-primary"
                  : "text-text-muted hover:bg-surface-elevated hover:text-text"
              }`}
              title="Settings"
            >
              <SettingsIcon className="h-5 w-5" />
            </button>

            {/* Theme quick toggle */}
            <button
              onClick={() => cycleTheme()}
              className="flex h-10 w-10 items-center justify-center rounded-lg text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
              title="Switch theme"
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
              title="About"
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
                title="Tasks"
              >
                <ListTodo className="h-5 w-5" />
                {todosData.todos.length > 0 && (
                  <span className="absolute top-1 right-1 h-2 w-2 rounded-full bg-primary" />
                )}
              </button>
            )}
          </nav>

          {/* Security indicator */}
          <div className="mt-auto" title="Zero-trust security enabled">
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
              Sessions
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
              Files
            </button>
          </div>

          {/* Tab Content */}
          <div className="flex-1 overflow-hidden">
            {sidebarTab === "sessions" ? (
              <SessionSidebar
                isOpen={true}
                onToggle={() => setSidebarOpen(!sidebarOpen)}
                sessions={sessions.filter((session) => {
                  // Only show sessions from the active project
                  if (!activeProject) return true;
                  if (!session.directory) return false;

                  // Normalize paths for comparison (handle both / and \ separators)
                  const normalizedSessionDir = session.directory.toLowerCase().replace(/\\/g, "/");
                  const normalizedProjectPath = activeProject.path
                    .toLowerCase()
                    .replace(/\\/g, "/");

                  // Check if session directory starts with or contains the project path
                  return (
                    normalizedSessionDir.includes(normalizedProjectPath) ||
                    normalizedProjectPath.includes(normalizedSessionDir)
                  );
                })}
                projects={projects}
                currentSessionId={currentSessionId}
                onSelectSession={handleSelectSession}
                onNewChat={handleNewChat}
                onDeleteSession={handleDeleteSession}
                isLoading={historyLoading}
                userProjects={userProjects}
                activeProject={activeProject}
                onSwitchProject={handleSwitchProject}
                onAddProject={handleAddProject}
                onManageProjects={handleManageProjects}
                projectSwitcherLoading={projectSwitcherLoading}
              />
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
            className="flex h-full w-full items-center justify-center bg-background"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.8, ease: "easeOut" }}
          >
            <SidecarDownloader onComplete={handleSidecarReady} />
          </motion.div>
        ) : effectiveView === "onboarding" ? (
          <OnboardingView
            key="onboarding"
            onComplete={() => setView("settings")}
            hasConfiguredProvider={hasConfiguredProvider}
            hasWorkspace={!!state?.has_workspace}
            error={error}
          />
        ) : (
          <>
            {/* Chat Area */}
            <div className={cn("flex-1 overflow-hidden relative", selectedFile && "w-1/2")}>
              <Chat
                key={activeProject?.id || "no-project"}
                workspacePath={activeProject?.path || state?.workspace_path || null}
                sessionId={currentSessionId}
                onSessionCreated={handleSessionCreated}
                onSidecarConnected={loadHistory}
                usePlanMode={usePlanMode}
                onPlanModeChange={setUsePlanMode}
                selectedAgent={selectedAgent}
                onAgentChange={(agent) => {
                  setSelectedAgent(agent);
                  setUsePlanMode(agent === "plan");
                }}
                onToggleTaskSidebar={() => setTaskSidebarOpen((prev) => !prev)}
                executePendingTasksTrigger={executePendingTrigger}
                onGeneratingChange={setIsExecutingTasks}
                pendingTasks={todosData.pending}
                fileToAttach={fileToAttach}
                onFileAttached={() => setFileToAttach(null)}
                hasConfiguredProvider={hasConfiguredProvider}
                activeProviderLabel={activeProviderInfo?.providerLabel || undefined}
                activeModelLabel={activeProviderInfo?.modelLabel || undefined}
                onOpenSettings={() => setView("settings")}
                onFileOpen={(filePath) => {
                  // Resolve relative paths to absolute using workspace path
                  const workspacePath = activeProject?.path || state?.workspace_path;
                  let absolutePath = filePath;

                  // If path is not absolute, resolve it relative to workspace
                  if (workspacePath && !filePath.match(/^([a-zA-Z]:[\\/]|\/)/)) {
                    absolutePath = `${workspacePath}/${filePath}`.replace(/\\/g, "/");
                  }

                  // Create FileEntry from path and open in preview
                  const fileName = absolutePath.split(/[\\/]/).pop() || absolutePath;
                  const fileEntry: FileEntry = {
                    path: absolutePath,
                    name: fileName,
                    is_directory: false,
                    extension: fileName.includes(".") ? fileName.split(".").pop() : undefined,
                  };
                  setSelectedFile(fileEntry);
                  setSidebarTab("files"); // Switch to files tab for context
                }}
              />
              <AnimatePresence>
                {effectiveView === "settings" && (
                  <motion.div
                    key="settings"
                    className="absolute inset-0 bg-background"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={{ duration: 0.8, ease: "easeOut" }}
                  >
                    <Settings
                      onClose={handleSettingsClose}
                      onProjectChange={loadUserProjects}
                      onProviderChange={refreshAppState}
                    />
                  </motion.div>
                )}
                {effectiveView === "about" && (
                  <motion.div
                    key="about"
                    className="absolute inset-0 bg-background"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={{ duration: 0.8, ease: "easeOut" }}
                  >
                    <About />
                  </motion.div>
                )}
              </AnimatePresence>
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
                  <FilePreview
                    file={selectedFile}
                    onClose={() => setSelectedFile(null)}
                    onAddToChat={handleAddFileToChat}
                  />
                </motion.div>
              )}
            </AnimatePresence>

            {/* Git Initialization Dialog */}
            <GitInitDialog
              isOpen={showGitDialog}
              onClose={handleGitSkip}
              onInitialize={handleGitInitialize}
              gitInstalled={gitStatus?.git_installed ?? false}
              folderPath={pendingProjectPath ?? ""}
            />

            {/* Task Sidebar */}
            {effectiveView === "chat" && (
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
      <UpdateToast />
    </div>
  );
}

interface OnboardingViewProps {
  onComplete: () => void;
  hasConfiguredProvider: boolean;
  hasWorkspace: boolean;
  error?: string | null;
}

function OnboardingView({
  onComplete,
  hasConfiguredProvider,
  hasWorkspace,
  error,
}: OnboardingViewProps) {
  return (
    <motion.div
      className="flex h-full w-full flex-col items-center justify-center p-8"
      initial={{ opacity: 0, y: 50 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -50 }}
      transition={{ duration: 0.8, ease: "easeOut" }}
    >
      <div className="max-w-md text-center">
        {/* Removed brand mark from onboarding header (it's already in the sidebar) */}

        <motion.h1
          className="mb-3 text-3xl font-bold text-text"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.3 }}
        >
          Welcome to Tandem
        </motion.h1>

        <motion.p
          className="mb-8 text-text-muted"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.4 }}
        >
          Your local-first AI workspace. Let's get started by setting up your environment.
        </motion.p>

        {error && (
          <motion.div
            className="mb-6 flex items-start gap-3 rounded-lg border border-red-500/20 bg-red-500/10 p-4 text-left text-red-500"
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
          >
            <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0" />
            <div className="text-sm">
              <p className="font-semibold">Setup Error</p>
              <p className="opacity-90">{error}</p>
            </div>
          </motion.div>
        )}

        <motion.div
          className="space-y-4"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.5 }}
        >
          <div
            className={cn(
              "rounded-lg border p-4 text-left transition-colors",
              hasWorkspace ? "border-border bg-surface" : "border-primary/50 bg-primary/5"
            )}
          >
            <div className="flex items-start gap-3">
              <FolderOpen
                className={cn("mt-0.5 h-5 w-5", hasWorkspace ? "text-success" : "text-primary")}
              />
              <div>
                <p className="font-medium text-text">
                  1. Add a project
                  {hasWorkspace && (
                    <span className="ml-2 text-xs font-normal text-success">(Completed)</span>
                  )}
                </p>
                <p className="text-sm text-text-muted">
                  Add project folders to work with. Each project is an independent workspace.
                </p>
              </div>
            </div>
          </div>

          <div
            className={cn(
              "rounded-lg border p-4 text-left transition-colors",
              hasConfiguredProvider ? "border-border bg-surface" : "border-primary/50 bg-primary/5"
            )}
          >
            <div className="flex items-start gap-3">
              <MessageSquare
                className={cn(
                  "mt-0.5 h-5 w-5",
                  hasConfiguredProvider ? "text-success" : "text-primary"
                )}
              />
              <div>
                <p className="font-medium text-text">
                  2. Configure AI Provider
                  {hasConfiguredProvider && (
                    <span className="ml-2 text-xs font-normal text-success">(Configured)</span>
                  )}
                </p>
                <p className="text-sm text-text-muted">
                  {hasConfiguredProvider
                    ? "You are ready to chat!"
                    : "Connect an LLM provider (OpenAI, Anthropic, OpenRouter) to start chatting."}
                </p>
              </div>
            </div>
          </div>

          <div className="rounded-lg border border-border bg-surface p-4 text-left">
            <div className="flex items-start gap-3">
              <Shield className="mt-0.5 h-5 w-5 text-success" />
              <div>
                <p className="font-medium text-text">Your data stays local</p>
                <p className="text-sm text-text-muted">
                  API keys are encrypted. No telemetry. Zero-trust security.
                </p>
              </div>
            </div>
          </div>

          <motion.div whileHover={{ scale: 1.05 }} whileTap={{ scale: 0.98 }}>
            <Button onClick={onComplete} size="lg" className="w-full">
              <SettingsIcon className="mr-2 h-4 w-4" />
              {hasConfiguredProvider ? "Open Settings" : "Open Settings to Configure"}
            </Button>
          </motion.div>
        </motion.div>
      </div>
    </motion.div>
  );
}

export default App;
