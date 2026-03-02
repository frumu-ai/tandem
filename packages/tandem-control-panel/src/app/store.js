export const ROUTES = [
  ["dashboard", "Dashboard", "home"],
  ["chat", "Chat", "message-square"],
  ["agents", "Automations", "clock"],
  ["channels", "Channels", "message-circle"],
  ["mcp", "MCP", "link"],
  ["packs", "Packs", "package"],
  ["swarm", "Swarm (Live)", "share-2"],
  ["files", "Files", "folder-open"],
  ["memory", "Memory", "database"],
  ["teams", "Teams", "users"],
  ["feed", "Live Feed", "radio"],
  ["settings", "Settings", "settings"],
];

export const NAV_ROUTES = ROUTES.filter(
  ([id]) => !["channels", "mcp", "packs", "files"].includes(id)
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
  openrouter: {
    label: "OpenRouter",
    keyUrl: "https://openrouter.ai/settings/keys",
    placeholder: "sk-or-v1-...",
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
