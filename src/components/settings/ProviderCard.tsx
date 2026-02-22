import { useState, useEffect } from "react";
import { cn } from "@/lib/utils";
import { motion, AnimatePresence } from "framer-motion";
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import {
  Key,
  Check,
  X,
  Eye,
  EyeOff,
  ExternalLink,
  ChevronDown,
  Play,
  Square,
  RefreshCw,
  RotateCcw,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  storeApiKey,
  deleteApiKey,
  hasApiKey,
  listOllamaModels,
  listRunningOllamaModels,
  stopOllamaModel,
  runOllamaModel,
  type ApiKeyType,
  type ModelInfo,
} from "@/lib/tauri";
import { useTranslation } from "react-i18next";

// Popular/suggested models for providers with limited options
const PROVIDER_MODELS: Record<string, { id: string; name: string; description?: string }[]> = {
  anthropic: [
    {
      id: "claude-sonnet-4-20250514",
      name: "Sonnet 4",
      description: "Latest, most intelligent",
    },
    { id: "claude-3-5-sonnet-20241022", name: "3.5 Sonnet", description: "Fast & capable" },
    { id: "claude-3-5-haiku-20241022", name: "3.5 Haiku", description: "Fastest" },
    { id: "claude-3-opus-20240229", name: "3 Opus", description: "Most capable (legacy)" },
  ],
  openai: [
    { id: "gpt-4o", name: "GPT-4o", description: "Flagship model" },
    { id: "gpt-4o-mini", name: "GPT-4o Mini", description: "Fast & affordable" },
    { id: "gpt-4-turbo", name: "GPT-4 Turbo", description: "Previous flagship" },
    { id: "o1", name: "o1", description: "Reasoning model" },
    { id: "o1-mini", name: "o1 Mini", description: "Fast reasoning" },
  ],
  opencode_zen: [
    { id: "minimax-m2.1-free", name: "Minimax M2", description: "Free (Flash)" },
    { id: "gpt-5-nano", name: "GPT 5 Nano", description: "Free" },
    { id: "grok-code", name: "Grok Code Fast 1", description: "Free (limited time)" },
    { id: "glm-4.7-free", name: "GLM 4.7", description: "Free (limited time)" },
    { id: "big-pickle", name: "Big Pickle", description: "Free (limited time)" },
    { id: "gpt-5.2-codex", name: "GPT 5.2 Codex", description: "Premium coding" },
    { id: "claude-sonnet-4-5", name: "Sonnet 4.5", description: "Premium" },
    { id: "qwen3-coder", name: "Qwen3 Coder 480B", description: "Premium coding" },
  ],
};

// Suggested models for text input (shown as placeholder examples)
const SUGGESTED_MODELS: Record<string, string[]> = {
  openrouter: [
    "anthropic/claude-sonnet-4",
    "anthropic/claude-3.5-sonnet",
    "openai/gpt-4o",
    "google/gemini-2.0-flash-exp:free",
    "deepseek/deepseek-chat",
  ],
  ollama: ["llama3.2", "codellama", "mistral", "deepseek-coder-v2", "qwen2.5-coder"],
  opencode_zen: [
    "minimax-m2.1-free",
    "gpt-5-nano",
    "grok-code",
    "glm-4.7-free",
    "big-pickle",
    "gpt-5.2-codex",
    "claude-sonnet-4-5",
    "qwen3-coder",
    "kimi-k2",
  ],
};

// Providers that use free-form text input (have too many models for a dropdown)
const TEXT_INPUT_PROVIDERS = ["openrouter", "ollama", "opencode_zen"];

interface ProviderCardProps {
  id: ApiKeyType;
  name: string;
  description: string;
  endpoint: string;
  defaultEndpoint?: string; // Default endpoint for reset functionality
  model?: string;
  isDefault?: boolean;
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  onModelChange?: (model: string) => void;
  onEndpointChange?: (endpoint: string) => void;
  onSetDefault?: () => void;
  onKeyChange?: () => void; // Called when API key is saved or deleted
  docsUrl?: string;
}

export function ProviderCard({
  id,
  name,
  description,
  endpoint,
  defaultEndpoint,
  model,
  isDefault = false,
  enabled,
  onEnabledChange,
  onModelChange,
  onEndpointChange,
  onSetDefault,
  onKeyChange,
  docsUrl,
}: ProviderCardProps) {
  const { t } = useTranslation(["common", "settings"]);
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [hasKey, setHasKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [showModelDropdown, setShowModelDropdown] = useState(false);
  const [modelInput, setModelInput] = useState(model || "");
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [discoveredModels, setDiscoveredModels] = useState<ModelInfo[]>([]);
  const [runningModels, setRunningModels] = useState<ModelInfo[]>([]);
  const [loadingOllama, setLoadingOllama] = useState(false);
  const [endpointInput, setEndpointInput] = useState(endpoint);
  const [isEditingEndpoint, setIsEditingEndpoint] = useState(false);

  const isTextInputProvider = TEXT_INPUT_PROVIDERS.includes(id);
  const availableModels = PROVIDER_MODELS[id] || [];

  // Use discovered models for Ollama suggestions, otherwise fallback to static ones
  const suggestions =
    id === "ollama" && discoveredModels.length > 0
      ? discoveredModels.map((m) => m.id)
      : SUGGESTED_MODELS[id] || [];

  const selectedModel = model || availableModels[0]?.id || "";
  const selectedModelInfo = availableModels.find((m) => m.id === selectedModel);
  const requiresApiKey = id !== "ollama";

  // Filter suggestions based on input
  const filteredSuggestions = suggestions.filter((s) =>
    s.toLowerCase().includes(modelInput.toLowerCase())
  );

  // Check if key exists on mount
  useEffect(() => {
    if (!requiresApiKey) {
      setHasKey(false);
      return;
    }
    hasApiKey(id).then(setHasKey).catch(console.error);
  }, [id, requiresApiKey]);

  // Sync modelInput with model prop
  useEffect(() => {
    setModelInput(model || "");
  }, [model]);

  // Sync endpointInput with endpoint prop
  useEffect(() => {
    setEndpointInput(endpoint);
  }, [endpoint]);

  // Load Ollama data
  const loadOllamaData = async () => {
    if (id !== "ollama" || !enabled) return;
    setLoadingOllama(true);
    try {
      const [discovered, running] = await Promise.all([
        listOllamaModels(),
        listRunningOllamaModels(),
      ]);
      setDiscoveredModels(discovered);
      setRunningModels(running);
    } catch (err) {
      console.error("Failed to load Ollama data:", err);
    } finally {
      setLoadingOllama(false);
    }
  };

  useEffect(() => {
    if (id === "ollama" && enabled) {
      loadOllamaData();
    }
  }, [id, enabled]);

  const handleStopModel = async (name: string) => {
    try {
      await stopOllamaModel(name);
      await loadOllamaData();
    } catch (err) {
      setError(t("providerCard.errors.stopModel", { error: String(err), ns: "settings" }));
    }
  };

  const handleRunModel = async (name: string) => {
    try {
      await runOllamaModel(name);
      await loadOllamaData();
    } catch (err) {
      setError(t("providerCard.errors.runModel", { error: String(err), ns: "settings" }));
    }
  };

  const handleSaveKey = async () => {
    if (!apiKey.trim()) {
      setError(t("providerCard.errors.apiKeyRequired", { ns: "settings" }));
      return;
    }

    setSaving(true);
    setError(null);
    setSuccess(false);

    try {
      await storeApiKey(id, apiKey);
      setHasKey(true);
      setApiKey("");
      setSuccess(true);
      setTimeout(() => setSuccess(false), 2000);
      // Notify parent that key state changed
      onKeyChange?.();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : t("providerCard.errors.saveApiKey", { ns: "settings" })
      );
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteKey = async () => {
    try {
      await deleteApiKey(id);
      setHasKey(false);
      // Notify parent that key state changed
      onKeyChange?.();
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : t("providerCard.errors.deleteApiKey", { ns: "settings" })
      );
    }
  };

  const handleOpenExternal = async (url: string) => {
    try {
      await openUrl(url);
    } catch (err) {
      console.error("Failed to open link:", err);
    }
  };

  const handleSaveEndpoint = () => {
    if (endpointInput.trim() && endpointInput !== endpoint) {
      onEndpointChange?.(endpointInput.trim());
    }
    setIsEditingEndpoint(false);
  };

  const handleResetEndpoint = () => {
    if (defaultEndpoint) {
      setEndpointInput(defaultEndpoint);
      onEndpointChange?.(defaultEndpoint);
    }
  };

  const isEndpointModified = defaultEndpoint && endpoint !== defaultEndpoint;

  return (
    <Card className="relative overflow-hidden">
      {isDefault && (
        <div className="absolute right-0 top-0 rounded-bl-lg bg-primary px-3 py-1 text-xs font-medium text-white">
          {t("providerCard.default", { ns: "settings" })}
        </div>
      )}

      <CardHeader>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-surface-elevated">
              <Key className="h-5 w-5 text-primary" />
            </div>
            <div>
              <CardTitle>{name}</CardTitle>
              <CardDescription>{description}</CardDescription>
            </div>
          </div>
          <div className="flex items-center gap-2">
            {requiresApiKey && hasKey && (
              <span className="rounded-full bg-success/15 px-2 py-0.5 text-xs text-success">
                {t("providerCard.keySaved", { ns: "settings" })}
              </span>
            )}
            <Switch checked={enabled} onChange={(e) => onEnabledChange(e.target.checked)} />
          </div>
        </div>
      </CardHeader>

      <AnimatePresence>
        {enabled && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2 }}
          >
            <CardContent className="space-y-4">
              {/* Model Selection - Text Input for OpenRouter/Ollama */}
              {isTextInputProvider && id !== "ollama" && (
                <div className="space-y-2">
                  <label className="text-xs font-medium text-text-subtle">
                    {t("providers.model", { ns: "settings" })}
                  </label>
                  <div className="relative">
                    <Input
                      type="text"
                      placeholder={
                        id === "openrouter"
                          ? t("providerCard.placeholders.modelOpenRouter", { ns: "settings" })
                          : t("providerCard.placeholders.modelGeneric", { ns: "settings" })
                      }
                      value={modelInput}
                      onChange={(e) => {
                        setModelInput(e.target.value);
                        setShowSuggestions(true);
                      }}
                      onFocus={() => setShowSuggestions(true)}
                      onBlur={() => {
                        // Delay to allow click on suggestion
                        setTimeout(() => setShowSuggestions(false), 150);
                      }}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && modelInput.trim()) {
                          onModelChange?.(modelInput.trim());
                          setShowSuggestions(false);
                        }
                      }}
                    />
                    {modelInput !== model && modelInput.trim() && (
                      <Button
                        size="sm"
                        className="absolute right-1 top-1/2 -translate-y-1/2 h-7 px-2"
                        onClick={() => {
                          onModelChange?.(modelInput.trim());
                          setShowSuggestions(false);
                        }}
                      >
                        {t("actions.save", { ns: "common" })}
                      </Button>
                    )}

                    {/* Suggestions dropdown */}
                    <AnimatePresence>
                      {showSuggestions && filteredSuggestions.length > 0 && (
                        <motion.div
                          initial={{ opacity: 0, y: -8 }}
                          animate={{ opacity: 1, y: 0 }}
                          exit={{ opacity: 0, y: -8 }}
                          transition={{ duration: 0.15 }}
                          className="absolute left-0 right-0 top-full z-50 mt-1 max-h-48 overflow-y-auto rounded-lg border border-border bg-surface shadow-lg"
                        >
                          <p className="px-3 py-1.5 text-xs text-text-subtle border-b border-border">
                            {t("providerCard.suggestions", { ns: "settings" })}
                          </p>
                          {filteredSuggestions.map((s) => (
                            <button
                              key={s}
                              type="button"
                              onMouseDown={(e) => {
                                e.preventDefault();
                                setModelInput(s);
                                onModelChange?.(s);
                                setShowSuggestions(false);
                              }}
                              className={`flex w-full items-center justify-between px-3 py-2 text-left text-sm transition-colors hover:bg-surface-elevated ${
                                s === model ? "bg-primary/10 text-primary" : "text-text"
                              }`}
                            >
                              <span className="font-mono text-xs">{s}</span>
                              {s === model && <Check className="h-3 w-3" />}
                            </button>
                          ))}
                          {id === "openrouter" && (
                            <button
                              type="button"
                              onClick={() => handleOpenExternal("https://openrouter.ai/models")}
                              className="flex w-full items-center gap-1 px-3 py-2 text-xs text-primary hover:bg-surface-elevated border-t border-border"
                            >
                              {t("providerCard.browseAllModels", { ns: "settings" })}{" "}
                              <ExternalLink className="h-3 w-3" />
                            </button>
                          )}
                        </motion.div>
                      )}
                    </AnimatePresence>
                  </div>
                  {model && id !== "ollama" && (
                    <p className="text-xs text-text-muted">
                      {t("providerCard.currentModel", { ns: "settings" })}:{" "}
                      <span className="font-mono text-text">{model}</span>
                    </p>
                  )}
                </div>
              )}

              {/* Ollama Running Models & Management */}
              {id === "ollama" && enabled && (
                <div className="space-y-3 rounded-lg border border-border bg-surface-elevated/50 p-3">
                  <div className="flex items-center justify-between">
                    <h4 className="text-xs font-semibold uppercase tracking-wider text-text-subtle">
                      {t("providerCard.ollama.status", { ns: "settings" })}
                    </h4>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={loadOllamaData}
                      disabled={loadingOllama}
                      className="h-6 px-2 text-xs"
                    >
                      <RefreshCw
                        className={`mr-1 h-3 w-3 ${loadingOllama ? "animate-spin" : ""}`}
                      />
                      {t("actions.refresh", { ns: "common" })}
                    </Button>
                  </div>

                  {runningModels.length > 0 ? (
                    <div className="space-y-2">
                      <p className="text-xs text-text-subtle">
                        {t("providerCard.ollama.currentlyRunning", { ns: "settings" })}
                      </p>
                      {runningModels.map((m) => (
                        <div
                          key={m.id}
                          className="flex items-center justify-between rounded bg-surface p-2 text-sm"
                        >
                          <span className="font-mono font-medium">{m.name}</span>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => handleStopModel(m.id)}
                            className="h-7 px-2 text-xs text-error hover:bg-error/10 hover:text-error"
                          >
                            <Square className="mr-1 h-3 w-3 fill-current" />
                            {t("actions.stop", { ns: "common" })}
                          </Button>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="text-xs italic text-text-muted">
                      {t("providerCard.ollama.noModelsInMemory", { ns: "settings" })}
                    </p>
                  )}

                  {discoveredModels.length > 0 && (
                    <div className="space-y-2 border-t border-border pt-2">
                      <p className="text-xs text-text-subtle">
                        {t("providerCard.ollama.installedModels", { ns: "settings" })}
                      </p>
                      <div className="grid grid-cols-2 gap-2">
                        {discoveredModels.map((m) => {
                          const isRunning = runningModels.some((rm) => rm.id === m.id);
                          const isSelected = model === m.id;
                          return (
                            <div
                              key={m.id}
                              onClick={() => onModelChange?.(m.id)}
                              className={cn(
                                "flex items-center justify-between rounded border p-1.5 text-xs cursor-pointer transition-all",
                                isSelected
                                  ? "border-primary bg-primary/10 ring-1 ring-primary/30"
                                  : "border-border bg-surface/50 hover:border-border-subtle hover:bg-surface-elevated"
                              )}
                            >
                              <span
                                className={cn(
                                  "truncate pr-1 font-mono",
                                  isSelected ? "text-primary font-medium" : "text-text"
                                )}
                              >
                                {m.name}
                              </span>
                              <div className="flex items-center gap-1.5">
                                {!isRunning && (
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      handleRunModel(m.id);
                                      onModelChange?.(m.id);
                                    }}
                                    className="h-6 w-6 p-0 text-primary hover:bg-primary/20"
                                    title={t("providerCard.ollama.loadAndSelect", {
                                      ns: "settings",
                                    })}
                                  >
                                    <Play className="h-3 w-3 fill-current" />
                                  </Button>
                                )}
                                {isRunning && (
                                  <span
                                    className="flex h-2 w-2 rounded-full bg-success ring-4 ring-success/20"
                                    title={t("status.running", { ns: "common" })}
                                  />
                                )}
                              </div>
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  )}
                </div>
              )}

              {/* Model Selection - Dropdown for Anthropic/OpenAI */}
              {!isTextInputProvider && availableModels.length > 0 && (
                <div className="space-y-2">
                  <label className="text-xs font-medium text-text-subtle">
                    {t("providers.model", { ns: "settings" })}
                  </label>
                  <div className="relative">
                    <button
                      type="button"
                      onClick={() => setShowModelDropdown(!showModelDropdown)}
                      className="flex w-full items-center justify-between rounded-lg border border-border bg-surface-elevated px-3 py-2.5 text-left transition-colors hover:border-border-subtle focus:outline-none focus:ring-2 focus:ring-primary/50"
                    >
                      <div className="flex-1 min-w-0">
                        <p className="truncate text-sm font-medium text-text">
                          {selectedModelInfo?.name || selectedModel}
                        </p>
                        {selectedModelInfo?.description && (
                          <p className="truncate text-xs text-text-muted">
                            {selectedModelInfo.description}
                          </p>
                        )}
                      </div>
                      <ChevronDown
                        className={`ml-2 h-4 w-4 flex-shrink-0 text-text-muted transition-transform ${showModelDropdown ? "rotate-180" : ""}`}
                      />
                    </button>

                    <AnimatePresence>
                      {showModelDropdown && (
                        <motion.div
                          initial={{ opacity: 0, y: -8 }}
                          animate={{ opacity: 1, y: 0 }}
                          exit={{ opacity: 0, y: -8 }}
                          transition={{ duration: 0.15 }}
                          className="absolute left-0 right-0 top-full z-50 mt-1 max-h-64 overflow-y-auto rounded-lg border border-border bg-surface shadow-lg"
                        >
                          {availableModels.map((m) => (
                            <button
                              key={m.id}
                              type="button"
                              onClick={() => {
                                onModelChange?.(m.id);
                                setShowModelDropdown(false);
                              }}
                              className={`flex w-full items-center justify-between px-3 py-2.5 text-left transition-colors hover:bg-surface-elevated ${
                                m.id === selectedModel ? "bg-primary/10" : ""
                              }`}
                            >
                              <div>
                                <p className="text-sm font-medium text-text">{m.name}</p>
                                {m.description && (
                                  <p className="text-xs text-text-muted">{m.description}</p>
                                )}
                              </div>
                              {m.id === selectedModel && <Check className="h-4 w-4 text-primary" />}
                            </button>
                          ))}
                        </motion.div>
                      )}
                    </AnimatePresence>
                  </div>
                </div>
              )}

              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <label className="text-xs font-medium text-text-subtle">
                    {t("providerCard.baseUrl", { ns: "settings" })}
                  </label>
                  {!isEditingEndpoint && onEndpointChange && (
                    <button
                      type="button"
                      onClick={() => setIsEditingEndpoint(true)}
                      className="text-xs text-primary hover:underline"
                    >
                      {t("actions.edit", { ns: "common" })}
                    </button>
                  )}
                </div>
                {isEditingEndpoint ? (
                  <div className="space-y-2">
                    <div className="flex items-center gap-2">
                      <Input
                        type="text"
                        placeholder={t("providerCard.placeholders.endpoint", { ns: "settings" })}
                        value={endpointInput}
                        onChange={(e) => setEndpointInput(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            handleSaveEndpoint();
                          } else if (e.key === "Escape") {
                            setEndpointInput(endpoint);
                            setIsEditingEndpoint(false);
                          }
                        }}
                        className="flex-1"
                      />
                      {isEndpointModified && defaultEndpoint && (
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={handleResetEndpoint}
                          title={t("providerCard.resetEndpoint", { ns: "settings" })}
                          className="shrink-0"
                        >
                          <RotateCcw className="h-4 w-4" />
                        </Button>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      <Button
                        size="sm"
                        onClick={handleSaveEndpoint}
                        disabled={!endpointInput.trim()}
                      >
                        {t("actions.save", { ns: "common" })}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => {
                          setEndpointInput(endpoint);
                          setIsEditingEndpoint(false);
                        }}
                      >
                        {t("actions.cancel", { ns: "common" })}
                      </Button>
                    </div>
                  </div>
                ) : (
                  <div className="rounded-lg bg-surface-elevated p-3">
                    <p className="font-mono text-sm text-text-muted break-all">{endpoint}</p>
                  </div>
                )}
              </div>

              {!requiresApiKey ? (
                <div className="rounded-lg border border-success/30 bg-success/10 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-2">
                      <Check className="h-4 w-4 text-success" />
                      <span className="text-sm text-success">
                        {t("providerCard.noApiKeyRequired", { ns: "settings" })}
                      </span>
                    </div>
                    {docsUrl && (
                      <button
                        type="button"
                        onClick={() => handleOpenExternal(docsUrl)}
                        className="inline-flex items-center gap-1 text-xs text-success hover:underline"
                      >
                        {t("providerCard.openWebsite", { ns: "settings" })}
                        <ExternalLink className="h-3 w-3" />
                      </button>
                    )}
                  </div>
                </div>
              ) : hasKey ? (
                <div className="flex items-center justify-between rounded-lg border border-success/30 bg-success/10 p-3">
                  <div className="flex items-center gap-2">
                    <Check className="h-4 w-4 text-success" />
                    <span className="text-sm text-success">
                      {t("providers.hasKey", { ns: "settings" })}
                    </span>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={handleDeleteKey}
                    className="text-error hover:text-error"
                  >
                    <X className="mr-1 h-4 w-4" />
                    {t("providers.removeKey", { ns: "settings" })}
                  </Button>
                </div>
              ) : (
                <div className="space-y-3">
                  <div className="relative">
                    <Input
                      type={showKey ? "text" : "password"}
                      placeholder={t("providers.apiKeyPlaceholder", { ns: "settings" })}
                      value={apiKey}
                      onChange={(e) => setApiKey(e.target.value)}
                      error={error || undefined}
                    />
                    <button
                      type="button"
                      onClick={() => setShowKey(!showKey)}
                      className="absolute right-3 top-1/2 -translate-y-1/2 flex items-center justify-center text-text-subtle hover:text-text"
                    >
                      {showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                    </button>
                  </div>

                  <div className="flex items-center justify-between">
                    {docsUrl && (
                      <button
                        type="button"
                        onClick={() => handleOpenExternal(docsUrl)}
                        className="inline-flex items-center gap-1 text-sm text-primary hover:underline"
                      >
                        {t("providerCard.getApiKey", { ns: "settings" })}
                        <ExternalLink className="h-3 w-3" />
                      </button>
                    )}
                    <Button
                      onClick={handleSaveKey}
                      loading={saving}
                      disabled={!apiKey.trim()}
                      className="ml-auto"
                    >
                      {success ? (
                        <>
                          <Check className="mr-1 h-4 w-4" />
                          {t("status.success", { ns: "common" })}
                        </>
                      ) : (
                        t("providers.saveKey", { ns: "settings" })
                      )}
                    </Button>
                  </div>
                </div>
              )}

              {!isDefault && onSetDefault && hasKey && (
                <Button variant="secondary" size="sm" onClick={onSetDefault} className="w-full">
                  {t("providerCard.setDefaultProvider", { ns: "settings" })}
                </Button>
              )}
            </CardContent>
          </motion.div>
        )}
      </AnimatePresence>
    </Card>
  );
}
