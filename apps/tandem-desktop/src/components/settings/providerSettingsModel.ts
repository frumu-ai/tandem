import type { ProviderConfig, ProvidersConfig } from "@/lib/tauri";

export const PROVIDER_SLOTS = [
  "opencode_zen",
  "openai-codex",
  "openrouter",
  "anthropic",
  "openai",
  "groq",
  "mistral",
  "together",
  "cohere",
  "llama_cpp",
  "ollama",
  "poe",
  "azure",
  "bedrock",
  "vertex",
  "copilot",
] as const;

export type ProviderSlot = (typeof PROVIDER_SLOTS)[number];

export const PROVIDER_CARD_DEFINITIONS: {
  slot: ProviderSlot;
  name: string;
  defaultEndpoint: string;
  docsUrl?: string;
  supportsOAuth?: boolean;
}[] = [
  {
    slot: "opencode_zen",
    name: "Opencode Zen",
    defaultEndpoint: "https://opencode.ai/zen/v1",
    docsUrl: "https://opencode.ai/auth",
  },
  {
    slot: "openai-codex",
    name: "OpenAI Codex",
    defaultEndpoint: "https://chatgpt.com/backend-api/codex",
    docsUrl: "https://chatgpt.com/codex",
    supportsOAuth: true,
  },
  {
    slot: "openrouter",
    name: "OpenRouter",
    defaultEndpoint: "https://openrouter.ai/api/v1",
    docsUrl: "https://openrouter.ai/keys",
  },
  {
    slot: "anthropic",
    name: "Anthropic",
    defaultEndpoint: "https://api.anthropic.com",
    docsUrl: "https://console.anthropic.com/settings/keys",
  },
  {
    slot: "openai",
    name: "OpenAI",
    defaultEndpoint: "https://api.openai.com/v1",
    docsUrl: "https://platform.openai.com/api-keys",
  },
  {
    slot: "groq",
    name: "Groq",
    defaultEndpoint: "https://api.groq.com/openai/v1",
    docsUrl: "https://console.groq.com/keys",
  },
  {
    slot: "mistral",
    name: "Mistral",
    defaultEndpoint: "https://api.mistral.ai/v1",
    docsUrl: "https://console.mistral.ai/api-keys",
  },
  {
    slot: "together",
    name: "Together",
    defaultEndpoint: "https://api.together.xyz/v1",
    docsUrl: "https://api.together.xyz/settings/api-keys",
  },
  {
    slot: "cohere",
    name: "Cohere",
    defaultEndpoint: "https://api.cohere.com/v2",
    docsUrl: "https://dashboard.cohere.com/api-keys",
  },
  {
    slot: "llama_cpp",
    name: "llama.cpp",
    defaultEndpoint: "http://127.0.0.1:8080/v1",
    docsUrl: "https://github.com/ggml-org/llama.cpp",
  },
  {
    slot: "ollama",
    name: "Ollama",
    defaultEndpoint: "http://localhost:11434",
    docsUrl: "https://ollama.com",
  },
  {
    slot: "poe",
    name: "Poe",
    defaultEndpoint: "https://api.poe.com/v1",
    docsUrl: "https://poe.com/api",
  },
  {
    slot: "azure",
    name: "Azure OpenAI-compatible",
    defaultEndpoint: "https://example.openai.azure.com/openai/deployments/default",
    docsUrl: "https://learn.microsoft.com/azure/ai-services/openai/",
  },
  {
    slot: "bedrock",
    name: "Amazon Bedrock-compatible",
    defaultEndpoint: "https://bedrock-runtime.us-east-1.amazonaws.com",
    docsUrl: "https://docs.aws.amazon.com/bedrock/",
  },
  {
    slot: "vertex",
    name: "Vertex-compatible",
    defaultEndpoint: "https://aiplatform.googleapis.com/v1",
    docsUrl: "https://cloud.google.com/vertex-ai/generative-ai/docs",
  },
  {
    slot: "copilot",
    name: "GitHub Copilot-compatible",
    defaultEndpoint: "https://api.githubcopilot.com",
    docsUrl: "https://docs.github.com/copilot",
  },
];

export function updateProviderSlots(
  config: ProvidersConfig,
  updater: (slot: ProviderSlot, value: ProviderConfig) => ProviderConfig
): ProvidersConfig {
  const next: ProvidersConfig = { ...config };
  for (const slot of PROVIDER_SLOTS) {
    next[slot] = updater(slot, config[slot]);
  }
  return next;
}

export function selectedModelForProvider(
  config: ProvidersConfig,
  provider: ProviderSlot
): ProvidersConfig["selected_model"] {
  const modelId = config[provider].model?.trim();
  return modelId ? { provider_id: provider, model_id: modelId } : null;
}

export function providerMatchesSelected(
  selectedModel: ProvidersConfig["selected_model"] | undefined | null,
  provider: ProviderSlot
) {
  const selectedProvider = selectedModel?.provider_id?.trim();
  if (!selectedProvider) return false;
  return (
    selectedProvider === provider ||
    (provider === "opencode_zen" && selectedProvider === "opencode")
  );
}

export function providerAuthReady(provider: ProviderConfig, providerId: ProviderSlot) {
  return (
    provider.has_key ||
    providerId === "opencode_zen" ||
    providerId === "llama_cpp" ||
    providerId === "ollama"
  );
}
