import { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import { Key, Check, X, Eye, EyeOff, ExternalLink, ChevronDown } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { storeApiKey, deleteApiKey, hasApiKey, type ApiKeyType } from "@/lib/tauri";

// Popular/suggested models for providers with limited options
const PROVIDER_MODELS: Record<string, { id: string; name: string; description?: string }[]> = {
  anthropic: [
    {
      id: "claude-sonnet-4-20250514",
      name: "Claude Sonnet 4",
      description: "Latest, most intelligent",
    },
    { id: "claude-3-5-sonnet-20241022", name: "Claude 3.5 Sonnet", description: "Fast & capable" },
    { id: "claude-3-5-haiku-20241022", name: "Claude 3.5 Haiku", description: "Fastest" },
    { id: "claude-3-opus-20240229", name: "Claude 3 Opus", description: "Most capable (legacy)" },
  ],
  openai: [
    { id: "gpt-4o", name: "GPT-4o", description: "Flagship model" },
    { id: "gpt-4o-mini", name: "GPT-4o Mini", description: "Fast & affordable" },
    { id: "gpt-4-turbo", name: "GPT-4 Turbo", description: "Previous flagship" },
    { id: "o1", name: "o1", description: "Reasoning model" },
    { id: "o1-mini", name: "o1 Mini", description: "Fast reasoning" },
  ],
  opencode_zen: [
    { id: "gpt-5-nano", name: "GPT 5 Nano", description: "Free" },
    { id: "grok-code", name: "Grok Code Fast 1", description: "Free (limited time)" },
    { id: "glm-4.7-free", name: "GLM 4.7", description: "Free (limited time)" },
    { id: "big-pickle", name: "Big Pickle", description: "Free (limited time)" },
    { id: "gpt-5.2-codex", name: "GPT 5.2 Codex", description: "Premium coding" },
    { id: "claude-sonnet-4-5", name: "Claude Sonnet 4.5", description: "Premium" },
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
  model?: string;
  isDefault?: boolean;
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  onModelChange?: (model: string) => void;
  onSetDefault?: () => void;
  docsUrl?: string;
}

export function ProviderCard({
  id,
  name,
  description,
  endpoint,
  model,
  isDefault = false,
  enabled,
  onEnabledChange,
  onModelChange,
  onSetDefault,
  docsUrl,
}: ProviderCardProps) {
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [hasKey, setHasKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [showModelDropdown, setShowModelDropdown] = useState(false);
  const [modelInput, setModelInput] = useState(model || "");
  const [showSuggestions, setShowSuggestions] = useState(false);

  const isTextInputProvider = TEXT_INPUT_PROVIDERS.includes(id);
  const availableModels = PROVIDER_MODELS[id] || [];
  const suggestions = SUGGESTED_MODELS[id] || [];
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

  const handleSaveKey = async () => {
    if (!apiKey.trim()) {
      setError("API key is required");
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
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save API key");
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteKey = async () => {
    try {
      await deleteApiKey(id);
      setHasKey(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete API key");
    }
  };

  const handleOpenExternal = async (url: string) => {
    try {
      await openUrl(url);
    } catch (err) {
      console.error("Failed to open link:", err);
    }
  };

  return (
    <Card className="relative overflow-hidden">
      {isDefault && (
        <div className="absolute right-0 top-0 rounded-bl-lg bg-primary px-3 py-1 text-xs font-medium text-white">
          Default
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
                Key saved
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
              {isTextInputProvider && (
                <div className="space-y-2">
                  <label className="text-xs font-medium text-text-subtle">Model</label>
                  <div className="relative">
                    <Input
                      type="text"
                      placeholder={
                        id === "openrouter" ? "e.g., anthropic/claude-sonnet-4" : "e.g., llama3.2"
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
                        Save
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
                            Suggestions
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
                              Browse all models <ExternalLink className="h-3 w-3" />
                            </button>
                          )}
                        </motion.div>
                      )}
                    </AnimatePresence>
                  </div>
                  {model && (
                    <p className="text-xs text-text-muted">
                      Current: <span className="font-mono text-text">{model}</span>
                    </p>
                  )}
                </div>
              )}

              {/* Model Selection - Dropdown for Anthropic/OpenAI */}
              {!isTextInputProvider && availableModels.length > 0 && (
                <div className="space-y-2">
                  <label className="text-xs font-medium text-text-subtle">Model</label>
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

              <div className="rounded-lg bg-surface-elevated p-3">
                <p className="text-xs text-text-subtle">Endpoint</p>
                <p className="font-mono text-sm text-text-muted">{endpoint}</p>
              </div>

              {!requiresApiKey ? (
                <div className="rounded-lg border border-success/30 bg-success/10 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-2">
                      <Check className="h-4 w-4 text-success" />
                      <span className="text-sm text-success">
                        No API key required for local models
                      </span>
                    </div>
                    {docsUrl && (
                      <button
                        type="button"
                        onClick={() => handleOpenExternal(docsUrl)}
                        className="inline-flex items-center gap-1 text-xs text-success hover:underline"
                      >
                        Open website
                        <ExternalLink className="h-3 w-3" />
                      </button>
                    )}
                  </div>
                </div>
              ) : hasKey ? (
                <div className="flex items-center justify-between rounded-lg border border-success/30 bg-success/10 p-3">
                  <div className="flex items-center gap-2">
                    <Check className="h-4 w-4 text-success" />
                    <span className="text-sm text-success">API key configured</span>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={handleDeleteKey}
                    className="text-error hover:text-error"
                  >
                    <X className="mr-1 h-4 w-4" />
                    Remove
                  </Button>
                </div>
              ) : (
                <div className="space-y-3">
                  <div className="relative">
                    <Input
                      type={showKey ? "text" : "password"}
                      placeholder="Enter your API key"
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
                        Get API key
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
                          Saved
                        </>
                      ) : (
                        "Save Key"
                      )}
                    </Button>
                  </div>
                </div>
              )}

              {!isDefault && onSetDefault && hasKey && (
                <Button variant="secondary" size="sm" onClick={onSetDefault} className="w-full">
                  Set as Default Provider
                </Button>
              )}
            </CardContent>
          </motion.div>
        )}
      </AnimatePresence>
    </Card>
  );
}
