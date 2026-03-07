// All registered routes (for router/command palette)
export const ROUTES = [
  ["dashboard", "Dashboard", "home"],
  ["chat", "Chat", "message-square"],
  ["automations", "Automations", "bot"],
  ["orchestrator", "Orchestrator", "sparkles"],
  ["memory", "Memory", "database"],
  ["feed", "Live Feed", "radio"],
  ["settings", "Settings", "settings"],
  // Legacy routes kept for backwards compat (not in primary nav)
  ["agents", "Routines", "clock"],
  ["packs", "Packs", "package"],
  ["teams", "Teams", "users"],
  ["channels", "Channels", "message-circle"],
  ["mcp", "MCP", "link"],
  ["failure-reporter", "Bug Monitor", "bug-play"],
  ["files", "Files", "folder-open"],
  // Internal detail routes (not in primary nav)
  ["packs-detail", "Packs", "package"],
  ["teams-detail", "Teams", "users"],
];

// Primary nav — 7 items your grandma can understand
export const NAV_ROUTES = ROUTES.filter(([id]) =>
  ["dashboard", "chat", "automations", "orchestrator", "memory", "feed", "settings"].includes(id)
);

export const providerHints = {
  openai: {
    label: "OpenAI",
    keyUrl: "https://platform.openai.com/api-keys",
    placeholder: "sk-proj-...",
  },
  anthropic: {
    label: "Anthropic",
    keyUrl: "https://console.anthropic.com/settings/keys",
    placeholder: "sk-ant-...",
  },
  google: {
    label: "Google",
    keyUrl: "https://aistudio.google.com/app/apikey",
    placeholder: "AIza...",
  },
  groq: { label: "Groq", keyUrl: "https://console.groq.com/keys", placeholder: "gsk_..." },
  mistral: { label: "Mistral", keyUrl: "https://console.mistral.ai/api-keys/", placeholder: "..." },
  together: {
    label: "Together",
    keyUrl: "https://api.together.xyz/settings/api-keys",
    placeholder: "...",
  },
  cohere: {
    label: "Cohere",
    keyUrl: "https://dashboard.cohere.com/api-keys",
    placeholder: "...",
  },
  openrouter: {
    label: "OpenRouter",
    keyUrl: "https://openrouter.ai/settings/keys",
    placeholder: "sk-or-v1-...",
  },
  azure: {
    label: "Azure OpenAI",
    keyUrl: "https://portal.azure.com/",
    placeholder: "...",
  },
  bedrock: {
    label: "Bedrock",
    keyUrl: "https://console.aws.amazon.com/bedrock/",
    placeholder: "...",
  },
  vertex: {
    label: "Vertex",
    keyUrl: "https://console.cloud.google.com/vertex-ai",
    placeholder: "...",
  },
  copilot: {
    label: "GitHub Copilot",
    keyUrl: "https://github.com/settings/tokens",
    placeholder: "ghp_...",
  },
  ollama: { label: "Ollama", keyUrl: "", placeholder: "No key required" },
};

export function createState() {
  return {
    authed: false,
    route: "dashboard",
    me: null,
    client: null,
    needsProviderOnboarding: false,
    providerReady: false,
    providerDefault: "",
    providerDefaultModel: "",
    providerConnected: [],
    providerError: "",
    providerGateNoticeShown: false,
    botName: "Tandem",
    botAvatarUrl: "",
    controlPanelName: "Tandem Control Panel",
    themeId: "charcoal_fire",
    currentSessionId: "",
    chatUploadedFiles: [],
    filesDir: "uploads",
    cleanup: [],
    toasts: [],
  };
}
