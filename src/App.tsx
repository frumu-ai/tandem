import { useState, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Settings } from "@/components/settings";
import { About } from "@/components/about";
import { Chat } from "@/components/chat";
import { SidecarDownloader } from "@/components/sidecar";
import { SessionSidebar, type SessionInfo, type Project } from "@/components/sidebar";
import { TaskSidebar } from "@/components/tasks/TaskSidebar";
import { Button } from "@/components/ui";
import { useAppState } from "@/hooks/useAppState";
import { useTodos } from "@/hooks/useTodos";
import logo from "@/assets/logo.png";
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
  type Session,
  type VaultStatus,
  type UserProject,
} from "@/lib/tauri";
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
  const [sidecarReady, setSidecarReady] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [taskSidebarOpen, setTaskSidebarOpen] = useState(false);
  const [usePlanMode, setUsePlanMode] = useState(false);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [vaultUnlocked, setVaultUnlocked] = useState(false);
  const [executePendingTrigger, setExecutePendingTrigger] = useState(0);
  const [isExecutingTasks, setIsExecutingTasks] = useState(false);

  // Project management state
  const [userProjects, setUserProjects] = useState<UserProject[]>([]);
  const [activeProject, setActiveProjectState] = useState<UserProject | null>(null);
  const [projectSwitcherLoading, setProjectSwitcherLoading] = useState(false);

  // Todos for task sidebar
  const todosData = useTodos(currentSessionId);

  // Start with sidecar setup, then onboarding if no workspace, otherwise chat
  const [view, setView] = useState<View>(() => "sidecar-setup");

  // Update view based on workspace state after loading
  const effectiveView =
    loading || !vaultUnlocked
      ? "sidecar-setup"
      : !sidecarReady
        ? "sidecar-setup"
        : view === "onboarding" && state?.has_workspace
          ? "chat"
          : view === "sidecar-setup"
            ? state?.has_workspace
              ? "chat"
              : "onboarding"
            : view;

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
            return;
          }
          // Also check the global flag set by splash screen
          if (window.__vaultUnlocked) {
            setVaultUnlocked(true);
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
  }, []);

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

  useEffect(() => {
    if (vaultUnlocked) {
      loadUserProjects();
    }
  }, [vaultUnlocked, loadUserProjects]);

  const handleSidecarReady = () => {
    setSidecarReady(true);
    // Navigate to appropriate view
    if (state?.has_workspace) {
      setView("chat");
    } else {
      setView("onboarding");
    }
  };

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

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Project Folder",
      });

      if (selected && typeof selected === "string") {
        setProjectSwitcherLoading(true);
        const project = await addProject(selected);
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
      }
    } catch (e) {
      console.error("Failed to add project:", e);
    } finally {
      setProjectSwitcherLoading(false);
    }
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
    setExecutePendingTrigger((prev) => prev + 1);
  };

  return (
    <div className="flex h-screen bg-background">
      {/* Icon Sidebar */}
      <motion.aside
        className="flex w-16 flex-col items-center border-r border-border bg-surface py-4 z-20"
        initial={{ x: -64 }}
        animate={{ x: 0 }}
        transition={{ duration: 0.3 }}
      >
        {/* Logo */}
        <div className="mb-8 h-10 w-10 overflow-hidden rounded-xl ring-1 ring-white/10">
          <img src={logo} alt="Tandem logo" className="h-full w-full object-cover" />
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

          {/* Task sidebar toggle - visible in Plan Mode or when tasks exist */}
          {(usePlanMode || todosData.todos.length > 0) && (
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

      {/* Session Sidebar */}
      {effectiveView === "chat" && (
        <SessionSidebar
          isOpen={sidebarOpen}
          onToggle={() => setSidebarOpen(!sidebarOpen)}
          sessions={sessions.filter((session) => {
            // Only show sessions from the active project
            if (!activeProject) return true;
            if (!session.directory) return false;

            // Normalize paths for comparison (handle both / and \ separators)
            const normalizedSessionDir = session.directory.toLowerCase().replace(/\\/g, "/");
            const normalizedProjectPath = activeProject.path.toLowerCase().replace(/\\/g, "/");

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
      )}

      {/* Main Content */}
      <main className="flex-1 overflow-hidden relative">
        {effectiveView === "sidecar-setup" ? (
          <motion.div
            key="sidecar-setup"
            className="flex h-full items-center justify-center bg-background"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <SidecarDownloader onComplete={handleSidecarReady} />
          </motion.div>
        ) : effectiveView === "onboarding" && !state?.has_workspace ? (
          <OnboardingView key="onboarding" onComplete={() => setView("settings")} />
        ) : (
          <>
            <Chat
              key={activeProject?.id || "no-project"}
              workspacePath={activeProject?.path || state?.workspace_path || null}
              sessionId={currentSessionId}
              onSessionCreated={handleSessionCreated}
              onSidecarConnected={loadHistory}
              usePlanMode={usePlanMode}
              onPlanModeChange={setUsePlanMode}
              onToggleTaskSidebar={() => setTaskSidebarOpen((prev) => !prev)}
              executePendingTasksTrigger={executePendingTrigger}
              onGeneratingChange={setIsExecutingTasks}
              pendingTasks={todosData.pending}
            />
            <AnimatePresence>
              {effectiveView === "settings" && (
                <motion.div
                  key="settings"
                  className="absolute inset-0 bg-background"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                >
                  <Settings onClose={handleSettingsClose} />
                </motion.div>
              )}
              {effectiveView === "about" && (
                <motion.div
                  key="about"
                  className="absolute inset-0 bg-background"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                >
                  <About />
                </motion.div>
              )}
            </AnimatePresence>

            {/* Task Sidebar */}
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
          </>
        )}
      </main>
    </div>
  );
}

interface OnboardingViewProps {
  onComplete: () => void;
}

function OnboardingView({ onComplete }: OnboardingViewProps) {
  return (
    <motion.div
      className="flex h-full flex-col items-center justify-center p-8"
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -20 }}
    >
      <div className="max-w-md text-center">
        <motion.div
          className="mx-auto mb-6 h-20 w-20 overflow-hidden rounded-2xl ring-1 ring-white/10"
          initial={{ scale: 0 }}
          animate={{ scale: 1 }}
          transition={{ delay: 0.2, type: "spring" }}
        >
          <img src={logo} alt="Tandem logo" className="h-full w-full object-cover" />
        </motion.div>

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
          Your local-first AI workspace. Let's get started by adding a project folder and
          configuring your LLM provider.
        </motion.p>

        <motion.div
          className="space-y-4"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.5 }}
        >
          <div className="rounded-lg border border-border bg-surface p-4 text-left">
            <div className="flex items-start gap-3">
              <FolderOpen className="mt-0.5 h-5 w-5 text-primary" />
              <div>
                <p className="font-medium text-text">Add a project</p>
                <p className="text-sm text-text-muted">
                  Add project folders to work with. Each project is an independent workspace.
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

          <Button onClick={onComplete} size="lg" className="w-full">
            <SettingsIcon className="mr-2 h-4 w-4" />
            Open Settings
          </Button>
        </motion.div>
      </div>
    </motion.div>
  );
}

export default App;
