import { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  Settings as SettingsIcon,
  FolderOpen,
  Shield,
  Cpu,
  Palette,
  ChevronDown,
  ChevronRight,
  Check,
  Trash2,
  Plus,
  Info,
} from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { ProviderCard } from "./ProviderCard";
import { ThemePicker } from "./ThemePicker";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from "@/components/ui/Card";
import { GitInitDialog } from "@/components/dialogs/GitInitDialog";
import { MemoryStats } from "./MemoryStats";

import { useUpdater } from "@/hooks/useUpdater";
import {
  getProvidersConfig,
  setProvidersConfig,
  getUserProjects,
  getActiveProject,
  addProject,
  removeProject,
  setActiveProject,
  storeApiKey,
  checkGitStatus,
  initializeGitRepo,
  checkSidecarStatus,
  type ProvidersConfig,
  type UserProject,
  type SidecarStatus,
} from "@/lib/tauri";
import { open } from "@tauri-apps/plugin-dialog";

interface SettingsProps {
  onClose?: () => void;
  onProjectChange?: () => void;
  onProviderChange?: () => void; // Called when API keys are added/removed
  initialSection?: "providers" | "projects";
  onInitialSectionConsumed?: () => void;
}

export function Settings({
  onClose,
  onProjectChange,
  onProviderChange,
  initialSection,
  onInitialSectionConsumed,
}: SettingsProps) {
  const {
    status: updateStatus,
    updateInfo,
    error: updateError,
    progress: updateProgress,
    checkUpdates,
    installUpdate,
  } = useUpdater();

  const [providers, setProviders] = useState<ProvidersConfig | null>(null);
  const [projects, setProjects] = useState<UserProject[]>([]);
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [projectLoading, setProjectLoading] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [projectsExpanded, setProjectsExpanded] = useState(false);

  const projectsSectionRef = useRef<HTMLDivElement>(null);
  const providersSectionRef = useRef<HTMLDivElement>(null);

  // Version info
  const [appVersion, setAppVersion] = useState<string>("");
  const [sidecarStatus, setSidecarStatus] = useState<SidecarStatus | null>(null);

  // Custom provider state
  const [customEndpoint, setCustomEndpoint] = useState("");
  const [customModel, setCustomModel] = useState("");
  const [customApiKey, setCustomApiKey] = useState("");
  const [customEnabled, setCustomEnabled] = useState(false);

  // Git initialization dialog state
  const [showGitDialog, setShowGitDialog] = useState(false);
  const [pendingProjectPath, setPendingProjectPath] = useState<string | null>(null);
  const [gitStatus, setGitStatus] = useState<{ git_installed: boolean; is_repo: boolean } | null>(
    null
  );

  useEffect(() => {
    loadSettings();
    getVersion().then(setAppVersion);
    checkSidecarStatus().then(setSidecarStatus).catch(console.error);
  }, []);

  useEffect(() => {
    if (!initialSection) return;

    if (initialSection === "projects") setProjectsExpanded(true);

    const target =
      initialSection === "projects" ? projectsSectionRef.current : providersSectionRef.current;

    // Wait a tick so accordions have time to open.
    setTimeout(() => target?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
    onInitialSectionConsumed?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialSection]);

  const loadSettings = async () => {
    try {
      const [config, userProjects, activeProj] = await Promise.all([
        getProvidersConfig(),
        getUserProjects(),
        getActiveProject(),
      ]);
      setProviders(config);
      setProjects(userProjects);

      // Load custom provider if exists
      if (config.custom && config.custom.length > 0) {
        const custom = config.custom[0];
        setCustomEndpoint(custom.endpoint);
        setCustomModel(custom.model || "");
        setCustomEnabled(custom.enabled);
      }

      // Set active project from backend
      if (activeProj) {
        setActiveProjectId(activeProj.id);
      }
    } catch (err) {
      console.error("Failed to load settings:", err);
    } finally {
      setLoading(false);
    }
  };

  const handleProviderChange = async (
    provider: keyof Omit<ProvidersConfig, "custom">,
    enabled: boolean
  ) => {
    if (!providers) return;

    // When enabling a provider, disable all others
    const updated = enabled
      ? {
        openrouter: {
          ...providers.openrouter,
          enabled: provider === "openrouter",
          default: provider === "openrouter",
        },
        opencode_zen: {
          ...providers.opencode_zen,
          enabled: provider === "opencode_zen",
          default: provider === "opencode_zen",
        },
        anthropic: {
          ...providers.anthropic,
          enabled: provider === "anthropic",
          default: provider === "anthropic",
        },
        openai: {
          ...providers.openai,
          enabled: provider === "openai",
          default: provider === "openai",
        },
        ollama: {
          ...providers.ollama,
          enabled: provider === "ollama",
          default: provider === "ollama",
        },
        poe: {
          ...providers.poe,
          enabled: provider === "poe",
          default: provider === "poe",
        },
        custom: providers.custom,
        selected_model: providers.selected_model ?? null,
      }
      : {
        ...providers,
        [provider]: { ...providers[provider], enabled: false, default: false },
      };

    setProviders(updated);
    await setProvidersConfig(updated);
    onProviderChange?.();
  };

  const handleSetDefault = async (provider: keyof Omit<ProvidersConfig, "custom">) => {
    if (!providers) return;

    // Reset all defaults and set the new one
    const updated: ProvidersConfig = {
      openrouter: { ...providers.openrouter, default: provider === "openrouter" },
      opencode_zen: { ...providers.opencode_zen, default: provider === "opencode_zen" },
      anthropic: { ...providers.anthropic, default: provider === "anthropic" },
      openai: { ...providers.openai, default: provider === "openai" },
      ollama: { ...providers.ollama, default: provider === "ollama" },
      poe: { ...providers.poe, default: provider === "poe" },
      custom: providers.custom,
      selected_model: providers.selected_model ?? null,
    };
    setProviders(updated);
    await setProvidersConfig(updated);
    onProviderChange?.();
  };

  const handleModelChange = async (
    provider: keyof Omit<ProvidersConfig, "custom">,
    model: string
  ) => {
    if (!providers) return;

    const updated = {
      ...providers,
      [provider]: { ...providers[provider], model },
    };
    setProviders(updated);
    await setProvidersConfig(updated);
    onProviderChange?.();
  };

  const handleSelectFolder = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Folder",
      });

      if (selected && typeof selected === "string") {
        // Check Git status
        const status = await checkGitStatus(selected);

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

        // Git is already set up - proceed
        await finalizeAddProject(selected);
      }
    } catch (err) {
      console.error("Failed to add project:", err);
    }
  };

  // New helper function to complete project addition
  const finalizeAddProject = async (path: string) => {
    try {
      setProjectLoading(true);
      const project = await addProject(path);
      await loadSettings();
      // Set as active
      await setActiveProject(project.id);
      setActiveProjectId(project.id);
      // Notify parent that projects have changed
      onProjectChange?.();
    } catch (err) {
      console.error("Failed to finalize project:", err);
    } finally {
      setProjectLoading(false);
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

  const handleSetActiveProject = async (projectId: string) => {
    try {
      setProjectLoading(true);
      await setActiveProject(projectId);
      setActiveProjectId(projectId);
      await loadSettings();
      // Notify parent that active project has changed
      onProjectChange?.();
    } catch (err) {
      console.error("Failed to set active project:", err);
    } finally {
      setProjectLoading(false);
    }
  };

  const handleRemoveProject = async (projectId: string) => {
    try {
      setProjectLoading(true);
      await removeProject(projectId);
      if (activeProjectId === projectId) {
        setActiveProjectId(null);
      }
      await loadSettings();
      setDeleteConfirm(null);
    } catch (err) {
      console.error("Failed to remove project:", err);
    } finally {
      setProjectLoading(false);
    }
  };

  const handleCustomProviderSave = async () => {
    if (!providers || !customEndpoint.trim()) return;

    // When enabling custom provider, disable all others
    const updated: ProvidersConfig = {
      openrouter: { ...providers.openrouter, enabled: false, default: false },
      opencode_zen: { ...providers.opencode_zen, enabled: false, default: false },
      anthropic: { ...providers.anthropic, enabled: false, default: false },
      openai: { ...providers.openai, enabled: false, default: false },
      ollama: { ...providers.ollama, enabled: false, default: false },
      poe: { ...providers.poe, enabled: false, default: false },
      custom: [
        {
          enabled: customEnabled,
          default: customEnabled,
          endpoint: customEndpoint,
          model: customModel || undefined,
          has_key: false, // Custom provider key checking not implemented yet
        },
      ],
      selected_model: providers.selected_model ?? null,
    };

    setProviders(updated);
    await setProvidersConfig(updated);

    // Store custom API key if provided
    if (customApiKey.trim() && customEnabled) {
      try {
        await storeApiKey("custom_provider", customApiKey);
      } catch (err) {
        console.error("Failed to store custom API key:", err);
      }
    }
  };

  const handleCustomProviderToggle = async (enabled: boolean) => {
    setCustomEnabled(enabled);

    if (enabled && providers) {
      // When enabling custom provider, disable all others
      const updated: ProvidersConfig = {
        openrouter: { ...providers.openrouter, enabled: false, default: false },
        opencode_zen: { ...providers.opencode_zen, enabled: false, default: false },
        anthropic: { ...providers.anthropic, enabled: false, default: false },
        openai: { ...providers.openai, enabled: false, default: false },
        ollama: { ...providers.ollama, enabled: false, default: false },
        poe: { ...providers.poe, enabled: false, default: false },
        custom: [
          {
            enabled: true,
            default: true,
            endpoint: customEndpoint || "https://api.example.com/v1",
            model: customModel || undefined,
            has_key: false, // Custom provider key checking not implemented yet
          },
        ],
        selected_model: providers.selected_model ?? null,
      };

      setProviders(updated);
      await setProvidersConfig(updated);
    } else if (!enabled && providers) {
      // Disable custom provider
      const updated: ProvidersConfig = {
        ...providers,
        custom: [],
      };

      setProviders(updated);
      await setProvidersConfig(updated);
    }
  };

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-primary border-t-transparent" />
      </div>
    );
  }

  const activeProject = projects.find((p) => p.id === activeProjectId) ?? null;

  return (
    <motion.div
      className="h-full overflow-y-auto p-6"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
    >
      <div className="mx-auto max-w-2xl space-y-8">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-primary/10">
              <SettingsIcon className="h-6 w-6 text-primary" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-text">Settings</h1>
              <p className="text-text-muted">Configure your Tandem folders and AI</p>
            </div>
          </div>
          {onClose && (
            <Button variant="ghost" onClick={onClose}>
              Close
            </Button>
          )}
        </div>

        {/* Version Info */}
        <div className="flex items-center justify-between gap-4 rounded-lg border border-border bg-surface-elevated/50 p-3 text-sm text-text-muted">
          <div className="flex items-center gap-2">
            <Info className="h-4 w-4 text-primary" />
            <span>Tandem v{appVersion || "..."}</span>
            <span className="text-text-subtle">•</span>
            <span>OpenCode v{sidecarStatus?.version || "..."}</span>
          </div>
        </div>

        {/* Updates */}
        <Card>
          <CardHeader>
            <div className="flex items-start justify-between gap-4">
              <div className="flex-1">
                <CardTitle>Updates</CardTitle>
                <CardDescription>Keep Tandem up to date.</CardDescription>
              </div>
              <Button
                size="sm"
                onClick={updateStatus === "available" ? installUpdate : () => checkUpdates(false)}
                disabled={
                  updateStatus === "checking" ||
                  updateStatus === "downloading" ||
                  updateStatus === "installing"
                }
              >
                {updateStatus === "checking" && "Checking..."}
                {updateStatus === "downloading" && "Downloading..."}
                {updateStatus === "installing" && "Installing..."}
                {updateStatus === "available" && "Download & Install"}
                {(updateStatus === "idle" ||
                  updateStatus === "upToDate" ||
                  updateStatus === "installed" ||
                  updateStatus === "error") &&
                  "Check for Updates"}
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="text-sm text-text-muted">
              {updateStatus === "available" && updateInfo
                ? `Update available: v${updateInfo.version} `
                : updateStatus === "upToDate"
                  ? "You're on the latest version."
                  : updateStatus === "installed"
                    ? "Update installed. Relaunching..."
                    : updateStatus === "error"
                      ? updateError || "Update failed."
                      : updateStatus === "checking"
                        ? "Checking for updates..."
                        : updateStatus === "installing"
                          ? "Installing update..."
                          : updateStatus === "downloading"
                            ? "Downloading update..."
                            : ""}
            </div>

            {updateStatus === "downloading" && updateProgress && (
              <div className="space-y-2">
                <div className="h-2 w-full overflow-hidden rounded-full bg-surface-elevated">
                  <motion.div
                    className="h-full bg-gradient-to-r from-primary to-secondary"
                    initial={{ width: 0 }}
                    animate={{ width: `${updateProgress.percent}% ` }}
                    transition={{ duration: 0.2 }}
                  />
                </div>
                <div className="flex justify-between text-xs text-text-subtle">
                  <span>{Math.round(updateProgress.percent)}%</span>
                  <span>
                    {updateProgress.total > 0
                      ? `${Math.round(updateProgress.downloaded / 1024 / 1024)} MB / ${Math.round(updateProgress.total / 1024 / 1024)} MB`
                      : "Downloading..."}
                  </span>
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        {/* Folders Section */}
        <div ref={projectsSectionRef} />
        <Card>
          <CardHeader>
            <div className="flex items-start gap-3">
              <FolderOpen className="mt-0.5 h-5 w-5 text-secondary" />
              <div className="flex-1">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <CardTitle>Folders</CardTitle>
                    <CardDescription>
                      Add and manage folders. Each folder is an independent space.
                    </CardDescription>
                  </div>
                  <button
                    type="button"
                    onClick={() => setProjectsExpanded((v) => !v)}
                    className="rounded-md p-1 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
                    aria-expanded={projectsExpanded}
                    title={projectsExpanded ? "Collapse folders" : "Expand folders"}
                  >
                    {projectsExpanded ? (
                      <ChevronDown className="h-5 w-5" />
                    ) : (
                      <ChevronRight className="h-5 w-5" />
                    )}
                  </button>
                </div>

                {!projectsExpanded && (
                  <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-subtle">
                    <span>
                      {projects.length} folder{projects.length === 1 ? "" : "s"}
                    </span>
                    <span className="opacity-60">•</span>
                    <span className="truncate">Active: {activeProject?.name ?? "None"}</span>
                  </div>
                )}
              </div>
            </div>
          </CardHeader>
          <AnimatePresence initial={false}>
            {projectsExpanded && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.2, ease: "easeOut" }}
                className="overflow-hidden"
              >
                <CardContent>
                  <div className="space-y-3">
                    {projects.length === 0 ? (
                      <div className="rounded-lg border border-border bg-surface-elevated p-6 text-center">
                        <FolderOpen className="mx-auto mb-2 h-8 w-8 text-text-subtle" />
                        <p className="text-sm text-text-muted">No folders added yet</p>
                        <p className="text-xs text-text-subtle">Add a folder to get started</p>
                      </div>
                    ) : (
                      <div className="space-y-2">
                        <AnimatePresence>
                          {projects.map((project) => (
                            <motion.div
                              key={project.id}
                              initial={{ opacity: 0, y: -10 }}
                              animate={{ opacity: 1, y: 0 }}
                              exit={{ opacity: 0, x: -20 }}
                              className="flex items-center gap-3 rounded-lg border border-border bg-surface-elevated p-3"
                            >
                              <div className="min-w-0 flex-1">
                                <div className="flex items-center gap-2">
                                  <p className="font-medium text-text">{project.name}</p>
                                  {activeProjectId === project.id && (
                                    <span className="inline-flex items-center gap-1 rounded-full bg-primary/20 px-2 py-0.5 text-xs text-primary">
                                      <Check className="h-3 w-3" />
                                      Active
                                    </span>
                                  )}
                                </div>
                                <p
                                  className="truncate font-mono text-xs text-text-muted"
                                  title={project.path}
                                >
                                  {project.path}
                                </p>
                              </div>
                              <div className="flex items-center gap-2">
                                {activeProjectId !== project.id && (
                                  <Button
                                    size="sm"
                                    variant="ghost"
                                    onClick={() => handleSetActiveProject(project.id)}
                                    disabled={projectLoading}
                                  >
                                    Set Active
                                  </Button>
                                )}
                                {deleteConfirm === project.id ? (
                                  <>
                                    <Button
                                      size="sm"
                                      variant="ghost"
                                      onClick={() => handleRemoveProject(project.id)}
                                      disabled={projectLoading}
                                      className="text-error hover:bg-error/10"
                                    >
                                      Confirm
                                    </Button>
                                    <Button
                                      size="sm"
                                      variant="ghost"
                                      onClick={() => setDeleteConfirm(null)}
                                      disabled={projectLoading}
                                    >
                                      Cancel
                                    </Button>
                                  </>
                                ) : (
                                  <Button
                                    size="sm"
                                    variant="ghost"
                                    onClick={() => setDeleteConfirm(project.id)}
                                    disabled={projectLoading}
                                    className="text-text-subtle hover:text-error"
                                  >
                                    <Trash2 className="h-4 w-4" />
                                  </Button>
                                )}
                              </div>
                            </motion.div>
                          ))}
                        </AnimatePresence>
                      </div>
                    )}

                    <Button
                      onClick={handleSelectFolder}
                      disabled={projectLoading}
                      className="w-full"
                    >
                      <Plus className="mr-2 h-4 w-4" />
                      Add Folder
                    </Button>
                  </div>

                  <p className="mt-3 text-xs text-text-subtle">
                    <Shield className="mr-1 inline h-3 w-3" />
                    Tandem can only access files within selected folders. Sensitive files (.env,
                    .ssh, etc.) are always blocked.
                  </p>
                </CardContent>
              </motion.div>
            )}
          </AnimatePresence>
        </Card>

        {/* Appearance Section */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-3">
              <Palette className="h-5 w-5 text-primary" />
              <div className="flex-1">
                <CardTitle>Appearance</CardTitle>
                <CardDescription>
                  Choose a theme. Changes apply instantly and are saved on this device.
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            <ThemePicker variant="compact" />
          </CardContent>
        </Card>

        {/* LLM Providers Section */}
        <div ref={providersSectionRef} />
        <div className="space-y-4">
          <div className="flex items-center gap-3">
            <Cpu className="h-5 w-5 text-primary" />
            <div>
              <h2 className="text-lg font-semibold text-text">LLM Providers</h2>
              <p className="text-sm text-text-muted">
                Configure your AI providers. OpenRouter is recommended for access to multiple
                models.
              </p>
            </div>
          </div>

          {providers && (
            <div className="space-y-4">
              <ProviderCard
                id="opencode_zen"
                name="OpenCode Zen"
                description="Access to free and premium models - includes free options"
                endpoint="https://opencode.ai/zen/v1"
                model={providers.opencode_zen.model}
                isDefault={providers.opencode_zen.default}
                enabled={providers.opencode_zen.enabled}
                onEnabledChange={(enabled) => handleProviderChange("opencode_zen", enabled)}
                onModelChange={(model) => handleModelChange("opencode_zen", model)}
                onSetDefault={() => handleSetDefault("opencode_zen")}
                onKeyChange={onProviderChange}
                docsUrl="https://opencode.ai/auth"
              />

              <ProviderCard
                id="openrouter"
                name="OpenRouter"
                description="Access 100+ models with one API key"
                endpoint="https://openrouter.ai/api/v1"
                model={providers.openrouter.model}
                isDefault={providers.openrouter.default}
                enabled={providers.openrouter.enabled}
                onEnabledChange={(enabled) => handleProviderChange("openrouter", enabled)}
                onModelChange={(model) => handleModelChange("openrouter", model)}
                onSetDefault={() => handleSetDefault("openrouter")}
                onKeyChange={onProviderChange}
                docsUrl="https://openrouter.ai/keys"
              />

              <ProviderCard
                id="anthropic"
                name="Anthropic"
                description="Direct access to Anthropic models"
                endpoint="https://api.anthropic.com"
                model={providers.anthropic.model}
                isDefault={providers.anthropic.default}
                enabled={providers.anthropic.enabled}
                onEnabledChange={(enabled) => handleProviderChange("anthropic", enabled)}
                onModelChange={(model) => handleModelChange("anthropic", model)}
                onSetDefault={() => handleSetDefault("anthropic")}
                onKeyChange={onProviderChange}
                docsUrl="https://console.anthropic.com/settings/keys"
              />

              <ProviderCard
                id="openai"
                name="OpenAI"
                description="GPT-4 and other OpenAI models"
                endpoint="https://api.openai.com/v1"
                model={providers.openai.model}
                isDefault={providers.openai.default}
                enabled={providers.openai.enabled}
                onEnabledChange={(enabled) => handleProviderChange("openai", enabled)}
                onModelChange={(model) => handleModelChange("openai", model)}
                onSetDefault={() => handleSetDefault("openai")}
                onKeyChange={onProviderChange}
                docsUrl="https://platform.openai.com/api-keys"
              />

              <ProviderCard
                id="ollama"
                name="Ollama"
                description="Run models locally - no API key needed"
                endpoint="http://localhost:11434"
                model={providers.ollama.model}
                isDefault={providers.ollama.default}
                enabled={providers.ollama.enabled}
                onEnabledChange={(enabled) => handleProviderChange("ollama", enabled)}
                onModelChange={(model) => handleModelChange("ollama", model)}
                onSetDefault={() => handleSetDefault("ollama")}
                onKeyChange={onProviderChange}
                docsUrl="https://ollama.ai"
              />

              <ProviderCard
                id="poe"
                name="Poe"
                description="Access models via Poe's API"
                endpoint="https://api.poe.com/v1"
                model={providers.poe.model}
                isDefault={providers.poe.default}
                enabled={providers.poe.enabled}
                onEnabledChange={(enabled) => handleProviderChange("poe", enabled)}
                onModelChange={(model) => handleModelChange("poe", model)}
                onSetDefault={() => handleSetDefault("poe")}
                onKeyChange={onProviderChange}
                docsUrl="https://poe.com/api"
              />

              {/* Custom Provider Section */}
              <Card className="border-2 border-dashed">
                <CardHeader>
                  <div className="flex items-center justify-between">
                    <div>
                      <CardTitle>Custom Provider</CardTitle>
                      <CardDescription>
                        Configure your own LLM provider with a custom endpoint
                      </CardDescription>
                    </div>
                    <Switch
                      checked={customEnabled}
                      onChange={(e) => handleCustomProviderToggle(e.target.checked)}
                    />
                  </div>
                </CardHeader>
                <AnimatePresence>
                  {customEnabled && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: "auto", opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: 0.2 }}
                    >
                      <CardContent className="space-y-4">
                        <div>
                          <label className="text-xs font-medium text-text-subtle">
                            Endpoint URL
                          </label>
                          <Input
                            placeholder="https://api.example.com/v1"
                            value={customEndpoint}
                            onChange={(e) => setCustomEndpoint(e.target.value)}
                          />
                        </div>
                        <div>
                          <label className="text-xs font-medium text-text-subtle">Model</label>
                          <Input
                            placeholder="model-name"
                            value={customModel}
                            onChange={(e) => setCustomModel(e.target.value)}
                          />
                        </div>
                        <div>
                          <label className="text-xs font-medium text-text-subtle">
                            API Key (optional)
                          </label>
                          <Input
                            type="password"
                            placeholder="sk-..."
                            value={customApiKey}
                            onChange={(e) => setCustomApiKey(e.target.value)}
                          />
                        </div>
                        <Button
                          onClick={handleCustomProviderSave}
                          disabled={!customEndpoint.trim()}
                          className="w-full"
                        >
                          Save Custom Provider
                        </Button>
                      </CardContent>
                    </motion.div>
                  )}
                </AnimatePresence>
              </Card>
            </div>
          )}
        </div>

        {/* Memory Stats */}
        <MemoryStats />

        {/* Security Info */}
        <Card variant="glass">
          <CardContent className="flex items-start gap-4">
            <Shield className="mt-0.5 h-5 w-5 flex-shrink-0 text-success" />
            <div className="space-y-2">
              <p className="font-medium text-text">Your keys are secure</p>
              <ul className="space-y-1 text-sm text-text-muted">
                <li>• API keys are encrypted with AES-256-GCM</li>
                <li>• Keys never leave your device</li>
                <li>• No telemetry or data collection</li>
                <li>• All network traffic is allowlisted</li>
              </ul>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Git Initialization Dialog */}
      <GitInitDialog
        isOpen={showGitDialog}
        onClose={handleGitSkip}
        onInitialize={handleGitInitialize}
        gitInstalled={gitStatus?.git_installed ?? false}
        folderPath={pendingProjectPath ?? ""}
      />
    </motion.div>
  );
}
