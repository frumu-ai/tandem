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
  ["enterprise-admin", "Enterprise", "shield"],
  ["coding", "Coder", "code"],
  ["agents", "Agents", "users"],
  ["orchestrator", "Task Board", "workflow"],
  ["files", "Files", "folder"],
  ["memory", "Memory", "database"],
  ["runs", "Runs", "activity"],
  ["control-loop", "Control Loop", "radar"],
  ["approvals", "Approvals", "shield-check"],
  ["settings", "Settings", "settings"],
  // Legacy routes kept for backwards compat (not in primary nav)
  ["packs", "Packs", "package"],
  ["teams", "Teams", "users"],
  ["channels", "Channels", "message-circle"],
  ["mcp", "MCP", "link"],
  ["incident-monitor", "Incident Monitor", "shield-alert"],
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
  "enterprise-admin",
  "coding",
  "agents",
  "orchestrator",
  "incident-monitor",
  "files",
  "memory",
  "runs",
  "control-loop",
  "approvals",
  "settings",
];

// Sidebar routes used by the control panel and command palette
export const NAV_ROUTES = NAV_ROUTE_ORDER.map((routeId) => {
  const route = ROUTES.find(([id]) => id === routeId);
  if (!route) throw new Error(`Missing navigation route: ${routeId}`);
  return route;
});

// Ordered sidebar sections. Grouping the flat 19-item list by what the user is
// trying to do (build -> operate -> govern -> system) makes the nav scannable
// and clarifies overlapping surfaces (planner/orchestrator, workflows/studio).
export const NAV_GROUPS = [
  { label: "Overview", routeIds: ["dashboard"] },
  { label: "Build", routeIds: ["chat", "planner", "studio", "workflows", "automations"] },
  { label: "Operate", routeIds: ["runs", "orchestrator", "coding", "incident-monitor"] },
  { label: "Govern", routeIds: ["approvals", "control-loop", "enterprise-admin"] },
  { label: "System", routeIds: ["agents", "memory", "files", "marketplace", "experiments", "settings"] },
];

// Group an (already visibility-filtered) list of nav routes into the ordered
// sections above. Groups with no visible routes are dropped; any visible route
// not assigned to a group falls into a trailing "More" section so a newly added
// route can never silently disappear from the sidebar.
export function groupNavRoutes(routes) {
  const byId = new Map(routes.map((route) => [route[0], route]));
  const assigned = new Set();
  const groups = [];
  for (const group of NAV_GROUPS) {
    const items = [];
    for (const id of group.routeIds) {
      const route = byId.get(id);
      if (route) {
        items.push(route);
        assigned.add(id);
      }
    }
    if (items.length) groups.push({ label: group.label, items });
  }
  const rest = routes.filter((route) => !assigned.has(route[0]));
  if (rest.length) groups.push({ label: "More", items: rest });
  return groups;
}

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
