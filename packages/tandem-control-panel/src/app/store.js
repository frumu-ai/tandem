// All registered routes (for router/command palette)
export const ROUTES = [
  ["dashboard", "Dashboard", "home"],
  ["chat", "Chat", "message-square"],
  ["planner", "Planner", "compass"],
  ["workflows", "Workflows", "network"],
  ["marketplace", "Marketplace", "globe"],
  ["studio", "Studio", "blocks"],
  ["automations", "Automations", "bot"],
  ["experiments", "Experiments", "flask-conical"],
  ["coding", "Coding", "code"],
  ["agents", "Agents", "users"],
  ["orchestrator", "Task Board", "workflow"],
  ["files", "Files", "folder"],
  ["memory", "Memory", "database"],
  ["runs", "Runs", "activity"],
  ["approvals", "Approvals", "shield-check"],
  ["settings", "Settings", "settings"],
  // Legacy routes kept for backwards compat (not in primary nav)
  ["packs", "Packs", "package"],
  ["teams", "Teams", "users"],
  ["channels", "Channels", "message-circle"],
  ["mcp", "MCP", "link"],
  ["bug-monitor", "Bug Monitor", "bug-play"],
  // Internal detail routes (not in primary nav)
  ["packs-detail", "Packs", "package"],
  ["teams-detail", "Teams", "users"],
];

const NAV_ROUTE_ORDER = [
  "dashboard",
  "chat",
  "planner",
  "workflows",
  "marketplace",
  "studio",
  "automations",
  "experiments",
  "coding",
  "agents",
  "orchestrator",
  "files",
  "memory",
  "runs",
  "approvals",
  "settings",
];

// Sidebar routes used by the control panel and command palette
export const NAV_ROUTES = NAV_ROUTE_ORDER.map((routeId) => {
  const route = ROUTES.find(([id]) => id === routeId);
  if (!route) throw new Error(`Missing navigation route: ${routeId}`);
  return route;
});

export const providerHints = {
  openai: {
    label: "OpenAI",
    keyUrl: "https://platform.openai.com/api-keys",
    placeholder: "sk-proj-...",
  },
  "openai-codex": {
    label: "Codex Account",
    keyUrl: "",
    placeholder: "Browser sign-in required",
    authMode: "oauth",
    description:
      "Use your ChatGPT/Codex subscription on this machine without pasting a separate API key.",
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
  llama_cpp: {
    label: "llama.cpp",
    keyUrl: "",
    placeholder: "No key required",
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
