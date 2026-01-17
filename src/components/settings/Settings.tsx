import { useState, useEffect } from "react";
import { motion } from "framer-motion";
import { Settings as SettingsIcon, FolderOpen, Shield, Cpu } from "lucide-react";
import { ProviderCard } from "./ProviderCard";
import { Button } from "@/components/ui/Button";
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from "@/components/ui/Card";
import {
  getProvidersConfig,
  setProvidersConfig,
  setWorkspacePath,
  getWorkspacePath,
  type ProvidersConfig,
} from "@/lib/tauri";
import { open } from "@tauri-apps/plugin-dialog";

interface SettingsProps {
  onClose?: () => void;
}

export function Settings({ onClose }: SettingsProps) {
  const [providers, setProviders] = useState<ProvidersConfig | null>(null);
  const [workspacePath, setWorkspace] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    async function loadSettings() {
      try {
        const [config, workspace] = await Promise.all([getProvidersConfig(), getWorkspacePath()]);
        setProviders(config);
        setWorkspace(workspace);
      } catch (err) {
        console.error("Failed to load settings:", err);
      } finally {
        setLoading(false);
      }
    }
    loadSettings();
  }, []);

  const handleProviderChange = async (
    provider: keyof Omit<ProvidersConfig, "custom">,
    enabled: boolean
  ) => {
    if (!providers) return;

    const updated = {
      ...providers,
      [provider]: { ...providers[provider], enabled },
    };
    setProviders(updated);
    await setProvidersConfig(updated);
  };

  const handleSetDefault = async (provider: keyof Omit<ProvidersConfig, "custom">) => {
    if (!providers) return;

    // Reset all defaults and set the new one
    const updated: ProvidersConfig = {
      openrouter: { ...providers.openrouter, default: provider === "openrouter" },
      anthropic: { ...providers.anthropic, default: provider === "anthropic" },
      openai: { ...providers.openai, default: provider === "openai" },
      ollama: { ...providers.ollama, default: provider === "ollama" },
      custom: providers.custom,
    };
    setProviders(updated);
    await setProvidersConfig(updated);
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
  };

  const handleSelectWorkspace = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Workspace Folder",
      });

      if (selected && typeof selected === "string") {
        await setWorkspacePath(selected);
        setWorkspace(selected);
      }
    } catch (err) {
      console.error("Failed to select workspace:", err);
    }
  };

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-primary border-t-transparent" />
      </div>
    );
  }

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
              <p className="text-text-muted">Configure your Tandem workspace</p>
            </div>
          </div>
          {onClose && (
            <Button variant="ghost" onClick={onClose}>
              Close
            </Button>
          )}
        </div>

        {/* Workspace Section */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-3">
              <FolderOpen className="h-5 w-5 text-secondary" />
              <div>
                <CardTitle>Workspace</CardTitle>
                <CardDescription>
                  Select the folder where Tandem can read and write files
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-4">
              <div className="flex-1 rounded-lg bg-surface-elevated p-3">
                {workspacePath ? (
                  <p className="font-mono text-sm text-text">{workspacePath}</p>
                ) : (
                  <p className="text-sm text-text-subtle">No workspace selected</p>
                )}
              </div>
              <Button onClick={handleSelectWorkspace}>
                {workspacePath ? "Change" : "Select Folder"}
              </Button>
            </div>
            <p className="mt-3 text-xs text-text-subtle">
              <Shield className="mr-1 inline h-3 w-3" />
              Tandem can only access files within this folder. Sensitive files (.env, .ssh, etc.)
              are always blocked.
            </p>
          </CardContent>
        </Card>

        {/* LLM Providers Section */}
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
                docsUrl="https://openrouter.ai/keys"
              />

              <ProviderCard
                id="anthropic"
                name="Anthropic"
                description="Direct access to Claude models"
                endpoint="https://api.anthropic.com"
                model={providers.anthropic.model}
                isDefault={providers.anthropic.default}
                enabled={providers.anthropic.enabled}
                onEnabledChange={(enabled) => handleProviderChange("anthropic", enabled)}
                onModelChange={(model) => handleModelChange("anthropic", model)}
                onSetDefault={() => handleSetDefault("anthropic")}
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
                docsUrl="https://ollama.ai"
              />
            </div>
          )}
        </div>

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
    </motion.div>
  );
}
