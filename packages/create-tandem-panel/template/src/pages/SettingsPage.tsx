import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import {
  AnimatedPage,
  Badge,
  DetailDrawer,
  PanelCard,
  SplitView,
  StaggerGroup,
  Toolbar,
} from "../ui/index.tsx";
import { ThemePicker } from "../ui/ThemePicker.tsx";
import { providerHints } from "../app/store.js";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

type BrowserBlockingIssue = {
  code?: string;
  message?: string;
};

type BrowserBinaryStatus = {
  found?: boolean;
  path?: string | null;
  version?: string | null;
  channel?: string | null;
};

type BrowserStatusResponse = {
  enabled?: boolean;
  runnable?: boolean;
  headless_default?: boolean;
  sidecar?: BrowserBinaryStatus;
  browser?: BrowserBinaryStatus;
  blocking_issues?: BrowserBlockingIssue[];
  recommendations?: string[];
  install_hints?: string[];
  last_error?: string | null;
};

type BrowserSmokeTestResponse = {
  ok?: boolean;
  url?: string;
  final_url?: string;
  title?: string;
  load_state?: string;
  element_count?: number;
  excerpt?: string | null;
  closed?: boolean;
};

type SettingsSection =
  | "providers"
  | "identity"
  | "theme"
  | "channels"
  | "mcp"
  | "incident_monitor"
  | "browser";

type IncidentMonitorConfigRow = {
  enabled?: boolean;
  paused?: boolean;
  workspace_root?: string | null;
  repo?: string | null;
  mcp_server?: string | null;
  provider_preference?: string | null;
  model_policy?: {
    default_model?: {
      provider_id?: string | null;
      model_id?: string | null;
    };
  } | null;
  auto_create_new_issues?: boolean;
  require_approval_for_new_issues?: boolean;
  auto_comment_on_matched_open_issues?: boolean;
  label_mode?: string | null;
};

type IncidentMonitorStatusRow = {
  config?: IncidentMonitorConfigRow;
  readiness?: Record<string, boolean>;
  runtime?: {
    monitoring_active?: boolean;
    paused?: boolean;
    pending_incidents?: number;
    total_incidents?: number;
    last_processed_at_ms?: number | null;
    last_incident_event_type?: string | null;
    last_runtime_error?: string | null;
  };
  required_capabilities?: Record<string, boolean>;
  missing_required_capabilities?: string[];
  resolved_capabilities?: Array<{
    capability_id?: string;
    provider?: string;
    tool_name?: string;
    binding_index?: number;
  }>;
  discovered_mcp_tools?: string[];
  selected_server_binding_candidates?: Array<{
    capability_id?: string;
    binding_tool_name?: string;
    aliases?: string[];
    matched?: boolean;
  }>;
  binding_source_version?: string | null;
  bindings_last_merged_at_ms?: number | null;
  selected_model?: {
    provider_id?: string | null;
    model_id?: string | null;
  } | null;
  pending_drafts?: number;
  pending_posts?: number;
  last_activity_at_ms?: number | null;
  last_error?: string | null;
};

type IncidentMonitorIncidentRow = {
  incident_id: string;
  fingerprint: string;
  event_type: string;
  status: string;
  repo: string;
  workspace_root: string;
  title: string;
  detail?: string | null;
  excerpt?: string[];
  occurrence_count?: number;
  created_at_ms: number;
  updated_at_ms: number;
  draft_id?: string | null;
  triage_run_id?: string | null;
  last_error?: string | null;
};

type IncidentMonitorDraftRow = {
  draft_id: string;
  fingerprint: string;
  repo: string;
  status: string;
  created_at_ms: number;
  triage_run_id?: string | null;
  issue_number?: number | null;
  title?: string | null;
  detail?: string | null;
  github_status?: string | null;
  github_issue_url?: string | null;
  github_comment_url?: string | null;
  github_posted_at_ms?: number | null;
  matched_issue_number?: number | null;
  matched_issue_state?: string | null;
  evidence_digest?: string | null;
  last_post_error?: string | null;
};

type IncidentMonitorPostRow = {
  post_id: string;
  draft_id: string;
  repo: string;
  operation: string;
  status: string;
  issue_number?: number | null;
  issue_url?: string | null;
  comment_url?: string | null;
  error?: string | null;
  updated_at_ms?: number | null;
};

type ChannelConfigRow = {
  has_token?: boolean;
  token_masked?: string;
  allowed_users?: string[];
  mention_only?: boolean;
  guild_id?: string;
  channel_id?: string;
  style_profile?: string;
  security_profile?: string;
};

type ChannelStatusRow = {
  enabled?: boolean;
  connected?: boolean;
  last_error?: string | null;
  active_sessions?: number;
  meta?: Record<string, unknown>;
};

type ChannelDraft = {
  botToken: string;
  allowedUsers: string;
  mentionOnly: boolean;
  guildId: string;
  channelId: string;
  styleProfile: string;
  securityProfile: string;
};

type ChannelToolPreferencesRow = {
  enabled_tools: string[];
  disabled_tools: string[];
  enabled_mcp_servers: string[];
};

const BUILTIN_PROVIDER_IDS = new Set([
  "openai",
  "openrouter",
  "anthropic",
  "ollama",
  "groq",
  "mistral",
  "together",
  "azure",
  "bedrock",
  "vertex",
  "copilot",
  "cohere",
]);

const CHANNEL_TOOL_GROUPS = [
  { label: "File", tools: ["read", "glob", "ls", "list", "grep", "codesearch", "search"] },
  { label: "Web", tools: ["websearch", "webfetch", "webfetch_html"] },
  { label: "Terminal", tools: ["bash", "write", "edit", "apply_patch"] },
  { label: "Memory", tools: ["memory_search", "memory_store", "memory_list"] },
  { label: "Other", tools: ["skill", "task", "question", "pack_builder"] },
] as const;
const PUBLIC_DEMO_ALLOWED_TOOLS = [
  "websearch",
  "webfetch",
  "webfetch_html",
  "memory_search",
  "memory_store",
  "memory_list",
] as const;

type McpServerRow = {
  name: string;
  transport: string;
  connected: boolean;
  enabled: boolean;
  lastError: string;
  headers: Record<string, string>;
  toolCache: any[];
};

type McpCatalogServer = {
  slug: string;
  name: string;
  description: string;
  transportUrl: string;
  serverConfigName: string;
  documentationUrl: string;
  directoryUrl: string;
  toolCount: number;
  requiresAuth: boolean;
  requiresSetup: boolean;
};

type McpCatalog = {
  generatedAt: string;
  count: number;
  servers: McpCatalogServer[];
};

function parseUrl(input: string) {
  try {
    return new URL(input);
  } catch {
    return null;
  }
}

function normalizeMcpName(raw: string) {
  const cleaned = String(raw || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return cleaned || "mcp-server";
}

function inferMcpNameFromTransport(transport: string) {
  const url = parseUrl(transport);
  if (!url) return "";
  const host = String(url.hostname || "").toLowerCase();
  if (!host) return "";
  if (host.endsWith("composio.dev")) return "composio";
  const parts = host.split(".").filter(Boolean);
  if (!parts.length) return "";
  const preferred = ["backend", "api", "mcp", "www"].includes(parts[0])
    ? parts[1] || parts[0]
    : parts[0];
  return normalizeMcpName(preferred);
}

function isComposioTransport(transport: string) {
  const url = parseUrl(transport);
  if (!url) return false;
  return String(url.hostname || "")
    .toLowerCase()
    .endsWith("composio.dev");
}

function isGithubCopilotMcpTransport(transport: string) {
  const url = parseUrl(transport);
  if (!url) return false;
  const host = String(url.hostname || "").toLowerCase();
  return host === "api.githubcopilot.com" || host.endsWith(".githubcopilot.com");
}

function normalizeMcpHeaderRows(rows: Array<{ key: string; value: string }>) {
  return rows.map((row) => ({
    key: String(row?.key || "").trim(),
    value: String(row?.value || "").trim(),
  }));
}

function mergeMcpHeaders(
  base: Record<string, string>,
  extraRows: Array<{ key: string; value: string }>
) {
  const merged: Record<string, string> = { ...base };
  for (const row of normalizeMcpHeaderRows(extraRows)) {
    if (!row.key || !row.value) continue;
    merged[row.key] = row.value;
  }
  return merged;
}

function buildMcpHeaders({
  authMode,
  token,
  customHeader,
  transport,
}: {
  authMode: string;
  token: string;
  customHeader: string;
  transport: string;
}) {
  const rawToken = String(token || "").trim();
  if (!rawToken || authMode === "none") return {};
  if (authMode === "custom") {
    const headerName = String(customHeader || "").trim();
    if (!headerName) throw new Error("Custom header name is required.");
    return { [headerName]: rawToken };
  }
  if (authMode === "x-api-key") return { "x-api-key": rawToken };
  if (authMode === "bearer") {
    const bearerToken = rawToken.replace(/^bearer\s+/i, "").trim();
    return { Authorization: `Bearer ${bearerToken}` };
  }
  if (isComposioTransport(transport)) return { "x-api-key": rawToken };
  const bearerToken = rawToken.replace(/^bearer\s+/i, "").trim();
  return { Authorization: `Bearer ${bearerToken}` };
}

function mcpAuthPreview(authMode: string, token: string, customHeader: string, transport: string) {
  if (!String(token || "").trim() || authMode === "none") return "No auth header will be sent.";
  if (authMode === "custom") {
    return customHeader ? `Header preview: ${customHeader}: <token>` : "Set a custom header name.";
  }
  if (authMode === "x-api-key") return "Header preview: x-api-key: <token>";
  if (authMode === "bearer") return "Header preview: Authorization: Bearer <token>";
  if (isComposioTransport(transport)) return "Auto mode: selected x-api-key for this endpoint";
  return "Auto mode: using Authorization Bearer token";
}

function normalizeMcpServerRow(input: any, fallbackName = ""): McpServerRow | null {
  if (!input || typeof input !== "object") return null;
  const row = input;
  const name = String(row.name || fallbackName || "").trim();
  if (!name) return null;
  return {
    name,
    transport: String(row.transport || "").trim(),
    connected: !!row.connected,
    enabled: row.enabled !== false,
    lastError: String(row.last_error || row.lastError || "").trim(),
    headers: row.headers && typeof row.headers === "object" ? row.headers : {},
    toolCache: Array.isArray(row.tool_cache || row.toolCache)
      ? row.tool_cache || row.toolCache
      : [],
  };
}

function normalizeMcpServers(raw: any): McpServerRow[] {
  if (Array.isArray(raw)) {
    return raw
      .map((entry) => normalizeMcpServerRow(entry))
      .filter((row): row is McpServerRow => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  if (!raw || typeof raw !== "object") return [];
  if (Array.isArray(raw.servers)) {
    return raw.servers
      .map((entry: any) => normalizeMcpServerRow(entry))
      .filter((row): row is McpServerRow => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  return Object.entries(raw)
    .map(([name, cfg]) =>
      normalizeMcpServerRow(
        cfg && typeof cfg === "object" ? cfg : { transport: String(cfg || "") },
        name
      )
    )
    .filter((row): row is McpServerRow => !!row)
    .sort((a, b) => a.name.localeCompare(b.name));
}

function normalizeMcpTools(raw: any): string[] {
  const rows = Array.isArray(raw) ? raw : Array.isArray(raw?.tools) ? raw.tools : [];
  return rows
    .map((tool: any) => {
      if (typeof tool === "string") return tool;
      if (!tool || typeof tool !== "object") return "";
      return String(
        tool.namespaced_name ||
          tool.namespacedName ||
          tool.id ||
          tool.tool_name ||
          tool.toolName ||
          ""
      ).trim();
    })
    .filter(Boolean);
}

function normalizeMcpCatalog(raw: any): McpCatalog {
  const catalog = raw && typeof raw === "object" ? raw : {};
  const list = Array.isArray(catalog.servers) ? catalog.servers : [];
  return {
    generatedAt: String(catalog.generated_at || "").trim(),
    count: Number.isFinite(Number(catalog.count)) ? Number(catalog.count) : list.length,
    servers: list
      .map((row: any) => {
        if (!row || typeof row !== "object") return null;
        return {
          slug: String(row.slug || "").trim(),
          name: String(row.name || row.slug || "").trim(),
          description: String(row.description || "").trim(),
          transportUrl: String(row.transport_url || "").trim(),
          serverConfigName: String(row.server_config_name || row.slug || "").trim(),
          documentationUrl: String(row.documentation_url || "").trim(),
          directoryUrl: String(row.directory_url || "").trim(),
          toolCount: Number.isFinite(Number(row.tool_count)) ? Number(row.tool_count) : 0,
          requiresAuth: row.requires_auth !== false,
          requiresSetup: !!row.requires_setup,
        };
      })
      .filter((row): row is McpCatalogServer => !!row && !!row.slug && !!row.transportUrl)
      .sort((a, b) => a.name.localeCompare(b.name)),
  };
}

function normalizeChannelDraft(
  channel: string,
  config: ChannelConfigRow | null | undefined
): ChannelDraft {
  const row = config && typeof config === "object" ? config : {};
  return {
    botToken: "",
    allowedUsers: Array.isArray(row.allowed_users) ? row.allowed_users.join(", ") : "",
    mentionOnly: row.mention_only !== false && channel === "discord" ? true : !!row.mention_only,
    guildId: String(row.guild_id || "").trim(),
    channelId: String(row.channel_id || "").trim(),
    styleProfile: String(row.style_profile || "default").trim() || "default",
    securityProfile: String(row.security_profile || "operator").trim() || "operator",
  };
}

function defaultChannelToolPreferences(): ChannelToolPreferencesRow {
  return {
    enabled_tools: [],
    disabled_tools: [],
    enabled_mcp_servers: [],
  };
}

function uniqueChannelValues(values: string[]) {
  return Array.from(new Set(values.map((value) => value.trim()).filter(Boolean)));
}

function normalizeChannelToolPreferences(raw: any): ChannelToolPreferencesRow {
  const row = raw && typeof raw === "object" ? raw : {};
  return {
    enabled_tools: Array.isArray(row.enabled_tools)
      ? uniqueChannelValues(row.enabled_tools.map((value: any) => String(value)))
      : [],
    disabled_tools: Array.isArray(row.disabled_tools)
      ? uniqueChannelValues(row.disabled_tools.map((value: any) => String(value)))
      : [],
    enabled_mcp_servers: Array.isArray(row.enabled_mcp_servers)
      ? uniqueChannelValues(row.enabled_mcp_servers.map((value: any) => String(value)))
      : [],
  };
}

function channelToolEnabled(prefs: ChannelToolPreferencesRow, tool: string) {
  if (prefs.disabled_tools.includes(tool)) return false;
  return prefs.enabled_tools.length === 0 || prefs.enabled_tools.includes(tool);
}

function nextChannelToolPreferences(
  prefs: ChannelToolPreferencesRow,
  tool: string,
  enabled: boolean
): ChannelToolPreferencesRow {
  const disabled = prefs.disabled_tools.filter((entry) => entry !== tool);
  const explicitEnabled = prefs.enabled_tools.length > 0 ? [...prefs.enabled_tools] : [];
  if (enabled) {
    return {
      ...prefs,
      disabled_tools: disabled,
      enabled_tools:
        explicitEnabled.length > 0 ? uniqueChannelValues([...explicitEnabled, tool]) : [],
    };
  }
  return {
    ...prefs,
    disabled_tools: uniqueChannelValues([...disabled, tool]),
    enabled_tools:
      explicitEnabled.length > 0
        ? explicitEnabled.filter((entry) => entry !== tool)
        : explicitEnabled,
  };
}

function nextChannelMcpPreferences(
  prefs: ChannelToolPreferencesRow,
  server: string,
  enabled: boolean
): ChannelToolPreferencesRow {
  const servers = prefs.enabled_mcp_servers.filter((entry) => entry !== server);
  return {
    ...prefs,
    enabled_mcp_servers: enabled ? uniqueChannelValues([...servers, server]) : servers,
  };
}

function toolAllowedForSecurityProfile(securityProfile: string, tool: string) {
  if (securityProfile !== "public_demo") return true;
  return PUBLIC_DEMO_ALLOWED_TOOLS.includes(tool as (typeof PUBLIC_DEMO_ALLOWED_TOOLS)[number]);
}

function toolEnabledForSecurityProfile(
  prefs: ChannelToolPreferencesRow,
  tool: string,
  securityProfile: string
) {
  return toolAllowedForSecurityProfile(securityProfile, tool) && channelToolEnabled(prefs, tool);
}

function parseAllowedUsers(input: string) {
  const users = String(input || "")
    .split(",")
    .map((row) => row.trim())
    .filter(Boolean);
  return users.length ? users : ["*"];
}

function providerCatalogBadge(provider: any, modelCount: number) {
  const source = String(provider?.catalog_source || "")
    .trim()
    .toLowerCase();
  if (source === "remote" && modelCount > 0) {
    return { tone: "ok" as const, text: `${modelCount} models` };
  }
  if (source === "config" && modelCount > 0) {
    return { tone: "info" as const, text: "configured models" };
  }
  return { tone: "warn" as const, text: "manual entry" };
}

function providerCatalogSubtitle(provider: any, defaultModel: string) {
  const catalogMessage = String(provider?.catalog_message || "").trim();
  if (catalogMessage) return catalogMessage;
  return `Default model: ${defaultModel || "none"}`;
}

export function SettingsPage({
  client,
  api,
  toast,
  navigate,
  currentRoute,
  identity,
  themes,
  setTheme,
  themeId,
  refreshProviderStatus,
  refreshIdentityStatus,
}: AppPageProps) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [modelSearchByProvider, setModelSearchByProvider] = useState<Record<string, string>>({});
  const [botName, setBotName] = useState(String(identity?.botName || "Tandem"));
  const [botAvatarUrl, setBotAvatarUrl] = useState(String(identity?.botAvatarUrl || ""));
  const [botControlPanelAlias, setBotControlPanelAlias] = useState("Control Center");
  const [activeSection, setActiveSection] = useState<SettingsSection>("providers");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [githubMcpGuideOpen, setGithubMcpGuideOpen] = useState(false);
  const [providerDefaultsOpen, setProviderDefaultsOpen] = useState(false);
  const [customProviderId, setCustomProviderId] = useState("custom");
  const [customProviderUrl, setCustomProviderUrl] = useState("");
  const [customProviderModel, setCustomProviderModel] = useState("");
  const [customProviderApiKey, setCustomProviderApiKey] = useState("");
  const [customProviderMakeDefault, setCustomProviderMakeDefault] = useState(true);
  const [channelDrafts, setChannelDrafts] = useState<Record<string, ChannelDraft>>({});
  const [channelVerifyResult, setChannelVerifyResult] = useState<Record<string, any>>({});
  const [mcpModalOpen, setMcpModalOpen] = useState(false);
  const [mcpName, setMcpName] = useState("");
  const [mcpTransport, setMcpTransport] = useState("");
  const [mcpAuthMode, setMcpAuthMode] = useState("none");
  const [mcpToken, setMcpToken] = useState("");
  const [mcpCustomHeader, setMcpCustomHeader] = useState("");
  const [mcpGithubToolsets, setMcpGithubToolsets] = useState("");
  const [mcpExtraHeaders, setMcpExtraHeaders] = useState<Array<{ key: string; value: string }>>([]);
  const [mcpConnectAfterAdd, setMcpConnectAfterAdd] = useState(true);
  const [mcpEditingName, setMcpEditingName] = useState("");
  const [mcpModalTab, setMcpModalTab] = useState<"manual" | "catalog">("manual");
  const [mcpCatalogSearch, setMcpCatalogSearch] = useState("");
  const [incidentMonitorEnabled, setIncidentMonitorEnabled] = useState(false);
  const [incidentMonitorPaused, setIncidentMonitorPaused] = useState(false);
  const [incidentMonitorWorkspaceRoot, setIncidentMonitorWorkspaceRoot] = useState("");
  const [incidentMonitorRepo, setIncidentMonitorRepo] = useState("");
  const [incidentMonitorMcpServer, setIncidentMonitorMcpServer] = useState("");
  const [incidentMonitorProviderPreference, setIncidentMonitorProviderPreference] =
    useState("auto");
  const [incidentMonitorProviderId, setIncidentMonitorProviderId] = useState("");
  const [incidentMonitorModelId, setIncidentMonitorModelId] = useState("");
  const [incidentMonitorAutoCreateIssues, setIncidentMonitorAutoCreateIssues] = useState(true);
  const [incidentMonitorRequireApproval, setIncidentMonitorRequireApproval] = useState(false);
  const [incidentMonitorAutoComment, setIncidentMonitorAutoComment] = useState(true);
  const [incidentMonitorWorkspaceBrowserOpen, setIncidentMonitorWorkspaceBrowserOpen] =
    useState(false);
  const [incidentMonitorWorkspaceBrowserDir, setIncidentMonitorWorkspaceBrowserDir] = useState("");
  const [incidentMonitorWorkspaceBrowserSearch, setIncidentMonitorWorkspaceBrowserSearch] =
    useState("");
  const avatarInputRef = useRef<HTMLInputElement | null>(null);

  const loadIdentityConfig = async () => {
    const identityApi = (client as any)?.identity;
    if (identityApi?.get) return identityApi.get();
    return api("/api/engine/config/identity", { method: "GET" });
  };

  const patchIdentityConfig = async (payload: any) => {
    const identityApi = (client as any)?.identity;
    if (identityApi?.patch) return identityApi.patch(payload);
    return api("/api/engine/config/identity", {
      method: "PATCH",
      body: JSON.stringify(payload),
    });
  };

  const identityConfig = useQuery({
    queryKey: ["settings", "identity", "config"],
    queryFn: () => loadIdentityConfig().catch(() => ({ identity: {} as any })),
  });

  useEffect(() => {
    const bot = (identityConfig.data as any)?.identity?.bot || {};
    const aliases = bot?.aliases || {};
    const canonical = String(
      bot?.canonicalName || bot?.canonical_name || identity?.botName || "Tandem"
    ).trim();
    const avatar = String(bot?.avatarUrl || bot?.avatar_url || identity?.botAvatarUrl || "").trim();
    const controlPanelAlias = String(aliases?.controlPanel || aliases?.control_panel || "").trim();
    setBotName(canonical || "Tandem");
    setBotAvatarUrl(avatar);
    setBotControlPanelAlias(controlPanelAlias || "Control Center");
  }, [identity?.botAvatarUrl, identity?.botName, identityConfig.data]);

  useEffect(() => {
    if (currentRoute === "mcp") setActiveSection("mcp");
    if (currentRoute === "channels") setActiveSection("channels");
    if (currentRoute === "incident-monitor") setActiveSection("incident_monitor");
  }, [currentRoute]);

  const providersCatalog = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [], connected: [] })),
  });

  const providersConfig = useQuery({
    queryKey: ["settings", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({ default: "", providers: {} })),
  });
  const configuredProviders = useMemo(
    () =>
      ((providersConfig.data?.providers as Record<string, any> | undefined) || {}) as Record<
        string,
        any
      >,
    [providersConfig.data?.providers]
  );
  const customConfiguredProviders = useMemo(
    () =>
      Object.entries(configuredProviders)
        .filter(([providerId]) => {
          const normalized = providerId.trim().toLowerCase();
          return normalized && !BUILTIN_PROVIDER_IDS.has(normalized);
        })
        .map(([providerId, value]) => ({
          id: providerId,
          url: String(value?.url || "").trim(),
          model: String(value?.default_model || value?.defaultModel || "").trim(),
          isDefault:
            String(providersConfig.data?.default || "")
              .trim()
              .toLowerCase() === providerId.trim().toLowerCase(),
        })),
    [configuredProviders, providersConfig.data?.default]
  );

  const browserStatus = useQuery<BrowserStatusResponse | null>({
    queryKey: ["settings", "browser", "status"],
    queryFn: () => api("/api/engine/browser/status", { method: "GET" }).catch(() => null),
    refetchInterval: 30_000,
  });
  const [browserSmokeResult, setBrowserSmokeResult] = useState<BrowserSmokeTestResponse | null>(
    null
  );
  const installBrowserMutation = useMutation({
    mutationFn: () => api("/api/engine/browser/install", { method: "POST" }),
    onSuccess: async () => {
      toast("ok", "Browser sidecar installed on the engine host.");
      await browserStatus.refetch();
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const smokeTestBrowserMutation = useMutation({
    mutationFn: () =>
      api("/api/engine/browser/smoke-test", {
        method: "POST",
        body: JSON.stringify({ url: "https://example.com" }),
      }),
    onSuccess: async (result: BrowserSmokeTestResponse) => {
      setBrowserSmokeResult(result);
      toast("ok", "Browser smoke test passed.");
      await browserStatus.refetch();
    },
    onError: (error) => {
      setBrowserSmokeResult(null);
      toast("err", error instanceof Error ? error.message : String(error));
    },
  });
  const mcpServersQuery = useQuery({
    queryKey: ["settings", "mcp", "servers"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: 10_000,
  });
  const mcpToolsQuery = useQuery({
    queryKey: ["settings", "mcp", "tools"],
    queryFn: () => client.mcp.listTools().catch(() => []),
    refetchInterval: 15_000,
  });
  const mcpCatalogQuery = useQuery({
    queryKey: ["settings", "mcp", "catalog"],
    queryFn: () => api("/api/engine/mcp/catalog", { method: "GET" }).catch(() => null),
    refetchInterval: 60_000,
  });
  const incidentMonitorConfigQuery = useQuery({
    queryKey: ["settings", "incident-monitor", "config"],
    queryFn: () =>
      api("/api/engine/config/incident-monitor", { method: "GET" }).catch(() => ({
        incident_monitor: {},
      })),
  });
  const incidentMonitorStatusQuery = useQuery({
    queryKey: ["settings", "incident-monitor", "status"],
    queryFn: () =>
      api("/api/engine/incident-monitor/status", { method: "GET" }).catch(() => ({
        status: {},
      })),
    refetchInterval: 10_000,
  });
  const incidentMonitorDraftsQuery = useQuery({
    queryKey: ["settings", "incident-monitor", "drafts"],
    queryFn: () =>
      api("/api/engine/incident-monitor/drafts?limit=10", { method: "GET" }).catch(() => ({
        drafts: [],
      })),
    refetchInterval: 15_000,
  });
  const incidentMonitorIncidentsQuery = useQuery({
    queryKey: ["settings", "incident-monitor", "incidents"],
    queryFn: () =>
      api("/api/engine/incident-monitor/incidents?limit=10", { method: "GET" }).catch(() => ({
        incidents: [],
      })),
    refetchInterval: 10_000,
  });
  const incidentMonitorPostsQuery = useQuery({
    queryKey: ["settings", "incident-monitor", "posts"],
    queryFn: () =>
      api("/api/engine/incident-monitor/posts?limit=10", { method: "GET" }).catch(() => ({
        posts: [],
      })),
    refetchInterval: 15_000,
  });
  const incidentMonitorWorkspaceBrowserQuery = useQuery({
    queryKey: [
      "settings",
      "incident-monitor",
      "workspace-browser",
      incidentMonitorWorkspaceBrowserDir,
    ],
    enabled: incidentMonitorWorkspaceBrowserOpen && !!incidentMonitorWorkspaceBrowserDir,
    queryFn: () =>
      api(
        `/api/orchestrator/workspaces/list?dir=${encodeURIComponent(
          incidentMonitorWorkspaceBrowserDir
        )}`,
        { method: "GET" }
      ),
  });
  const channelsConfigQuery = useQuery({
    queryKey: ["settings", "channels", "config"],
    queryFn: () => client.channels.config().catch(() => ({})),
    refetchInterval: 15_000,
  });
  const channelsStatusQuery = useQuery({
    queryKey: ["settings", "channels", "status"],
    queryFn: () => client.channels.status().catch(() => ({})),
    refetchInterval: 6_000,
  });
  const channelToolPreferencesQuery = useQuery({
    queryKey: ["settings", "channels", "tool-preferences"],
    queryFn: async () => {
      const entries = await Promise.all(
        (["telegram", "discord", "slack"] as const).map(async (channel) => [
          channel,
          normalizeChannelToolPreferences(
            await api(`/api/engine/channels/${channel}/tool-preferences`, {
              method: "GET",
            }).catch(() => defaultChannelToolPreferences())
          ),
        ])
      );
      return Object.fromEntries(entries) as Record<string, ChannelToolPreferencesRow>;
    },
    refetchInterval: 15_000,
  });

  const setDefaultsMutation = useMutation({
    mutationFn: async ({ providerId, modelId }: { providerId: string; modelId: string }) =>
      client.providers.setDefaults(providerId, modelId),
    onSuccess: async () => {
      toast("ok", "Updated provider defaults.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "providers"] }),
        refreshProviderStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const saveCustomProviderMutation = useMutation({
    mutationFn: async ({
      providerId,
      url,
      modelId,
      apiKey,
      makeDefault,
    }: {
      providerId: string;
      url: string;
      modelId: string;
      apiKey: string;
      makeDefault: boolean;
    }) => {
      const normalizedProviderId = providerId.trim().toLowerCase();
      const normalizedUrl = url.trim();
      const normalizedModelId = modelId.trim();
      if (!normalizedProviderId) throw new Error("Custom provider ID is required.");
      if (!normalizedUrl) throw new Error("Custom provider URL is required.");

      await api("/api/engine/config", {
        method: "PATCH",
        body: JSON.stringify({
          ...(makeDefault ? { default_provider: normalizedProviderId } : {}),
          providers: {
            [normalizedProviderId]: {
              url: normalizedUrl,
              ...(normalizedModelId ? { default_model: normalizedModelId } : {}),
            },
          },
        }),
      });

      if (apiKey.trim()) {
        await client.providers.setApiKey(normalizedProviderId, apiKey.trim());
      }
    },
    onSuccess: async () => {
      toast("ok", "Custom provider saved.");
      setCustomProviderApiKey("");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "providers"] }),
        refreshProviderStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const saveIncidentMonitorMutation = useMutation({
    mutationFn: async () =>
      api("/api/engine/config/incident-monitor", {
        method: "PATCH",
        body: JSON.stringify({
          incident_monitor: {
            enabled: incidentMonitorEnabled,
            paused: incidentMonitorPaused,
            workspace_root: String(incidentMonitorWorkspaceRoot || "").trim() || null,
            repo: String(incidentMonitorRepo || "").trim() || null,
            mcp_server: String(incidentMonitorMcpServer || "").trim() || null,
            provider_preference: String(incidentMonitorProviderPreference || "auto").trim(),
            model_policy:
              incidentMonitorProviderId && incidentMonitorModelId
                ? {
                    default_model: {
                      provider_id: incidentMonitorProviderId,
                      model_id: incidentMonitorModelId,
                    },
                  }
                : null,
            auto_create_new_issues: incidentMonitorAutoCreateIssues,
            require_approval_for_new_issues: incidentMonitorRequireApproval,
            auto_comment_on_matched_open_issues: incidentMonitorAutoComment,
            label_mode: "reporter_only",
          },
        }),
      }),
    onSuccess: async () => {
      toast("ok", "Incident Monitor settings saved.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] }),
      ]);
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const refreshIncidentMonitorBindingsMutation = useMutation({
    mutationFn: async () =>
      api("/api/engine/capabilities/bindings/refresh-builtins", {
        method: "POST",
      }),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] }),
        queryClient.invalidateQueries({ queryKey: ["settings", "mcp"] }),
      ]);
      toast("ok", "Capability bindings refreshed from built-ins.");
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const incidentMonitorDraftDecisionMutation = useMutation({
    mutationFn: async ({ draftId, decision }: { draftId: string; decision: "approve" | "deny" }) =>
      api(`/api/engine/incident-monitor/drafts/${encodeURIComponent(draftId)}/${decision}`, {
        method: "POST",
        body: JSON.stringify({
          reason: `${decision}d from control panel settings`,
        }),
      }),
    onSuccess: async (_payload, vars) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] }),
      ]);
      toast("ok", `Incident Monitor draft ${vars.decision === "approve" ? "approved" : "denied"}.`);
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const incidentMonitorTriageRunMutation = useMutation({
    mutationFn: async ({ draftId }: { draftId: string }) =>
      api(`/api/engine/incident-monitor/drafts/${encodeURIComponent(draftId)}/triage-run`, {
        method: "POST",
      }),
    onSuccess: async (payload: any) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] }),
      ]);
      toast(
        "ok",
        payload?.deduped
          ? `Incident Monitor triage run already exists: ${payload?.run?.run_id || "unknown"}`
          : `Incident Monitor triage run created: ${payload?.run?.run_id || "unknown"}`
      );
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const incidentMonitorPauseResumeMutation = useMutation({
    mutationFn: async ({ action }: { action: "pause" | "resume" }) =>
      api(`/api/engine/incident-monitor/${action}`, {
        method: "POST",
      }),
    onSuccess: async (_payload, vars) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] }),
      ]);
      toast("ok", `Incident Monitor ${vars.action === "pause" ? "paused" : "resumed"}.`);
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const incidentMonitorReplayIncidentMutation = useMutation({
    mutationFn: async ({ incidentId }: { incidentId: string }) =>
      api(`/api/engine/incident-monitor/incidents/${encodeURIComponent(incidentId)}/replay`, {
        method: "POST",
      }),
    onSuccess: async (payload: any) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] }),
      ]);
      toast(
        "ok",
        payload?.deduped
          ? `Incident Monitor triage run already exists: ${payload?.run?.run_id || "unknown"}`
          : `Incident Monitor replay queued triage: ${payload?.run?.run_id || "unknown"}`
      );
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const incidentMonitorPublishDraftMutation = useMutation({
    mutationFn: async ({ draftId }: { draftId: string }) =>
      api(`/api/engine/incident-monitor/drafts/${encodeURIComponent(draftId)}/publish`, {
        method: "POST",
      }),
    onSuccess: async (payload: any) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] }),
      ]);
      toast(
        "ok",
        payload?.action === "comment_issue"
          ? `Incident Monitor commented on issue #${payload?.draft?.issue_number || "unknown"}.`
          : `Incident Monitor published issue #${payload?.draft?.issue_number || "unknown"}.`
      );
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const incidentMonitorRecheckMatchMutation = useMutation({
    mutationFn: async ({ draftId }: { draftId: string }) =>
      api(`/api/engine/incident-monitor/drafts/${encodeURIComponent(draftId)}/recheck-match`, {
        method: "POST",
      }),
    onSuccess: async (payload: any) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] }),
      ]);
      toast(
        "ok",
        `GitHub match result: ${String(payload?.action || "rechecked").replaceAll("_", " ")}.`
      );
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });

  const setApiKeyMutation = useMutation({
    mutationFn: ({ providerId, apiKey }: { providerId: string; apiKey: string }) =>
      client.providers.setApiKey(providerId, apiKey),
    onSuccess: async () => {
      toast("ok", "API key updated.");
      await refreshProviderStatus();
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const saveIdentityMutation = useMutation({
    mutationFn: async () => {
      const currentBot = (identityConfig.data as any)?.identity?.bot || {};
      const currentAliases = currentBot?.aliases || {};
      const canonical = String(botName || "").trim();
      if (!canonical) throw new Error("Bot name is required.");
      const avatar = String(botAvatarUrl || "").trim();
      const controlPanelAlias = String(botControlPanelAlias || "").trim();
      return patchIdentityConfig({
        identity: {
          bot: {
            canonical_name: canonical,
            avatar_url: avatar || null,
            aliases: {
              ...currentAliases,
              control_panel: controlPanelAlias || undefined,
            },
          },
        },
      } as any);
    },
    onSuccess: async () => {
      toast("ok", "Identity updated.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "identity"] }),
        refreshIdentityStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const invalidateChannels = useCallback(
    async () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "channels"] }),
        queryClient.invalidateQueries({ queryKey: ["settings", "channels", "tool-preferences"] }),
      ]),
    [queryClient]
  );
  const saveChannelToolPreferencesMutation = useMutation({
    mutationFn: async ({
      channel,
      payload,
    }: {
      channel: "telegram" | "discord" | "slack";
      payload: ChannelToolPreferencesRow | { reset: true };
    }) => {
      if ("reset" in payload) {
        return api(`/api/engine/channels/${channel}/tool-preferences`, {
          method: "PUT",
          body: JSON.stringify({ reset: true }),
        });
      }
      return api(`/api/engine/channels/${channel}/tool-preferences`, {
        method: "PUT",
        body: JSON.stringify(payload),
      });
    },
    onSuccess: async (_, vars) => {
      await queryClient.invalidateQueries({
        queryKey: ["settings", "channels", "tool-preferences"],
      });
      toast("ok", `Saved ${vars.channel} channel tool scope.`);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const saveChannelMutation = useMutation({
    mutationFn: async (channel: "telegram" | "discord" | "slack") => {
      const draft = channelDrafts[channel];
      if (!draft) throw new Error(`No draft found for ${channel}.`);
      const payload: Record<string, unknown> = {
        allowed_users: parseAllowedUsers(draft.allowedUsers),
        mention_only: !!draft.mentionOnly,
        security_profile: String(draft.securityProfile || "operator").trim() || "operator",
      };
      const token = String(draft.botToken || "").trim();
      if (token) payload.bot_token = token;
      if (channel === "telegram") {
        payload.style_profile = String(draft.styleProfile || "default").trim() || "default";
      }
      if (channel === "discord") {
        payload.guild_id = String(draft.guildId || "").trim();
      }
      if (channel === "slack") {
        const channelId = String(draft.channelId || "").trim();
        if (!channelId && !(channelsConfigQuery.data as any)?.slack?.channel_id) {
          throw new Error("Slack channel ID is required.");
        }
        if (channelId) payload.channel_id = channelId;
      }
      return client.channels.put(channel, payload as any);
    },
    onSuccess: async (_, channel) => {
      toast("ok", `Saved ${channel} channel settings.`);
      setChannelVerifyResult((prev) => ({ ...prev, [channel]: null }));
      setChannelDrafts((prev) => ({
        ...prev,
        [channel]: {
          ...prev[channel],
          botToken: "",
        },
      }));
      await invalidateChannels();
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const deleteChannelMutation = useMutation({
    mutationFn: async (channel: "telegram" | "discord" | "slack") =>
      client.channels.delete(channel),
    onSuccess: async (_, channel) => {
      toast("ok", `Removed ${channel} channel settings.`);
      setChannelVerifyResult((prev) => ({ ...prev, [channel]: null }));
      setChannelDrafts((prev) => ({
        ...prev,
        [channel]: normalizeChannelDraft(channel, null),
      }));
      await invalidateChannels();
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const verifyChannelMutation = useMutation({
    mutationFn: async (channel: "discord") => {
      const draft = channelDrafts[channel];
      const payload = {
        bot_token: String(draft?.botToken || "").trim() || undefined,
      };
      return client.channels.verify(channel, payload as any);
    },
    onSuccess: (result, channel) => {
      setChannelVerifyResult((prev) => ({ ...prev, [channel]: result }));
      toast("ok", `${channel} verification complete.`);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const invalidateMcp = useCallback(
    async () => queryClient.invalidateQueries({ queryKey: ["settings", "mcp"] }),
    [queryClient]
  );
  const mcpActionMutation = useMutation({
    mutationFn: async ({ action, server }: { action: string; server?: McpServerRow }) => {
      if (!server) throw new Error("No MCP server selected.");
      if (action === "connect") return client.mcp.connect(server.name);
      if (action === "disconnect") return client.mcp.disconnect(server.name);
      if (action === "refresh") return client.mcp.refresh(server.name);
      if (action === "toggle-enabled")
        return (client.mcp as any).setEnabled(server.name, !server.enabled);
      if (action === "delete")
        return api(`/api/engine/mcp/${encodeURIComponent(server.name)}`, { method: "DELETE" });
      throw new Error(`Unknown action: ${action}`);
    },
    onSuccess: async (_, vars) => {
      await invalidateMcp();
      if (vars.action === "connect") toast("ok", `Connected ${vars.server?.name}.`);
      if (vars.action === "disconnect") toast("ok", `Disconnected ${vars.server?.name}.`);
      if (vars.action === "refresh") toast("ok", `Refreshed ${vars.server?.name}.`);
      if (vars.action === "toggle-enabled") {
        toast("ok", `${vars.server?.enabled ? "Disabled" : "Enabled"} ${vars.server?.name}.`);
      }
      if (vars.action === "delete") toast("ok", `Deleted ${vars.server?.name}.`);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const mcpSaveMutation = useMutation({
    mutationFn: async () => {
      const transportValue = String(mcpTransport || "").trim();
      const inferredName = inferMcpNameFromTransport(transportValue);
      const normalizedName = normalizeMcpName(mcpName || inferredName);
      if (!transportValue) throw new Error("Transport URL is required.");
      if (!parseUrl(transportValue) && !transportValue.startsWith("stdio:")) {
        throw new Error("Transport must be a valid URL or stdio:* transport.");
      }
      const headers = buildMcpHeaders({
        authMode: mcpAuthMode,
        token: mcpToken,
        customHeader: mcpCustomHeader,
        transport: transportValue,
      });
      const mergedHeaders = mergeMcpHeaders(headers, mcpExtraHeaders);
      const githubToolsets = String(mcpGithubToolsets || "").trim();
      if (isGithubCopilotMcpTransport(transportValue) && githubToolsets) {
        mergedHeaders["X-MCP-Toolsets"] = githubToolsets;
      }
      const payload: any = {
        name: normalizedName,
        transport: transportValue,
        enabled: true,
      };
      if (Object.keys(mergedHeaders).length) payload.headers = mergedHeaders;

      const editing = String(mcpEditingName || "").trim();
      if (editing && editing !== normalizedName) {
        await api(`/api/engine/mcp/${encodeURIComponent(editing)}`, { method: "DELETE" }).catch(
          () => null
        );
      }

      await (client.mcp as any).add(payload);
      if (mcpConnectAfterAdd) {
        const result = await client.mcp.connect(payload.name);
        if (!result?.ok) throw new Error(`Added "${payload.name}" but connect failed.`);
      }
      return payload.name;
    },
    onSuccess: async (serverName) => {
      await invalidateMcp();
      setMcpModalOpen(false);
      setMcpName("");
      setMcpTransport("");
      setMcpAuthMode("none");
      setMcpToken("");
      setMcpCustomHeader("");
      setMcpGithubToolsets("");
      setMcpExtraHeaders([]);
      setMcpConnectAfterAdd(true);
      setMcpEditingName("");
      toast("ok", `Saved MCP "${serverName}".`);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const handleAvatarUpload = (file: File | null) => {
    if (!file) return;
    if (file.size > 10 * 1024 * 1024) {
      toast("err", "Avatar image is too large (max 10 MB).");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const value = typeof reader.result === "string" ? reader.result : "";
      if (!value) {
        toast("err", "Failed to read avatar image.");
        return;
      }
      setBotAvatarUrl(value);
    };
    reader.onerror = () => toast("err", "Failed to read avatar image.");
    reader.readAsDataURL(file);
  };

  const providers = Array.isArray(providersCatalog.data?.all) ? providersCatalog.data.all : [];

  useEffect(() => {
    const preferred =
      customConfiguredProviders.find((provider) => provider.isDefault) ||
      customConfiguredProviders[0];
    if (!preferred) return;
    setCustomProviderId((current) =>
      current.trim() && current.trim().toLowerCase() !== "custom" ? current : preferred.id
    );
    setCustomProviderUrl((current) => (current.trim() ? current : preferred.url));
    setCustomProviderModel((current) => (current.trim() ? current : preferred.model));
    setCustomProviderMakeDefault(
      String(providersConfig.data?.default || "")
        .trim()
        .toLowerCase() === preferred.id.trim().toLowerCase()
    );
  }, [customConfiguredProviders, providersConfig.data?.default]);
  const mcpServers = useMemo(
    () => normalizeMcpServers(mcpServersQuery.data),
    [mcpServersQuery.data]
  );
  const mcpToolIds = useMemo(() => normalizeMcpTools(mcpToolsQuery.data), [mcpToolsQuery.data]);
  const mcpCatalog = useMemo(
    () =>
      normalizeMcpCatalog((mcpCatalogQuery.data as any)?.catalog || mcpCatalogQuery.data || null),
    [mcpCatalogQuery.data]
  );
  const configuredMcpServerNames = useMemo(
    () => new Set(mcpServers.map((server) => server.name.toLowerCase())),
    [mcpServers]
  );
  const filteredMcpCatalog = useMemo(() => {
    const query = String(mcpCatalogSearch || "")
      .trim()
      .toLowerCase();
    return mcpCatalog.servers
      .filter((row) => {
        if (!query) return true;
        return (
          row.name.toLowerCase().includes(query) ||
          row.slug.toLowerCase().includes(query) ||
          row.transportUrl.toLowerCase().includes(query)
        );
      })
      .slice(0, 36);
  }, [mcpCatalog.servers, mcpCatalogSearch]);
  const connectedMcpCount = mcpServers.filter((server) => server.connected).length;
  const incidentMonitorStatus = useMemo(
    () => ((incidentMonitorStatusQuery.data as any)?.status || {}) as IncidentMonitorStatusRow,
    [incidentMonitorStatusQuery.data]
  );
  const incidentMonitorDrafts = useMemo(
    () =>
      Array.isArray((incidentMonitorDraftsQuery.data as any)?.drafts)
        ? ((incidentMonitorDraftsQuery.data as any).drafts as IncidentMonitorDraftRow[]) || []
        : [],
    [incidentMonitorDraftsQuery.data]
  );
  const incidentMonitorIncidents = useMemo(
    () =>
      Array.isArray((incidentMonitorIncidentsQuery.data as any)?.incidents)
        ? ((incidentMonitorIncidentsQuery.data as any).incidents as IncidentMonitorIncidentRow[]) ||
          []
        : [],
    [incidentMonitorIncidentsQuery.data]
  );
  const incidentMonitorPosts = useMemo(
    () =>
      Array.isArray((incidentMonitorPostsQuery.data as any)?.posts)
        ? ((incidentMonitorPostsQuery.data as any).posts as IncidentMonitorPostRow[]) || []
        : [],
    [incidentMonitorPostsQuery.data]
  );
  const selectedIncidentMonitorServer = useMemo(
    () =>
      mcpServers.find(
        (server) =>
          server.name.toLowerCase() ===
          String(incidentMonitorMcpServer || "")
            .trim()
            .toLowerCase()
      ) || null,
    [incidentMonitorMcpServer, mcpServers]
  );
  const selectedIncidentMonitorProvider = useMemo(
    () =>
      providers.find(
        (provider: any) =>
          String(provider?.id || "").toLowerCase() ===
          String(incidentMonitorProviderId || "")
            .trim()
            .toLowerCase()
      ) || null,
    [incidentMonitorProviderId, providers]
  );
  const incidentMonitorProviderModels = useMemo(() => {
    const modelMap =
      selectedIncidentMonitorProvider && typeof selectedIncidentMonitorProvider === "object"
        ? selectedIncidentMonitorProvider.models || {}
        : {};
    return Object.keys(modelMap).sort((a, b) => a.localeCompare(b));
  }, [selectedIncidentMonitorProvider]);
  const browserIssues = Array.isArray(browserStatus.data?.blocking_issues)
    ? browserStatus.data?.blocking_issues || []
    : [];
  const browserRecommendations = Array.isArray(browserStatus.data?.recommendations)
    ? browserStatus.data?.recommendations || []
    : [];
  const browserInstallHints = Array.isArray(browserStatus.data?.install_hints)
    ? browserStatus.data?.install_hints || []
    : [];
  const channelNames = ["telegram", "discord", "slack"] as const;
  const connectedChannelCount = channelNames.filter(
    (name) => !!(channelsStatusQuery.data as any)?.[name]?.connected
  ).length;
  const incidentMonitorWorkspaceDirectories = Array.isArray(
    incidentMonitorWorkspaceBrowserQuery.data?.directories
  )
    ? incidentMonitorWorkspaceBrowserQuery.data.directories
    : [];
  const incidentMonitorWorkspaceSearchQuery = String(incidentMonitorWorkspaceBrowserSearch || "")
    .trim()
    .toLowerCase();
  const filteredIncidentMonitorWorkspaceDirectories = useMemo(() => {
    if (!incidentMonitorWorkspaceSearchQuery) return incidentMonitorWorkspaceDirectories;
    return incidentMonitorWorkspaceDirectories.filter((entry: any) => {
      const name = String(entry?.name || entry?.path || "")
        .trim()
        .toLowerCase();
      return name.includes(incidentMonitorWorkspaceSearchQuery);
    });
  }, [incidentMonitorWorkspaceDirectories, incidentMonitorWorkspaceSearchQuery]);
  const incidentMonitorWorkspaceParentDir = String(
    incidentMonitorWorkspaceBrowserQuery.data?.parent || ""
  ).trim();
  const incidentMonitorCurrentBrowseDir = String(
    incidentMonitorWorkspaceBrowserQuery.data?.dir || incidentMonitorWorkspaceBrowserDir || ""
  ).trim();

  useEffect(() => {
    const configPayload = incidentMonitorConfigQuery.data as any;
    const config =
      configPayload?.incident_monitor && typeof configPayload.incident_monitor === "object"
        ? (configPayload.incident_monitor as IncidentMonitorConfigRow)
        : configPayload?.bug_monitor && typeof configPayload.bug_monitor === "object"
          ? (configPayload.bug_monitor as IncidentMonitorConfigRow)
          : {};
    setIncidentMonitorEnabled(!!config.enabled);
    setIncidentMonitorPaused(!!config.paused);
    setIncidentMonitorWorkspaceRoot(String(config.workspace_root || "").trim());
    setIncidentMonitorRepo(String(config.repo || "").trim());
    setIncidentMonitorMcpServer(String(config.mcp_server || "").trim());
    setIncidentMonitorProviderPreference(
      String(config.provider_preference || "auto").trim() || "auto"
    );
    setIncidentMonitorProviderId(
      String(config.model_policy?.default_model?.provider_id || "").trim()
    );
    setIncidentMonitorModelId(String(config.model_policy?.default_model?.model_id || "").trim());
    setIncidentMonitorAutoCreateIssues(config.auto_create_new_issues !== false);
    setIncidentMonitorRequireApproval(!!config.require_approval_for_new_issues);
    setIncidentMonitorAutoComment(config.auto_comment_on_matched_open_issues !== false);
  }, [incidentMonitorConfigQuery.data]);

  useEffect(() => {
    const config =
      channelsConfigQuery.data && typeof channelsConfigQuery.data === "object"
        ? (channelsConfigQuery.data as Record<string, ChannelConfigRow>)
        : {};
    setChannelDrafts((prev) => {
      const next = { ...prev };
      for (const channel of channelNames) {
        if (!next[channel]) next[channel] = normalizeChannelDraft(channel, config[channel]);
      }
      return next;
    });
  }, [channelsConfigQuery.data]);

  const applyDefaultModel = (providerId: string, modelId: string) => {
    const next = String(modelId || "").trim();
    if (!next) return;
    setDefaultsMutation.mutate({ providerId, modelId: next });
  };

  const openMcpModal = (server?: McpServerRow) => {
    if (server) {
      setMcpModalTab("manual");
      const headers = server.headers && typeof server.headers === "object" ? server.headers : {};
      const keys = Object.keys(headers);
      const authKey = keys.find((key) => String(key).toLowerCase() === "authorization");
      const apiKey = keys.find((key) => String(key).toLowerCase() === "x-api-key");
      const toolsetsKey = keys.find((key) => String(key).toLowerCase() === "x-mcp-toolsets");
      const customKeys = keys.filter(
        (key) =>
          ![authKey, apiKey, toolsetsKey]
            .filter(Boolean)
            .map((value) => String(value).toLowerCase())
            .includes(String(key).toLowerCase())
      );
      let nextAuthMode = "none";
      let nextCustomHeader = "";
      let nextToken = "";
      if (apiKey) {
        nextAuthMode = "x-api-key";
        nextToken = String(headers[apiKey] || "").trim();
      } else if (authKey) {
        nextAuthMode = "bearer";
        nextToken = String(headers[authKey] || "")
          .replace(/^bearer\s+/i, "")
          .trim();
      } else if (customKeys.length === 1) {
        nextAuthMode = "custom";
        nextCustomHeader = customKeys[0];
        nextToken = String(headers[customKeys[0]] || "").trim();
      }
      setMcpEditingName(server.name);
      setMcpName(server.name);
      setMcpTransport(server.transport || "");
      setMcpConnectAfterAdd(server.connected || false);
      setMcpGithubToolsets(
        toolsetsKey
          ? String(headers[toolsetsKey] || "").trim()
          : isGithubCopilotMcpTransport(server.transport || "")
            ? "default"
            : ""
      );
      setMcpAuthMode(nextAuthMode);
      setMcpCustomHeader(nextCustomHeader);
      setMcpToken(nextToken);
      const reservedHeaderKeys = new Set(
        [authKey, apiKey, toolsetsKey, nextAuthMode === "custom" ? nextCustomHeader : ""]
          .filter(Boolean)
          .map((value) => String(value).toLowerCase())
      );
      setMcpExtraHeaders(
        keys
          .filter((key) => !reservedHeaderKeys.has(String(key).toLowerCase()))
          .map((key) => ({
            key,
            value: String(headers[key] || "").trim(),
          }))
      );
    } else {
      setMcpModalTab("catalog");
      setMcpEditingName("");
      setMcpName("");
      setMcpTransport("");
      setMcpAuthMode("none");
      setMcpCustomHeader("");
      setMcpToken("");
      setMcpGithubToolsets("");
      setMcpExtraHeaders([]);
      setMcpConnectAfterAdd(true);
    }
    setMcpModalOpen(true);
  };

  const copyIncidentMonitorDebugPayload = async () => {
    const payload = await api("/api/engine/incident-monitor/debug", { method: "GET" });
    await navigator.clipboard.writeText(JSON.stringify(payload, null, 2));
    toast("ok", "Incident Monitor debug payload copied.");
  };

  const sectionTabs: Array<{ id: SettingsSection; label: string; icon: string }> = [
    { id: "providers", label: "Providers", icon: "cpu" },
    { id: "identity", label: "Identity", icon: "badge-check" },
    { id: "theme", label: "Themes", icon: "paint-bucket" },
    { id: "channels", label: "Channels", icon: "message-circle" },
    { id: "mcp", label: "MCP", icon: "plug-zap" },
    { id: "incident_monitor", label: "Incident Monitor", icon: "shield-alert" },
    { id: "browser", label: "Browser", icon: "monitor-cog" },
  ];
  const mcpAuthPreviewText = useMemo(
    () => mcpAuthPreview(mcpAuthMode, mcpToken, mcpCustomHeader, mcpTransport),
    [mcpAuthMode, mcpCustomHeader, mcpToken, mcpTransport]
  );
  const mcpIsGithubTransport = useMemo(
    () => isGithubCopilotMcpTransport(mcpTransport),
    [mcpTransport]
  );

  useEffect(() => {
    const root = rootRef.current;
    if (root) renderIcons(root);
    else renderIcons();
  }, [
    activeSection,
    incidentMonitorEnabled,
    incidentMonitorPaused,
    incidentMonitorWorkspaceRoot,
    incidentMonitorMcpServer,
    incidentMonitorStatus.readiness?.runtime_ready,
    incidentMonitorStatus.runtime?.monitoring_active,
    incidentMonitorStatus.runtime?.paused,
    incidentMonitorStatus.runtime?.pending_incidents,
    incidentMonitorStatus.pending_drafts,
    incidentMonitorDrafts.length,
    incidentMonitorIncidents.length,
    refreshIncidentMonitorBindingsMutation.isPending,
    incidentMonitorPauseResumeMutation.isPending,
    incidentMonitorDraftDecisionMutation.isPending,
    incidentMonitorReplayIncidentMutation.isPending,
    incidentMonitorTriageRunMutation.isPending,
    saveIncidentMonitorMutation.isPending,
    mcpActionMutation.isPending,
  ]);

  return (
    <AnimatedPage className="grid gap-4">
      <div ref={rootRef} className="grid gap-4">
        <div className="tcp-settings-tabs">
          {sectionTabs.map((section) => (
            <button
              key={section.id}
              type="button"
              className={`tcp-settings-tab tcp-settings-tab-underline ${
                activeSection === section.id ? "active" : ""
              }`}
              onClick={() => setActiveSection(section.id)}
            >
              <i data-lucide={section.icon}></i>
              {section.label}
            </button>
          ))}
        </div>

        <SplitView
          main={
            <StaggerGroup className="grid gap-4">
              {activeSection === "providers" ? (
                <PanelCard
                  title="Provider defaults"
                  subtitle="Provider catalog, model selection, and API key entry."
                  actions={
                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Badge
                        tone={String(providersConfig.data?.default || "").trim() ? "ok" : "warn"}
                      >
                        Default: {String(providersConfig.data?.default || "none")}
                      </Badge>
                      <Badge tone="info">
                        {String(providersCatalog.data?.connected?.length || 0)} connected
                      </Badge>
                      <button
                        className="tcp-btn"
                        onClick={() =>
                          refreshProviderStatus().then(() =>
                            toast("ok", "Provider status refreshed.")
                          )
                        }
                      >
                        <i data-lucide="refresh-cw"></i>
                        Refresh provider
                      </button>
                    </div>
                  }
                >
                  <div className="grid gap-3">
                    <div className="tcp-list-item grid gap-3">
                      <div className="flex flex-wrap items-start justify-between gap-3">
                        <div>
                          <div className="font-medium">Custom OpenAI-compatible provider</div>
                          <div className="tcp-subtle mt-1 text-xs">
                            Add providers like MiniMax by ID, base URL, default model, and API key.
                          </div>
                        </div>
                        <Badge tone={customConfiguredProviders.length ? "ok" : "info"}>
                          {customConfiguredProviders.length} configured
                        </Badge>
                      </div>
                      <form
                        className="grid gap-3"
                        onSubmit={(event) => {
                          event.preventDefault();
                          saveCustomProviderMutation.mutate({
                            providerId: customProviderId,
                            url: customProviderUrl,
                            modelId: customProviderModel,
                            apiKey: customProviderApiKey,
                            makeDefault: customProviderMakeDefault,
                          });
                        }}
                      >
                        <div className="grid gap-3 md:grid-cols-2">
                          <div className="grid gap-2">
                            <label className="text-sm font-medium">Provider ID</label>
                            <input
                              className="tcp-input"
                              value={customProviderId}
                              onInput={(event) =>
                                setCustomProviderId((event.target as HTMLInputElement).value)
                              }
                              placeholder="custom"
                            />
                          </div>
                          <div className="grid gap-2">
                            <label className="text-sm font-medium">Default model</label>
                            <input
                              className="tcp-input"
                              value={customProviderModel}
                              onInput={(event) =>
                                setCustomProviderModel((event.target as HTMLInputElement).value)
                              }
                              placeholder="MiniMax-M2"
                            />
                          </div>
                        </div>
                        <div className="grid gap-2">
                          <label className="text-sm font-medium">Base URL</label>
                          <input
                            className="tcp-input"
                            value={customProviderUrl}
                            onInput={(event) =>
                              setCustomProviderUrl((event.target as HTMLInputElement).value)
                            }
                            placeholder="https://api.minimax.io/v1"
                          />
                        </div>
                        <div className="grid gap-2">
                          <label className="text-sm font-medium">API key</label>
                          <input
                            className="tcp-input"
                            type="password"
                            value={customProviderApiKey}
                            onInput={(event) =>
                              setCustomProviderApiKey((event.target as HTMLInputElement).value)
                            }
                            placeholder="Optional. Leave blank to keep the existing key."
                          />
                        </div>
                        <label className="inline-flex items-center gap-2 text-sm text-slate-200">
                          <input
                            type="checkbox"
                            className="accent-slate-400"
                            checked={customProviderMakeDefault}
                            onChange={(event) =>
                              setCustomProviderMakeDefault(
                                (event.target as HTMLInputElement).checked
                              )
                            }
                          />
                          Make this the default provider
                        </label>
                        <div className="flex flex-wrap justify-end gap-2">
                          <button
                            className="tcp-btn-primary"
                            type="submit"
                            disabled={saveCustomProviderMutation.isPending}
                          >
                            <i data-lucide="plus"></i>
                            Save custom provider
                          </button>
                        </div>
                      </form>
                      {customConfiguredProviders.length ? (
                        <div className="grid gap-2">
                          {customConfiguredProviders.map((provider) => (
                            <div
                              key={provider.id}
                              className="flex flex-wrap items-start justify-between gap-2 rounded-xl border border-slate-700/60 bg-slate-900/20 px-3 py-2"
                            >
                              <div className="min-w-0">
                                <div className="font-medium">{provider.id}</div>
                                <div className="tcp-subtle break-all text-xs">
                                  {provider.url || "No URL configured"}
                                </div>
                                <div className="tcp-subtle text-xs">
                                  Model: {provider.model || "not set"}
                                </div>
                              </div>
                              <div className="flex flex-wrap gap-2">
                                {provider.isDefault ? <Badge tone="ok">default</Badge> : null}
                                <button
                                  type="button"
                                  className="tcp-btn h-8 px-3 text-xs"
                                  onClick={() => {
                                    setCustomProviderId(provider.id);
                                    setCustomProviderUrl(provider.url);
                                    setCustomProviderModel(provider.model);
                                    setCustomProviderMakeDefault(provider.isDefault);
                                    setProviderDefaultsOpen(true);
                                  }}
                                >
                                  <i data-lucide="square-pen"></i>
                                  Edit
                                </button>
                              </div>
                            </div>
                          ))}
                        </div>
                      ) : null}
                    </div>
                    <button
                      type="button"
                      className="tcp-list-item text-left"
                      onClick={() => setProviderDefaultsOpen((prev) => !prev)}
                      aria-expanded={providerDefaultsOpen}
                    >
                      <div className="flex items-center justify-between gap-3">
                        <div>
                          <div className="font-medium inline-flex items-center gap-2">
                            <i
                              data-lucide={providerDefaultsOpen ? "chevron-down" : "chevron-right"}
                            ></i>
                            <span>
                              {providerDefaultsOpen
                                ? "Hide provider catalog"
                                : "Show provider catalog"}
                            </span>
                          </div>
                          <div className="tcp-subtle mt-1 text-xs">
                            {providers.length} providers available for configuration. Expand to
                            change models and API keys.
                          </div>
                        </div>
                        <Badge tone="info">
                          {String(providersCatalog.data?.connected?.length || 0)} connected
                        </Badge>
                      </div>
                    </button>

                    <AnimatePresence initial={false}>
                      {providerDefaultsOpen ? (
                        <motion.div
                          className="grid gap-3"
                          initial={{ opacity: 0, y: -8 }}
                          animate={{ opacity: 1, y: 0 }}
                          exit={{ opacity: 0, y: -8 }}
                        >
                          {providers.length ? (
                            providers.map((provider: any) => {
                              const providerId = String(provider?.id || "");
                              const models = Object.keys(provider?.models || {});
                              const defaultModel = String(
                                providersConfig.data?.providers?.[providerId]?.default_model ||
                                  models[0] ||
                                  ""
                              );
                              const typedModel = String(
                                modelSearchByProvider[providerId] ?? defaultModel
                              ).trim();
                              const normalizedTyped = typedModel.toLowerCase();
                              const filteredModels = models
                                .filter((modelId) =>
                                  normalizedTyped
                                    ? modelId.toLowerCase().includes(normalizedTyped)
                                    : true
                                )
                                .slice(0, 80);
                              const badge = providerCatalogBadge(provider, models.length);
                              const subtitle = providerCatalogSubtitle(provider, defaultModel);
                              const providerHint =
                                (providerHints as Record<string, any>)[providerId] || null;
                              const keyUrl = String(providerHint?.keyUrl || "").trim();

                              return (
                                <motion.details key={providerId} layout className="tcp-list-item">
                                  <summary className="cursor-pointer list-none">
                                    <div className="flex items-center justify-between gap-3">
                                      <div>
                                        <div className="font-medium">{providerId}</div>
                                        <div className="tcp-subtle text-xs">{subtitle}</div>
                                      </div>
                                      <Badge tone={badge.tone}>{badge.text}</Badge>
                                    </div>
                                  </summary>
                                  <div className="mt-3 grid gap-3">
                                    {keyUrl ? (
                                      <div className="flex justify-end">
                                        <a
                                          className="tcp-btn h-8 px-3 text-xs"
                                          href={keyUrl}
                                          target="_blank"
                                          rel="noreferrer"
                                        >
                                          <i data-lucide="external-link"></i>
                                          Get API key
                                        </a>
                                      </div>
                                    ) : null}
                                    <form
                                      className="grid gap-2"
                                      onSubmit={(e) => {
                                        e.preventDefault();
                                        applyDefaultModel(providerId, typedModel);
                                      }}
                                    >
                                      <div className="flex gap-2">
                                        <input
                                          className="tcp-input"
                                          value={typedModel}
                                          placeholder={`Type model id for ${providerId}`}
                                          onInput={(e) =>
                                            setModelSearchByProvider((prev) => ({
                                              ...prev,
                                              [providerId]: (e.target as HTMLInputElement).value,
                                            }))
                                          }
                                        />
                                        <button className="tcp-btn" type="submit">
                                          <i data-lucide="badge-check"></i>
                                          Apply
                                        </button>
                                      </div>
                                      <div className="max-h-48 overflow-auto rounded-xl border border-slate-700/60 bg-slate-900/20 p-1">
                                        {filteredModels.length ? (
                                          filteredModels.map((modelId) => (
                                            <button
                                              key={modelId}
                                              type="button"
                                              className={`block w-full rounded-lg px-2 py-1.5 text-left text-sm hover:bg-slate-700/30 ${
                                                modelId === defaultModel ? "bg-slate-700/40" : ""
                                              }`}
                                              onClick={() => {
                                                setModelSearchByProvider((prev) => ({
                                                  ...prev,
                                                  [providerId]: modelId,
                                                }));
                                                applyDefaultModel(providerId, modelId);
                                              }}
                                            >
                                              {modelId}
                                            </button>
                                          ))
                                        ) : (
                                          <div className="tcp-subtle px-2 py-1 text-xs">
                                            {models.length
                                              ? "No matching models."
                                              : "No live catalog available. Type a model ID manually."}
                                          </div>
                                        )}
                                      </div>
                                    </form>

                                    <form
                                      onSubmit={(e) => {
                                        e.preventDefault();
                                        const input = e.currentTarget.elements.namedItem(
                                          "apiKey"
                                        ) as HTMLInputElement;
                                        const value = String(input?.value || "").trim();
                                        if (!value) return;
                                        setApiKeyMutation.mutate({ providerId, apiKey: value });
                                        input.value = "";
                                      }}
                                      className="flex gap-2"
                                    >
                                      <input
                                        name="apiKey"
                                        className="tcp-input"
                                        placeholder={String(
                                          providerHint?.placeholder || `Set ${providerId} API key`
                                        )}
                                      />
                                      <button className="tcp-btn" type="submit">
                                        <i data-lucide="save"></i>
                                        Save
                                      </button>
                                    </form>
                                  </div>
                                </motion.details>
                              );
                            })
                          ) : (
                            <EmptyState text="No providers were detected from the engine catalog." />
                          )}
                        </motion.div>
                      ) : null}
                    </AnimatePresence>
                  </div>
                </PanelCard>
              ) : null}

              {activeSection === "identity" ? (
                <PanelCard
                  title="Identity preview"
                  subtitle="Live preview of how the assistant appears across the panel."
                  actions={
                    <Toolbar>
                      <button
                        className="tcp-btn"
                        onClick={() =>
                          refreshIdentityStatus().then(() => toast("ok", "Identity refreshed."))
                        }
                      >
                        <i data-lucide="refresh-cw"></i>
                        Refresh identity
                      </button>
                      <button
                        className="tcp-btn-primary"
                        onClick={() => saveIdentityMutation.mutate()}
                        disabled={saveIdentityMutation.isPending}
                      >
                        <i data-lucide="save"></i>
                        Save
                      </button>
                    </Toolbar>
                  }
                >
                  <div className="grid gap-3">
                    <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                      <div className="flex items-center justify-between gap-3">
                        <div className="inline-flex items-center gap-3">
                          <span className="tcp-brand-avatar inline-grid h-12 w-12 rounded-xl">
                            {botAvatarUrl ? (
                              <img
                                src={botAvatarUrl}
                                alt={botName || "Bot"}
                                className="block h-full w-full object-cover"
                              />
                            ) : (
                              <i data-lucide="cpu"></i>
                            )}
                          </span>
                          <div>
                            <div className="font-semibold">{botName || "Tandem"}</div>
                            <div className="tcp-subtle text-xs">
                              {botControlPanelAlias || "Control Center"}
                            </div>
                          </div>
                        </div>
                        <Toolbar>
                          <button
                            className="tcp-icon-btn"
                            title="Upload avatar"
                            aria-label="Upload avatar"
                            onClick={() => avatarInputRef.current?.click()}
                          >
                            <i data-lucide="pencil"></i>
                          </button>
                          <button
                            className="tcp-icon-btn"
                            title="Clear avatar"
                            aria-label="Clear avatar"
                            onClick={() => setBotAvatarUrl("")}
                          >
                            <i data-lucide="trash-2"></i>
                          </button>
                        </Toolbar>
                      </div>
                    </div>

                    <input
                      className="tcp-input"
                      value={botName}
                      onInput={(e) => setBotName((e.target as HTMLInputElement).value)}
                      placeholder="Bot name"
                    />
                    <input
                      className="tcp-input"
                      value={botControlPanelAlias}
                      onInput={(e) => setBotControlPanelAlias((e.target as HTMLInputElement).value)}
                      placeholder="Control panel alias"
                    />
                    <input
                      className="tcp-input"
                      value={botAvatarUrl}
                      onInput={(e) => setBotAvatarUrl((e.target as HTMLInputElement).value)}
                      placeholder="Avatar URL or data URL"
                    />
                    <input
                      ref={avatarInputRef}
                      type="file"
                      accept="image/*"
                      className="hidden"
                      onChange={(e) =>
                        handleAvatarUpload((e.target as HTMLInputElement).files?.[0] || null)
                      }
                    />
                  </div>
                </PanelCard>
              ) : null}

              {activeSection === "theme" ? (
                <PanelCard
                  title="Theme studio"
                  subtitle="Preview tiles with richer feedback and immediate switching."
                >
                  <ThemePicker themes={themes} themeId={themeId} onChange={setTheme} />
                </PanelCard>
              ) : null}

              {activeSection === "channels" ? (
                <PanelCard
                  title="Channel connections"
                  subtitle="Telegram, Discord, and Slack delivery setup and live listener status."
                  actions={
                    <Toolbar>
                      <Badge tone={connectedChannelCount ? "ok" : "warn"}>
                        {connectedChannelCount}/{channelNames.length} connected
                      </Badge>
                      <button className="tcp-btn" onClick={() => void invalidateChannels()}>
                        <i data-lucide="refresh-cw"></i>
                        Refresh channels
                      </button>
                    </Toolbar>
                  }
                >
                  <div className="grid gap-3">
                    {channelNames.map((channel) => {
                      const config = ((channelsConfigQuery.data as any)?.[channel] ||
                        {}) as ChannelConfigRow;
                      const status = ((channelsStatusQuery.data as any)?.[channel] ||
                        {}) as ChannelStatusRow;
                      const draft =
                        channelDrafts[channel] || normalizeChannelDraft(channel, config);
                      const verifyResult = channelVerifyResult[channel];
                      const toolPrefs = normalizeChannelToolPreferences(
                        (
                          channelToolPreferencesQuery.data as
                            | Record<string, ChannelToolPreferencesRow>
                            | undefined
                        )?.[channel] || defaultChannelToolPreferences()
                      );
                      const publicDemo = draft.securityProfile === "public_demo";
                      const hasSavedConfig =
                        !!config?.has_token ||
                        !!(Array.isArray(config?.allowed_users) && config.allowed_users.length) ||
                        !!String(config?.guild_id || "").trim() ||
                        !!String(config?.channel_id || "").trim();

                      return (
                        <div key={channel} className="tcp-list-item grid gap-3">
                          <div className="flex flex-wrap items-center justify-between gap-3">
                            <div>
                              <div className="font-semibold capitalize">{channel}</div>
                              <div className="tcp-subtle text-xs">
                                {channel === "telegram"
                                  ? "Bot token, allowed users, and style profile."
                                  : channel === "discord"
                                    ? "Bot token, allowed users, mention policy, and guild targeting."
                                    : "Bot token, allowed users, mention policy, and default channel."}
                              </div>
                            </div>
                            <div className="flex flex-wrap gap-2">
                              <Badge tone={status.connected ? "ok" : "warn"}>
                                {status.connected
                                  ? "Connected"
                                  : status.enabled
                                    ? "Configured"
                                    : "Disconnected"}
                              </Badge>
                              <Badge tone={config.has_token ? "info" : "warn"}>
                                {config.has_token ? "Token saved" : "No token"}
                              </Badge>
                            </div>
                          </div>

                          <div className="grid gap-3 md:grid-cols-2">
                            <input
                              className="tcp-input"
                              type="password"
                              placeholder={
                                config.has_token
                                  ? String(config.token_masked || "********")
                                  : `Paste ${channel} bot token`
                              }
                              value={draft.botToken}
                              onInput={(e) =>
                                setChannelDrafts((prev) => ({
                                  ...prev,
                                  [channel]: {
                                    ...draft,
                                    botToken: (e.target as HTMLInputElement).value,
                                  },
                                }))
                              }
                            />
                            {config.has_token && !draft.botToken ? (
                              <div className="tcp-subtle text-xs">
                                Token is already stored. Enter a new token only if you want to
                                replace it.
                              </div>
                            ) : null}
                            <input
                              className="tcp-input"
                              placeholder="Allowed users (comma separated)"
                              value={draft.allowedUsers}
                              onInput={(e) =>
                                setChannelDrafts((prev) => ({
                                  ...prev,
                                  [channel]: {
                                    ...draft,
                                    allowedUsers: (e.target as HTMLInputElement).value,
                                  },
                                }))
                              }
                            />
                          </div>

                          <div className="grid gap-3 md:grid-cols-2">
                            <select
                              className="tcp-input"
                              value={draft.securityProfile}
                              onInput={(e) =>
                                setChannelDrafts((prev) => ({
                                  ...prev,
                                  [channel]: {
                                    ...draft,
                                    securityProfile: (e.target as HTMLSelectElement).value,
                                  },
                                }))
                              }
                            >
                              <option value="operator">Operator</option>
                              <option value="trusted_team">Trusted team</option>
                              <option value="public_demo">Public demo</option>
                            </select>
                            {channel === "telegram" ? (
                              <input
                                className="tcp-input"
                                placeholder="Style profile"
                                value={draft.styleProfile}
                                onInput={(e) =>
                                  setChannelDrafts((prev) => ({
                                    ...prev,
                                    [channel]: {
                                      ...draft,
                                      styleProfile: (e.target as HTMLInputElement).value,
                                    },
                                  }))
                                }
                              />
                            ) : null}
                            {channel === "discord" ? (
                              <input
                                className="tcp-input"
                                placeholder="Guild ID (optional)"
                                value={draft.guildId}
                                onInput={(e) =>
                                  setChannelDrafts((prev) => ({
                                    ...prev,
                                    [channel]: {
                                      ...draft,
                                      guildId: (e.target as HTMLInputElement).value,
                                    },
                                  }))
                                }
                              />
                            ) : null}
                            {channel === "slack" ? (
                              <input
                                className="tcp-input"
                                placeholder="Default channel ID"
                                value={draft.channelId}
                                onInput={(e) =>
                                  setChannelDrafts((prev) => ({
                                    ...prev,
                                    [channel]: {
                                      ...draft,
                                      channelId: (e.target as HTMLInputElement).value,
                                    },
                                  }))
                                }
                              />
                            ) : null}
                            <label className="inline-flex items-center gap-2 rounded-xl border border-slate-700/60 bg-slate-900/20 px-3 py-2 text-sm">
                              <input
                                type="checkbox"
                                checked={draft.mentionOnly}
                                onChange={(e) =>
                                  setChannelDrafts((prev) => ({
                                    ...prev,
                                    [channel]: {
                                      ...draft,
                                      mentionOnly: e.target.checked,
                                    },
                                  }))
                                }
                              />
                              Mention only
                            </label>
                          </div>

                          {draft.securityProfile === "public_demo" ? (
                            <div className="tcp-subtle text-xs">
                              Public demo mode blocks operator commands, file/workspace access, MCP
                              access, shell access, and tool-scope widening. Memory stays confined
                              to this channel&apos;s quarantined public namespace, and `/help` still
                              advertises restricted capabilities for security.
                            </div>
                          ) : null}

                          <div className="tcp-subtle text-xs">
                            Active sessions: {Number(status.active_sessions || 0)}
                            {status.last_error ? ` · Last error: ${status.last_error}` : ""}
                          </div>

                          {verifyResult?.hints?.length ? (
                            <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3 text-xs">
                              <div className="mb-1 font-medium">Verification hints</div>
                              <div className="grid gap-1">
                                {verifyResult.hints.map((hint: string, index: number) => (
                                  <div key={`${channel}-hint-${index}`} className="tcp-subtle">
                                    {hint}
                                  </div>
                                ))}
                              </div>
                            </div>
                          ) : null}

                          <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                            <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
                              <div>
                                <div className="font-medium">Channel tool scope</div>
                                <div className="tcp-subtle text-xs">
                                  Built-in tools and MCP servers available to {channel} sessions.
                                </div>
                                {toolPrefs.enabled_tools.length > 0 ? (
                                  <div className="mt-1 text-xs text-amber-300">
                                    Explicit built-in allowlist is active for this channel.
                                  </div>
                                ) : null}
                                {publicDemo ? (
                                  <div className="mt-1 text-xs text-slate-400">
                                    Public demo profile can only expose web and quarantined
                                    public-memory tools here. File, shell, MCP, and operator-facing
                                    tools stay disabled even if saved in channel preferences.
                                  </div>
                                ) : null}
                              </div>
                              <button
                                className="tcp-btn"
                                disabled={saveChannelToolPreferencesMutation.isPending}
                                onClick={() =>
                                  saveChannelToolPreferencesMutation.mutate({
                                    channel,
                                    payload: { reset: true },
                                  })
                                }
                              >
                                Reset scope
                              </button>
                            </div>

                            <div className="grid gap-3">
                              {CHANNEL_TOOL_GROUPS.map((group) => (
                                <div key={`${channel}-${group.label}`} className="grid gap-2">
                                  <div className="tcp-subtle text-[11px] uppercase tracking-[0.24em]">
                                    {group.label}
                                  </div>
                                  <div className="grid gap-2 md:grid-cols-2">
                                    {group.tools.map((tool) => {
                                      const allowed = toolAllowedForSecurityProfile(
                                        draft.securityProfile,
                                        tool
                                      );
                                      const enabled = toolEnabledForSecurityProfile(
                                        toolPrefs,
                                        tool,
                                        draft.securityProfile
                                      );
                                      return (
                                        <label
                                          key={`${channel}-${tool}`}
                                          className="flex items-center justify-between rounded-xl border border-slate-700/60 bg-slate-950/30 px-3 py-2 text-sm"
                                        >
                                          <div className="flex flex-col">
                                            <span className="font-mono text-xs">{tool}</span>
                                            {!allowed ? (
                                              <span className="tcp-subtle text-[11px]">
                                                Disabled by security profile
                                              </span>
                                            ) : null}
                                          </div>
                                          <input
                                            type="checkbox"
                                            checked={enabled}
                                            disabled={
                                              saveChannelToolPreferencesMutation.isPending ||
                                              !allowed
                                            }
                                            onChange={(e) =>
                                              saveChannelToolPreferencesMutation.mutate({
                                                channel,
                                                payload: nextChannelToolPreferences(
                                                  toolPrefs,
                                                  tool,
                                                  e.currentTarget.checked
                                                ),
                                              })
                                            }
                                          />
                                        </label>
                                      );
                                    })}
                                  </div>
                                </div>
                              ))}

                              <div className="grid gap-2">
                                <div className="tcp-subtle text-[11px] uppercase tracking-[0.24em]">
                                  MCP servers
                                </div>
                                {mcpServers.length ? (
                                  <div className="grid gap-2 md:grid-cols-2">
                                    {mcpServers.map((server) => {
                                      const enabled =
                                        !publicDemo &&
                                        toolPrefs.enabled_mcp_servers.includes(server.name);
                                      return (
                                        <label
                                          key={`${channel}-mcp-${server.name}`}
                                          className="flex items-center justify-between rounded-xl border border-slate-700/60 bg-slate-950/30 px-3 py-2 text-sm"
                                        >
                                          <div className="flex flex-col">
                                            <span className="font-mono text-xs">{server.name}</span>
                                            {publicDemo ? (
                                              <span className="tcp-subtle text-[11px]">
                                                Disabled by security profile
                                              </span>
                                            ) : null}
                                          </div>
                                          <input
                                            type="checkbox"
                                            checked={enabled}
                                            disabled={
                                              saveChannelToolPreferencesMutation.isPending ||
                                              publicDemo
                                            }
                                            onChange={(e) =>
                                              saveChannelToolPreferencesMutation.mutate({
                                                channel,
                                                payload: nextChannelMcpPreferences(
                                                  toolPrefs,
                                                  server.name,
                                                  e.currentTarget.checked
                                                ),
                                              })
                                            }
                                          />
                                        </label>
                                      );
                                    })}
                                  </div>
                                ) : (
                                  <div className="tcp-subtle text-xs">
                                    {publicDemo
                                      ? "MCP servers stay disabled in public demo mode."
                                      : "No MCP servers configured yet."}
                                  </div>
                                )}
                              </div>
                            </div>
                          </div>

                          <div className="flex flex-wrap gap-2">
                            <button
                              className="tcp-btn-primary"
                              disabled={saveChannelMutation.isPending}
                              onClick={() => saveChannelMutation.mutate(channel)}
                            >
                              <i data-lucide="save"></i>
                              Save
                            </button>
                            {channel === "discord" ? (
                              <button
                                className="tcp-btn"
                                disabled={verifyChannelMutation.isPending}
                                onClick={() => verifyChannelMutation.mutate("discord")}
                              >
                                <i data-lucide="shield-check"></i>
                                Verify
                              </button>
                            ) : null}
                            <button
                              className="tcp-btn-danger"
                              disabled={deleteChannelMutation.isPending || !hasSavedConfig}
                              onClick={() => deleteChannelMutation.mutate(channel)}
                            >
                              <i data-lucide="trash-2"></i>
                              Remove
                            </button>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </PanelCard>
              ) : null}

              {activeSection === "mcp" ? (
                <PanelCard
                  title="MCP connections"
                  subtitle="Configured MCP servers, connection state, and discovered tool coverage."
                  actions={
                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Badge tone={connectedMcpCount ? "ok" : "warn"}>
                        {connectedMcpCount}/{mcpServers.length} connected
                      </Badge>
                      <Badge tone="info">{mcpToolIds.length} tools</Badge>
                      <button className="tcp-btn-primary" onClick={() => openMcpModal()}>
                        <i data-lucide="plus"></i>
                        Add MCP server
                      </button>
                      <button className="tcp-btn" onClick={() => void invalidateMcp()}>
                        <i data-lucide="refresh-cw"></i>
                        Reload
                      </button>
                    </div>
                  }
                >
                  <div className="grid gap-3">
                    {mcpServers.length ? (
                      mcpServers.map((server) => {
                        const headerKeys = Object.keys(server.headers || {}).filter(Boolean);
                        const toolCount = Array.isArray(server.toolCache)
                          ? server.toolCache.length
                          : 0;
                        return (
                          <div key={server.name} className="tcp-list-item grid gap-2">
                            <div className="flex flex-wrap items-center justify-between gap-2">
                              <div>
                                <div className="font-semibold">{server.name}</div>
                                <div className="tcp-subtle text-sm">
                                  {server.transport || "No transport set"}
                                </div>
                              </div>
                              <div className="flex flex-wrap gap-2">
                                <Badge tone={server.connected ? "ok" : "warn"}>
                                  {server.connected ? "Connected" : "Disconnected"}
                                </Badge>
                                <Badge tone={server.enabled ? "info" : "warn"}>
                                  {server.enabled ? "Enabled" : "Disabled"}
                                </Badge>
                                <Badge tone="info">{toolCount} tools</Badge>
                              </div>
                            </div>
                            {server.lastError ? (
                              <div className="rounded-xl border border-rose-700/60 bg-rose-950/20 px-2 py-1 text-xs text-rose-300">
                                {server.lastError}
                              </div>
                            ) : null}
                            <div className="tcp-subtle text-xs">
                              {headerKeys.length
                                ? `Auth headers: ${headerKeys.join(", ")}`
                                : "No stored auth headers."}
                            </div>
                            <div className="flex flex-wrap gap-2">
                              <button className="tcp-btn" onClick={() => openMcpModal(server)}>
                                Edit
                              </button>
                              <button
                                className="tcp-btn"
                                disabled={mcpActionMutation.isPending}
                                onClick={() =>
                                  mcpActionMutation.mutate({
                                    action: server.connected ? "disconnect" : "connect",
                                    server,
                                  })
                                }
                              >
                                {server.connected ? "Disconnect" : "Connect"}
                              </button>
                              <button
                                className="tcp-btn"
                                disabled={mcpActionMutation.isPending}
                                onClick={() =>
                                  mcpActionMutation.mutate({ action: "refresh", server })
                                }
                              >
                                Refresh
                              </button>
                              <button
                                className="tcp-btn"
                                disabled={mcpActionMutation.isPending}
                                onClick={() =>
                                  mcpActionMutation.mutate({ action: "toggle-enabled", server })
                                }
                              >
                                {server.enabled ? "Disable" : "Enable"}
                              </button>
                              <button
                                className="tcp-btn-danger"
                                disabled={mcpActionMutation.isPending}
                                onClick={() =>
                                  mcpActionMutation.mutate({ action: "delete", server })
                                }
                              >
                                Delete
                              </button>
                            </div>
                          </div>
                        );
                      })
                    ) : (
                      <div className="grid gap-3">
                        <EmptyState text="No MCP servers configured." />
                        <div className="flex justify-start">
                          <button className="tcp-btn-primary" onClick={() => openMcpModal()}>
                            <i data-lucide="plus"></i>
                            Add MCP server
                          </button>
                        </div>
                      </div>
                    )}

                    <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                      <div className="mb-2 font-medium">Discovered tools</div>
                      <pre className="tcp-code max-h-56 overflow-auto whitespace-pre-wrap break-words">
                        {mcpToolIds.length
                          ? mcpToolIds.slice(0, 250).join("\n")
                          : "No MCP tools discovered yet. Connect a server first."}
                      </pre>
                    </div>
                  </div>
                </PanelCard>
              ) : null}

              {activeSection === "incident_monitor" ? (
                <PanelCard
                  title="Incident Monitor"
                  actions={
                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Badge
                        tone={
                          incidentMonitorStatus.runtime?.monitoring_active
                            ? incidentMonitorStatus.readiness?.publish_ready
                              ? "ok"
                              : "info"
                            : incidentMonitorStatus.readiness?.ingest_ready
                              ? "info"
                              : "warn"
                        }
                      >
                        {incidentMonitorStatus.runtime?.monitoring_active
                          ? incidentMonitorStatus.readiness?.publish_ready
                            ? "Monitoring"
                            : "Watching locally"
                          : incidentMonitorStatus.readiness?.ingest_ready
                            ? "Ready"
                            : "Not ready"}
                      </Badge>
                      {incidentMonitorPaused || incidentMonitorStatus.runtime?.paused ? (
                        <Badge tone="warn">Paused</Badge>
                      ) : null}
                      <Badge tone="info">
                        {Number(incidentMonitorStatus.runtime?.pending_incidents || 0)} incidents
                      </Badge>
                      <Badge tone="info">
                        {Number(incidentMonitorStatus.pending_drafts || 0)} pending drafts
                      </Badge>
                      <Badge tone="info">
                        {Number(incidentMonitorStatus.pending_posts || 0)} post attempts
                      </Badge>
                      <button
                        className="tcp-icon-btn"
                        title="Reload status"
                        aria-label="Reload status"
                        onClick={() =>
                          Promise.all([
                            incidentMonitorStatusQuery.refetch(),
                            incidentMonitorDraftsQuery.refetch(),
                            incidentMonitorIncidentsQuery.refetch(),
                            incidentMonitorPostsQuery.refetch(),
                          ]).then(() => toast("ok", "Incident Monitor status refreshed."))
                        }
                      >
                        <i data-lucide="refresh-cw"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title={
                          incidentMonitorPaused || incidentMonitorStatus.runtime?.paused
                            ? "Resume monitoring"
                            : "Pause monitoring"
                        }
                        aria-label={
                          incidentMonitorPaused || incidentMonitorStatus.runtime?.paused
                            ? "Resume monitoring"
                            : "Pause monitoring"
                        }
                        disabled={incidentMonitorPauseResumeMutation.isPending}
                        onClick={() =>
                          incidentMonitorPauseResumeMutation.mutate({
                            action:
                              incidentMonitorPaused || incidentMonitorStatus.runtime?.paused
                                ? "resume"
                                : "pause",
                          })
                        }
                      >
                        <i
                          data-lucide={
                            incidentMonitorPaused || incidentMonitorStatus.runtime?.paused
                              ? "play"
                              : "pause"
                          }
                        ></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Refresh capability bindings"
                        aria-label="Refresh capability bindings"
                        disabled={refreshIncidentMonitorBindingsMutation.isPending}
                        onClick={() => refreshIncidentMonitorBindingsMutation.mutate()}
                      >
                        <i data-lucide="rotate-cw"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Copy debug payload"
                        aria-label="Copy debug payload"
                        onClick={() => void copyIncidentMonitorDebugPayload()}
                      >
                        <i data-lucide="copy"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Open GitHub MCP guide"
                        aria-label="Open GitHub MCP guide"
                        onClick={() => setGithubMcpGuideOpen(true)}
                      >
                        <i data-lucide="book-open"></i>
                      </button>
                    </div>
                  }
                >
                  <div className="grid gap-4">
                    <div className="grid gap-3 md:grid-cols-2">
                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          Reporter state
                        </span>
                        <button
                          type="button"
                          className={`tcp-list-item text-left ${incidentMonitorEnabled ? "ring-1 ring-emerald-400/40" : ""}`}
                          onClick={() => setIncidentMonitorEnabled((prev) => !prev)}
                        >
                          <div className="font-medium">
                            {incidentMonitorEnabled
                              ? incidentMonitorPaused
                                ? "Paused"
                                : "Enabled"
                              : "Disabled"}
                          </div>
                          <div className="tcp-subtle text-xs">
                            {incidentMonitorEnabled
                              ? incidentMonitorPaused
                                ? "Monitoring is paused. Resume to process new failures."
                                : "Failure events can be analyzed once readiness is green."
                              : "No reporter work will execute."}
                          </div>
                        </button>
                      </label>

                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          Local directory
                        </span>
                        <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
                          <button
                            className="tcp-btn"
                            type="button"
                            onClick={() => {
                              const seed = String(incidentMonitorWorkspaceRoot || "/").trim();
                              setIncidentMonitorWorkspaceBrowserDir(seed || "/");
                              setIncidentMonitorWorkspaceBrowserSearch("");
                              setIncidentMonitorWorkspaceBrowserOpen(true);
                            }}
                          >
                            <i data-lucide="folder-open"></i>
                            Browse
                          </button>
                          <input
                            className="tcp-input"
                            readOnly
                            value={incidentMonitorWorkspaceRoot}
                            placeholder="No local directory selected. Use Browse."
                          />
                          <button
                            className="tcp-btn"
                            type="button"
                            onClick={() => setIncidentMonitorWorkspaceRoot("")}
                            disabled={!incidentMonitorWorkspaceRoot}
                          >
                            <i data-lucide="x"></i>
                            Clear
                          </button>
                        </div>
                        <div className="tcp-subtle text-xs">
                          {incidentMonitorWorkspaceRoot
                            ? `Reporter analysis root: ${incidentMonitorWorkspaceRoot}`
                            : "Defaults to the engine workspace root if not set."}
                        </div>
                      </label>

                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          Target repo
                        </span>
                        <input
                          className="tcp-input"
                          value={incidentMonitorRepo}
                          onChange={(event) => setIncidentMonitorRepo(event.target.value)}
                          placeholder="owner/repo"
                        />
                      </label>

                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          MCP server
                        </span>
                        <select
                          className="tcp-input"
                          value={incidentMonitorMcpServer}
                          onChange={(event) => setIncidentMonitorMcpServer(event.target.value)}
                        >
                          <option value="">Select an MCP server</option>
                          {mcpServers.map((server) => (
                            <option key={server.name} value={server.name}>
                              {server.name}
                            </option>
                          ))}
                        </select>
                      </label>

                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          Provider preference
                        </span>
                        <select
                          className="tcp-input"
                          value={incidentMonitorProviderPreference}
                          onChange={(event) =>
                            setIncidentMonitorProviderPreference(event.target.value)
                          }
                        >
                          <option value="auto">Auto</option>
                          <option value="official_github">Official GitHub</option>
                          <option value="composio">Composio</option>
                          <option value="arcade">Arcade</option>
                        </select>
                      </label>

                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          Provider
                        </span>
                        <select
                          className="tcp-input"
                          value={incidentMonitorProviderId}
                          onChange={(event) => {
                            const nextProvider = event.target.value;
                            setIncidentMonitorProviderId(nextProvider);
                            setIncidentMonitorModelId("");
                          }}
                        >
                          <option value="">Select a provider</option>
                          {providers.map((provider: any) => (
                            <option
                              key={String(provider?.id || "")}
                              value={String(provider?.id || "")}
                            >
                              {String(provider?.id || "")}
                            </option>
                          ))}
                        </select>
                      </label>

                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          Model
                        </span>
                        <input
                          className="tcp-input"
                          value={incidentMonitorModelId}
                          onChange={(event) => setIncidentMonitorModelId(event.target.value)}
                          list="incident-monitor-models"
                          disabled={!incidentMonitorProviderId}
                          placeholder={
                            incidentMonitorProviderId
                              ? "Type or paste a model id"
                              : "Choose a provider first"
                          }
                          spellCheck={false}
                        />
                        <datalist id="incident-monitor-models">
                          {incidentMonitorProviderModels.map((modelId) => (
                            <option key={modelId} value={modelId} />
                          ))}
                        </datalist>
                        <div className="tcp-subtle text-xs">
                          {incidentMonitorProviderId
                            ? incidentMonitorProviderModels.length
                              ? `${incidentMonitorProviderModels.length} suggested models from provider catalog`
                              : "No provider catalog models available. Manual model ids are allowed."
                            : "Select a provider to load model suggestions."}
                        </div>
                      </label>

                      <div className="grid gap-2 md:col-span-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          GitHub posting
                        </span>
                        <div className="grid gap-2 md:grid-cols-3">
                          <button
                            type="button"
                            className={`tcp-list-item text-left ${incidentMonitorAutoCreateIssues && !incidentMonitorRequireApproval ? "ring-1 ring-emerald-400/40" : ""}`}
                            onClick={() => {
                              setIncidentMonitorAutoCreateIssues((prev) => !prev);
                              if (
                                incidentMonitorRequireApproval &&
                                incidentMonitorAutoCreateIssues
                              ) {
                                setIncidentMonitorRequireApproval(false);
                              }
                            }}
                          >
                            <div className="font-medium">Auto-create new issues</div>
                            <div className="tcp-subtle text-xs">
                              {incidentMonitorAutoCreateIssues
                                ? "New drafts post to GitHub automatically."
                                : "New drafts stay internal until published manually."}
                            </div>
                          </button>
                          <button
                            type="button"
                            className={`tcp-list-item text-left ${incidentMonitorRequireApproval ? "ring-1 ring-amber-400/40" : ""}`}
                            onClick={() => {
                              setIncidentMonitorRequireApproval((prev) => {
                                const next = !prev;
                                if (next) setIncidentMonitorAutoCreateIssues(false);
                                return next;
                              });
                            }}
                          >
                            <div className="font-medium">Require approval</div>
                            <div className="tcp-subtle text-xs">
                              {incidentMonitorRequireApproval
                                ? "New drafts wait for a manual publish click."
                                : "Approval gate disabled."}
                            </div>
                          </button>
                          <button
                            type="button"
                            className={`tcp-list-item text-left ${incidentMonitorAutoComment ? "ring-1 ring-sky-400/40" : ""}`}
                            onClick={() => setIncidentMonitorAutoComment((prev) => !prev)}
                          >
                            <div className="font-medium">Auto-comment matches</div>
                            <div className="tcp-subtle text-xs">
                              {incidentMonitorAutoComment
                                ? "Open matching GitHub issues receive new evidence comments."
                                : "Matching issues are detected but not updated automatically."}
                            </div>
                          </button>
                        </div>
                      </div>
                    </div>

                    <div className="flex flex-wrap gap-2">
                      <button
                        className="tcp-btn-primary"
                        disabled={saveIncidentMonitorMutation.isPending}
                        title="Save Incident Monitor settings"
                        aria-label="Save Incident Monitor settings"
                        onClick={() => saveIncidentMonitorMutation.mutate()}
                      >
                        <i data-lucide="save"></i>
                        {saveIncidentMonitorMutation.isPending ? "Saving..." : null}
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Add MCP server"
                        aria-label="Add MCP server"
                        onClick={() => openMcpModal()}
                      >
                        <i data-lucide="plus"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Open setup guide"
                        aria-label="Open setup guide"
                        onClick={() => setGithubMcpGuideOpen(true)}
                      >
                        <i data-lucide="external-link"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Refresh capability bindings"
                        aria-label="Refresh capability bindings"
                        disabled={refreshIncidentMonitorBindingsMutation.isPending}
                        onClick={() => refreshIncidentMonitorBindingsMutation.mutate()}
                      >
                        <i data-lucide="rotate-cw"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Copy debug payload"
                        aria-label="Copy debug payload"
                        onClick={() => void copyIncidentMonitorDebugPayload()}
                      >
                        <i data-lucide="copy"></i>
                      </button>
                      {selectedIncidentMonitorServer ? (
                        <button
                          className="tcp-icon-btn"
                          title={
                            selectedIncidentMonitorServer.connected
                              ? "Refresh selected MCP"
                              : "Connect selected MCP"
                          }
                          aria-label={
                            selectedIncidentMonitorServer.connected
                              ? "Refresh selected MCP"
                              : "Connect selected MCP"
                          }
                          disabled={mcpActionMutation.isPending}
                          onClick={() =>
                            mcpActionMutation.mutate({
                              action: selectedIncidentMonitorServer.connected
                                ? "refresh"
                                : "connect",
                              server: selectedIncidentMonitorServer,
                            })
                          }
                        >
                          <i
                            data-lucide={
                              selectedIncidentMonitorServer.connected ? "refresh-cw" : "plug-zap"
                            }
                          ></i>
                        </button>
                      ) : null}
                    </div>

                    <div className="grid gap-3 md:grid-cols-3">
                      <div className="tcp-list-item">
                        <div className="text-sm font-medium">Readiness</div>
                        <div className="mt-1 text-sm">
                          {incidentMonitorStatus.runtime?.monitoring_active
                            ? incidentMonitorStatus.readiness?.publish_ready
                              ? "Monitoring"
                              : "Watching locally"
                            : incidentMonitorStatus.runtime?.paused || incidentMonitorPaused
                              ? "Paused"
                              : incidentMonitorStatus.readiness?.ingest_ready
                                ? "Ready"
                                : "Blocked"}
                        </div>
                        <div className="tcp-subtle text-xs">
                          {incidentMonitorStatus.runtime?.last_runtime_error ||
                            incidentMonitorStatus.last_error ||
                            "No blocking issue reported."}
                        </div>
                        {!incidentMonitorStatus.readiness?.publish_ready &&
                        Array.isArray(incidentMonitorStatus.missing_required_capabilities) &&
                        incidentMonitorStatus.missing_required_capabilities.length ? (
                          <div className="tcp-subtle mt-2 text-xs">
                            Missing:{" "}
                            {incidentMonitorStatus.missing_required_capabilities.join(", ")}
                          </div>
                        ) : null}
                      </div>
                      <div className="tcp-list-item">
                        <div className="text-sm font-medium">Selected MCP</div>
                        <div className="mt-1 text-sm">
                          {selectedIncidentMonitorServer?.name || "None selected"}
                        </div>
                        <div className="tcp-subtle text-xs">
                          {selectedIncidentMonitorServer
                            ? selectedIncidentMonitorServer.connected
                              ? "Connected"
                              : "Disconnected"
                            : "No server selected"}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          Bindings:{" "}
                          {incidentMonitorStatus.binding_source_version || "unknown version"}
                          {incidentMonitorStatus.bindings_last_merged_at_ms
                            ? ` · merged ${new Date(incidentMonitorStatus.bindings_last_merged_at_ms).toLocaleString()}`
                            : ""}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          Local directory:{" "}
                          {incidentMonitorWorkspaceRoot ||
                            String(incidentMonitorStatus.config?.workspace_root || "").trim() ||
                            "engine workspace root"}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          Last event:{" "}
                          {String(
                            incidentMonitorStatus.runtime?.last_incident_event_type || ""
                          ).trim() || "No incidents processed yet"}
                        </div>
                      </div>
                      <div className="tcp-list-item">
                        <div className="text-sm font-medium">Model route</div>
                        <div className="mt-1 break-all text-sm">
                          {incidentMonitorStatus.selected_model?.provider_id &&
                          incidentMonitorStatus.selected_model?.model_id
                            ? `${incidentMonitorStatus.selected_model.provider_id} / ${incidentMonitorStatus.selected_model.model_id}`
                            : "No dedicated model selected"}
                        </div>
                        <div className="tcp-subtle text-xs">
                          {incidentMonitorStatus.readiness?.selected_model_ready
                            ? "Available"
                            : "Fail-closed when unavailable"}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          Last processed:{" "}
                          {incidentMonitorStatus.runtime?.last_processed_at_ms
                            ? new Date(
                                Number(incidentMonitorStatus.runtime.last_processed_at_ms)
                              ).toLocaleString()
                            : "Not processed yet"}
                        </div>
                      </div>
                    </div>

                    <div className="grid gap-3 md:grid-cols-2">
                      <div className="tcp-list-item">
                        <div className="font-medium">Capability readiness</div>
                        <div className="tcp-subtle mt-2 grid gap-1 text-xs">
                          <div>
                            github.list_issues:{" "}
                            {incidentMonitorStatus.required_capabilities?.github_list_issues
                              ? "ready"
                              : "missing"}
                          </div>
                          <div>
                            github.get_issue:{" "}
                            {incidentMonitorStatus.required_capabilities?.github_get_issue
                              ? "ready"
                              : "missing"}
                          </div>
                          <div>
                            github.create_issue:{" "}
                            {incidentMonitorStatus.required_capabilities?.github_create_issue
                              ? "ready"
                              : "missing"}
                          </div>
                          <div>
                            github.comment_on_issue:{" "}
                            {incidentMonitorStatus.required_capabilities?.github_comment_on_issue
                              ? "ready"
                              : "missing"}
                          </div>
                        </div>
                        {Array.isArray(incidentMonitorStatus.resolved_capabilities) &&
                        incidentMonitorStatus.resolved_capabilities.length ? (
                          <div className="tcp-subtle mt-3 grid gap-1 text-xs">
                            {incidentMonitorStatus.resolved_capabilities.map((row, index) => (
                              <div key={`${row.capability_id || "cap"}-${index}`}>
                                {String(row.capability_id || "unknown")}:{" "}
                                {String(row.tool_name || "unresolved")}
                              </div>
                            ))}
                          </div>
                        ) : null}
                        {Array.isArray(incidentMonitorStatus.selected_server_binding_candidates) &&
                        incidentMonitorStatus.selected_server_binding_candidates.length ? (
                          <div className="tcp-subtle mt-3 grid gap-1 text-xs">
                            {incidentMonitorStatus.selected_server_binding_candidates.map(
                              (row, index) => (
                                <div key={`${row.capability_id || "candidate"}-${index}`}>
                                  {String(row.capability_id || "unknown")}:{" "}
                                  {String(row.binding_tool_name || "unknown")}
                                  {row.matched ? " · matched" : " · candidate"}
                                </div>
                              )
                            )}
                          </div>
                        ) : null}
                        {Array.isArray(incidentMonitorStatus.discovered_mcp_tools) &&
                        incidentMonitorStatus.discovered_mcp_tools.length ? (
                          <div className="mt-3">
                            <div className="tcp-subtle text-xs font-medium">
                              Discovered MCP tools
                            </div>
                            <pre className="tcp-code mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs">
                              {incidentMonitorStatus.discovered_mcp_tools.join("\n")}
                            </pre>
                          </div>
                        ) : (
                          <div className="tcp-subtle mt-3 text-xs">
                            No MCP tools were discovered for the selected server.
                          </div>
                        )}
                      </div>

                      <div className="tcp-list-item">
                        <div className="font-medium">Posting policy</div>
                        <div className="tcp-subtle mt-2 grid gap-1 text-xs">
                          <div>
                            New issues:{" "}
                            {incidentMonitorRequireApproval
                              ? "Manual publish"
                              : incidentMonitorAutoCreateIssues
                                ? "Auto-create"
                                : "Internal draft only"}
                          </div>
                          <div>
                            Matched open issues:{" "}
                            {incidentMonitorAutoComment ? "Auto-comment" : "Detect only"}
                          </div>
                          <div>Dedupe: Fingerprint marker + label</div>
                          <div>Labels: incident-monitor</div>
                          <div>Workspace write tools: Disabled</div>
                          <div>Model fallback: Fail closed</div>
                        </div>
                      </div>
                    </div>

                    <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                      <div className="mb-2 font-medium">Recent incidents</div>
                      {incidentMonitorIncidents.length ? (
                        <div className="grid gap-2">
                          {incidentMonitorIncidents.map((incident) => (
                            <div key={incident.incident_id} className="tcp-list-item">
                              <div className="flex flex-wrap items-center justify-between gap-2">
                                <div className="font-medium">
                                  {incident.title || incident.event_type}
                                </div>
                                <Badge tone={incident.last_error ? "warn" : "info"}>
                                  {incident.status}
                                </Badge>
                              </div>
                              <div className="tcp-subtle mt-1 text-xs">
                                {incident.event_type} · seen{" "}
                                {Number(incident.occurrence_count || 0)}x{" · "}
                                {incident.updated_at_ms
                                  ? new Date(incident.updated_at_ms).toLocaleString()
                                  : "time unavailable"}
                              </div>
                              <div className="tcp-subtle mt-1 text-xs">
                                {incident.workspace_root || "engine workspace root"}
                              </div>
                              {incident.last_error ? (
                                <div className="tcp-subtle mt-1 text-xs">{incident.last_error}</div>
                              ) : null}
                              {incident.detail ? (
                                <div className="tcp-subtle mt-1 text-xs">{incident.detail}</div>
                              ) : null}
                              <div className="mt-3 flex flex-wrap gap-2">
                                <button
                                  className="tcp-icon-btn"
                                  title="Replay triage for this incident"
                                  aria-label="Replay triage for this incident"
                                  disabled={incidentMonitorReplayIncidentMutation.isPending}
                                  onClick={() =>
                                    incidentMonitorReplayIncidentMutation.mutate({
                                      incidentId: incident.incident_id,
                                    })
                                  }
                                >
                                  <i data-lucide="rotate-cw"></i>
                                </button>
                                {incident.triage_run_id ? (
                                  <span className="tcp-subtle text-xs">
                                    triage run: {incident.triage_run_id}
                                  </span>
                                ) : null}
                                {incident.draft_id ? (
                                  <span className="tcp-subtle text-xs">
                                    draft: {incident.draft_id}
                                  </span>
                                ) : null}
                              </div>
                            </div>
                          ))}
                        </div>
                      ) : (
                        <EmptyState text="No Incident Monitor incidents yet." />
                      )}
                    </div>

                    <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                      <div className="mb-2 font-medium">Recent reporter drafts</div>
                      {incidentMonitorDrafts.length ? (
                        <div className="grid gap-2">
                          {incidentMonitorDrafts.map((draft) => (
                            <div key={draft.draft_id} className="tcp-list-item">
                              <div className="flex flex-wrap items-center justify-between gap-2">
                                <div className="font-medium">
                                  {draft.title || draft.fingerprint}
                                </div>
                                <Badge
                                  tone={draft.status === "approval_required" ? "warn" : "info"}
                                >
                                  {draft.status}
                                </Badge>
                              </div>
                              <div className="tcp-subtle mt-1 text-xs">
                                {draft.repo} ·{" "}
                                {draft.issue_number ? `issue #${draft.issue_number}` : "draft only"}{" "}
                                ·{" "}
                                {draft.created_at_ms
                                  ? new Date(draft.created_at_ms).toLocaleString()
                                  : "time unavailable"}
                              </div>
                              {draft.github_status ? (
                                <div className="tcp-subtle mt-1 text-xs">
                                  GitHub: {draft.github_status}
                                  {draft.matched_issue_number
                                    ? ` · matched #${draft.matched_issue_number}${draft.matched_issue_state ? ` (${draft.matched_issue_state})` : ""}`
                                    : ""}
                                </div>
                              ) : null}
                              {draft.detail ? (
                                <div className="tcp-subtle mt-1 text-xs">{draft.detail}</div>
                              ) : null}
                              {draft.last_post_error ? (
                                <div className="tcp-subtle mt-1 text-xs">
                                  {draft.last_post_error}
                                </div>
                              ) : null}
                              {draft.triage_run_id ? (
                                <div className="tcp-subtle mt-2 text-xs">
                                  triage run: {draft.triage_run_id}
                                </div>
                              ) : null}
                              {draft.status === "approval_required" ? (
                                <div className="mt-3 flex flex-wrap gap-2">
                                  <button
                                    className="tcp-btn-primary"
                                    disabled={incidentMonitorDraftDecisionMutation.isPending}
                                    title="Approve draft"
                                    aria-label="Approve draft"
                                    onClick={() =>
                                      incidentMonitorDraftDecisionMutation.mutate({
                                        draftId: draft.draft_id,
                                        decision: "approve",
                                      })
                                    }
                                  >
                                    <i data-lucide="check"></i>
                                    {incidentMonitorDraftDecisionMutation.isPending
                                      ? "Updating..."
                                      : null}
                                  </button>
                                  <button
                                    className="tcp-icon-btn"
                                    title="Deny draft"
                                    aria-label="Deny draft"
                                    disabled={incidentMonitorDraftDecisionMutation.isPending}
                                    onClick={() =>
                                      incidentMonitorDraftDecisionMutation.mutate({
                                        draftId: draft.draft_id,
                                        decision: "deny",
                                      })
                                    }
                                  >
                                    <i data-lucide="x"></i>
                                  </button>
                                </div>
                              ) : null}
                              {!draft.issue_number ? (
                                <div className="mt-3 flex flex-wrap gap-2">
                                  <button
                                    className="tcp-icon-btn"
                                    title="Publish this draft to GitHub now"
                                    aria-label="Publish this draft to GitHub now"
                                    disabled={incidentMonitorPublishDraftMutation.isPending}
                                    onClick={() =>
                                      incidentMonitorPublishDraftMutation.mutate({
                                        draftId: draft.draft_id,
                                      })
                                    }
                                  >
                                    <i data-lucide="shield-alert"></i>
                                  </button>
                                  <button
                                    className="tcp-icon-btn"
                                    title="Recheck GitHub for an existing matching issue"
                                    aria-label="Recheck GitHub for an existing matching issue"
                                    disabled={incidentMonitorRecheckMatchMutation.isPending}
                                    onClick={() =>
                                      incidentMonitorRecheckMatchMutation.mutate({
                                        draftId: draft.draft_id,
                                      })
                                    }
                                  >
                                    <i data-lucide="refresh-cw"></i>
                                  </button>
                                </div>
                              ) : null}
                              {(draft.github_issue_url || draft.github_comment_url) && (
                                <div className="mt-3 flex flex-wrap gap-2 text-xs">
                                  {draft.github_issue_url ? (
                                    <a
                                      className="tcp-btn"
                                      href={draft.github_issue_url}
                                      target="_blank"
                                      rel="noreferrer"
                                    >
                                      <i data-lucide="external-link"></i>
                                      Open issue
                                    </a>
                                  ) : null}
                                  {draft.github_comment_url ? (
                                    <a
                                      className="tcp-btn"
                                      href={draft.github_comment_url}
                                      target="_blank"
                                      rel="noreferrer"
                                    >
                                      <i data-lucide="message-square"></i>
                                      Open comment
                                    </a>
                                  ) : null}
                                </div>
                              )}
                              {(draft.status === "draft_ready" ||
                                draft.status === "triage_queued") &&
                              !draft.triage_run_id ? (
                                <div className="mt-3 flex flex-wrap gap-2">
                                  <button
                                    className="tcp-icon-btn"
                                    title="Create triage run"
                                    aria-label="Create triage run"
                                    disabled={incidentMonitorTriageRunMutation.isPending}
                                    onClick={() =>
                                      incidentMonitorTriageRunMutation.mutate({
                                        draftId: draft.draft_id,
                                      })
                                    }
                                  >
                                    <i data-lucide="sparkles"></i>
                                  </button>
                                </div>
                              ) : null}
                            </div>
                          ))}
                        </div>
                      ) : (
                        <EmptyState text="No Incident Monitor drafts yet." />
                      )}
                    </div>

                    <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                      <div className="mb-2 font-medium">Recent GitHub posts</div>
                      {incidentMonitorPosts.length ? (
                        <div className="grid gap-2">
                          {incidentMonitorPosts.map((post) => (
                            <div key={post.post_id} className="tcp-list-item">
                              <div className="flex flex-wrap items-center justify-between gap-2">
                                <div className="font-medium">{post.operation}</div>
                                <Badge tone={post.status === "posted" ? "ok" : "warn"}>
                                  {post.status}
                                </Badge>
                              </div>
                              <div className="tcp-subtle mt-1 text-xs">
                                {post.repo}
                                {post.issue_number ? ` · issue #${post.issue_number}` : ""}
                                {post.updated_at_ms
                                  ? ` · ${new Date(post.updated_at_ms).toLocaleString()}`
                                  : ""}
                              </div>
                              {post.error ? (
                                <div className="tcp-subtle mt-1 text-xs">{post.error}</div>
                              ) : null}
                              <div className="mt-3 flex flex-wrap gap-2">
                                {post.issue_url ? (
                                  <a
                                    className="tcp-btn"
                                    href={post.issue_url}
                                    target="_blank"
                                    rel="noreferrer"
                                  >
                                    <i data-lucide="external-link"></i>
                                    Open issue
                                  </a>
                                ) : null}
                                {post.comment_url ? (
                                  <a
                                    className="tcp-btn"
                                    href={post.comment_url}
                                    target="_blank"
                                    rel="noreferrer"
                                  >
                                    <i data-lucide="message-square"></i>
                                    Open comment
                                  </a>
                                ) : null}
                              </div>
                            </div>
                          ))}
                        </div>
                      ) : (
                        <EmptyState text="No GitHub post attempts yet." />
                      )}
                    </div>
                  </div>
                </PanelCard>
              ) : null}

              {activeSection === "browser" ? (
                <PanelCard
                  title="Browser readiness"
                  subtitle="Operational browser status, diagnostics, and recovery actions."
                  actions={
                    <Toolbar>
                      <button className="tcp-btn" onClick={() => void browserStatus.refetch()}>
                        <i data-lucide="refresh-cw"></i>
                        Refresh browser status
                      </button>
                      <button
                        className="tcp-btn"
                        onClick={() => installBrowserMutation.mutate()}
                        disabled={installBrowserMutation.isPending}
                      >
                        <i data-lucide="download"></i>
                        {installBrowserMutation.isPending
                          ? "Installing sidecar..."
                          : "Install sidecar"}
                      </button>
                      <button
                        className="tcp-btn"
                        onClick={() => smokeTestBrowserMutation.mutate()}
                        disabled={smokeTestBrowserMutation.isPending}
                      >
                        <i data-lucide="globe"></i>
                        {smokeTestBrowserMutation.isPending
                          ? "Running smoke test..."
                          : "Run smoke test"}
                      </button>
                      <button className="tcp-btn" onClick={() => setDiagnosticsOpen(true)}>
                        <i data-lucide="activity"></i>
                        Diagnostics
                      </button>
                    </Toolbar>
                  }
                >
                  <div className="grid gap-2 md:grid-cols-3">
                    <div className="tcp-list-item">
                      <div className="text-sm font-medium">Status</div>
                      <div className="mt-1 text-sm">
                        {browserStatus.data
                          ? browserStatus.data.runnable
                            ? "Ready"
                            : browserStatus.data.enabled
                              ? "Blocked"
                              : "Disabled"
                          : "Unknown"}
                      </div>
                      <div className="tcp-subtle text-xs">
                        Headless default: {browserStatus.data?.headless_default ? "yes" : "no"}
                      </div>
                    </div>
                    <div className="tcp-list-item">
                      <div className="text-sm font-medium">Sidecar</div>
                      <div className="mt-1 break-all text-sm">
                        {browserStatus.data?.sidecar?.path || "Not found"}
                      </div>
                      <div className="tcp-subtle text-xs">
                        {browserStatus.data?.sidecar?.version || "No version detected"}
                      </div>
                    </div>
                    <div className="tcp-list-item">
                      <div className="text-sm font-medium">Browser</div>
                      <div className="mt-1 break-all text-sm">
                        {browserStatus.data?.browser?.path || "Not found"}
                      </div>
                      <div className="tcp-subtle text-xs">
                        {browserStatus.data?.browser?.version ||
                          browserStatus.data?.browser?.channel ||
                          "No version detected"}
                      </div>
                    </div>
                  </div>
                  {browserIssues.length ? (
                    <div className="mt-3 grid gap-2">
                      {browserIssues.map((issue, index) => (
                        <div key={`${issue.code || "issue"}-${index}`} className="tcp-list-item">
                          <div className="text-sm font-medium">{issue.code || "browser_issue"}</div>
                          <div className="tcp-subtle text-xs">
                            {issue.message || "Unknown browser issue."}
                          </div>
                        </div>
                      ))}
                    </div>
                  ) : null}
                  {browserSmokeResult ? (
                    <div className="mt-3 rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm">
                      <div className="font-medium">
                        Smoke test passed
                        {browserSmokeResult.title ? `: ${browserSmokeResult.title}` : ""}
                      </div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {browserSmokeResult.final_url ||
                          browserSmokeResult.url ||
                          "No URL returned"}
                      </div>
                      <div className="tcp-subtle text-xs">
                        Load state: {browserSmokeResult.load_state || "unknown"} · elements:{" "}
                        {String(browserSmokeResult.element_count ?? 0)} · closed:{" "}
                        {browserSmokeResult.closed ? "yes" : "no"}
                      </div>
                      {browserSmokeResult.excerpt ? (
                        <pre className="tcp-code mt-2 max-h-32 overflow-auto whitespace-pre-wrap break-words">
                          {browserSmokeResult.excerpt}
                        </pre>
                      ) : null}
                    </div>
                  ) : null}
                </PanelCard>
              ) : null}
            </StaggerGroup>
          }
          aside={
            <div className="grid gap-4">
              <PanelCard
                title="Readiness snapshot"
                subtitle="High-signal operational summary for this configuration state."
              >
                <div className="grid gap-2">
                  <div className="tcp-list-item">
                    <div className="font-medium">Connected providers</div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {String(providersCatalog.data?.connected?.length || 0)} connected, default{" "}
                      {String(providersConfig.data?.default || "none")}
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="font-medium">Browser automation</div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {browserStatus.data
                        ? browserStatus.data.runnable
                          ? "Ready"
                          : browserStatus.data.enabled
                            ? "Enabled but blocked"
                            : "Disabled"
                        : "Unknown"}
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="font-medium">Theme</div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {themes.find((theme: any) => theme.id === themeId)?.name || themeId}
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="font-medium">MCP</div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {connectedMcpCount} connected, {mcpToolIds.length} discovered tools
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="font-medium">Incident Monitor</div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {incidentMonitorStatus.readiness?.runtime_ready
                        ? "Ready"
                        : incidentMonitorEnabled
                          ? "Enabled but blocked"
                          : "Disabled"}
                      {" · "}
                      {Number(incidentMonitorStatus.pending_drafts || 0)} pending drafts
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="font-medium">Channels</div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {connectedChannelCount} connected, {channelNames.length} available
                    </div>
                  </div>
                </div>
              </PanelCard>

              <PanelCard title="Quick access" subtitle="Jump straight to the section you need.">
                <div className="grid gap-2">
                  {sectionTabs.map((section) => (
                    <button
                      key={section.id}
                      className="tcp-list-item flex items-center justify-between text-left"
                      onClick={() => setActiveSection(section.id)}
                    >
                      <span className="inline-flex items-center gap-2">
                        <i data-lucide={section.icon}></i>
                        {section.label}
                      </span>
                      {activeSection === section.id ? <Badge tone="ok">open</Badge> : null}
                    </button>
                  ))}
                </div>
              </PanelCard>
            </div>
          }
        />

        <DetailDrawer
          open={githubMcpGuideOpen}
          onClose={() => setGithubMcpGuideOpen(false)}
          title="Official GitHub MCP guide"
        >
          <div className="grid gap-3">
            <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm">
              Recommended for Incident Monitor: use the official GitHub MCP endpoint instead of a
              third-party wrapper when you want stable issue read/write operations.
            </div>

            <div className="grid gap-2 md:grid-cols-2">
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Transport URL</div>
                <div className="mt-1 break-all text-sm">https://api.githubcopilot.com/mcp/</div>
                <div className="tcp-subtle text-xs">
                  Use this as the MCP server transport in Tandem Settings.
                </div>
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Auth mode</div>
                <div className="mt-1 text-sm">Authorization Bearer</div>
                <div className="tcp-subtle text-xs">
                  Paste a GitHub token in the MCP server dialog and use bearer auth.
                </div>
              </div>
            </div>

            <div className="grid gap-2">
              <div className="text-sm font-medium">Recommended setup</div>
              <div className="tcp-list-item text-sm">
                1. Open `Add MCP server`.
                <br />
                2. Name it `github` or another stable name.
                <br />
                3. Set transport to `https://api.githubcopilot.com/mcp/`.
                <br />
                4. Set auth mode to `Authorization Bearer`.
                <br />
                5. Paste a GitHub Personal Access Token.
                <br />
                6. Save, connect, then select that MCP server in Incident Monitor settings.
              </div>
            </div>

            <div className="grid gap-2">
              <div className="text-sm font-medium">Token guidance</div>
              <div className="tcp-list-item text-sm">
                For failure reporting, the token needs issue read/write access on the target
                repository so the runtime can create issues and add comments.
              </div>
            </div>

            <div className="grid gap-2">
              <div className="text-sm font-medium">Direct links</div>
              <div className="flex flex-wrap gap-2">
                <a
                  className="tcp-btn"
                  href="https://github.com/github/github-mcp-server?tab=readme-ov-file"
                  target="_blank"
                  rel="noreferrer"
                >
                  <i data-lucide="external-link"></i>
                  GitHub MCP README
                </a>
                <a
                  className="tcp-btn"
                  href="https://docs.github.com/en/copilot/how-tos/provide-context/use-mcp/use-the-github-mcp-server"
                  target="_blank"
                  rel="noreferrer"
                >
                  <i data-lucide="external-link"></i>
                  GitHub Docs
                </a>
              </div>
            </div>

            <div className="grid gap-2">
              <div className="text-sm font-medium">Issue tools to expect</div>
              <div className="tcp-list-item text-sm">
                The reporter should be able to resolve issue-list, issue-read, issue-create, and
                issue-comment operations from the selected GitHub MCP server. If readiness still
                fails, compare the discovered MCP tools shown in Settings against those issue
                operations.
              </div>
            </div>
          </div>
        </DetailDrawer>

        <AnimatePresence>
          {incidentMonitorWorkspaceBrowserOpen ? (
            <motion.div
              className="tcp-confirm-overlay"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <button
                type="button"
                className="tcp-confirm-backdrop"
                aria-label="Close Incident Monitor workspace dialog"
                onClick={() => {
                  setIncidentMonitorWorkspaceBrowserOpen(false);
                  setIncidentMonitorWorkspaceBrowserSearch("");
                }}
              />
              <motion.div
                className="tcp-confirm-dialog max-w-2xl"
                initial={{ opacity: 0, y: 8, scale: 0.98 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 6, scale: 0.98 }}
              >
                <h3 className="tcp-confirm-title">Select Incident Monitor Directory</h3>
                <p className="tcp-confirm-message">
                  Current: {incidentMonitorCurrentBrowseDir || "n/a"}
                </p>
                <div className="mb-2 flex flex-wrap gap-2">
                  <button
                    className="tcp-btn"
                    onClick={() => {
                      if (!incidentMonitorWorkspaceParentDir) return;
                      setIncidentMonitorWorkspaceBrowserDir(incidentMonitorWorkspaceParentDir);
                    }}
                    disabled={!incidentMonitorWorkspaceParentDir}
                  >
                    <i data-lucide="arrow-up-circle"></i>
                    Up
                  </button>
                  <button
                    className="tcp-btn-primary"
                    onClick={() => {
                      if (!incidentMonitorCurrentBrowseDir) return;
                      setIncidentMonitorWorkspaceRoot(incidentMonitorCurrentBrowseDir);
                      setIncidentMonitorWorkspaceBrowserOpen(false);
                      setIncidentMonitorWorkspaceBrowserSearch("");
                      toast(
                        "ok",
                        `Incident Monitor directory selected: ${incidentMonitorCurrentBrowseDir}`
                      );
                    }}
                  >
                    <i data-lucide="badge-check"></i>
                    Select This Folder
                  </button>
                  <button
                    className="tcp-btn"
                    onClick={() => {
                      setIncidentMonitorWorkspaceBrowserOpen(false);
                      setIncidentMonitorWorkspaceBrowserSearch("");
                    }}
                  >
                    <i data-lucide="x"></i>
                    Close
                  </button>
                </div>
                <div className="mb-2">
                  <input
                    className="tcp-input"
                    placeholder="Type to filter folders..."
                    value={incidentMonitorWorkspaceBrowserSearch}
                    onInput={(e) =>
                      setIncidentMonitorWorkspaceBrowserSearch((e.target as HTMLInputElement).value)
                    }
                  />
                </div>
                <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
                  {filteredIncidentMonitorWorkspaceDirectories.length ? (
                    filteredIncidentMonitorWorkspaceDirectories.map((entry: any) => (
                      <button
                        key={String(entry?.path || entry?.name)}
                        className="tcp-list-item mb-1 w-full text-left"
                        onClick={() =>
                          setIncidentMonitorWorkspaceBrowserDir(String(entry?.path || ""))
                        }
                      >
                        <i data-lucide="folder-open"></i>
                        {String(entry?.name || entry?.path || "")}
                      </button>
                    ))
                  ) : (
                    <EmptyState
                      text={
                        incidentMonitorWorkspaceSearchQuery
                          ? "No folders match your search."
                          : "No subdirectories in this folder."
                      }
                    />
                  )}
                </div>
              </motion.div>
            </motion.div>
          ) : null}
        </AnimatePresence>

        <DetailDrawer
          open={diagnosticsOpen}
          onClose={() => setDiagnosticsOpen(false)}
          title="Browser diagnostics"
        >
          <div className="grid gap-3">
            <div className="grid gap-2 md:grid-cols-3">
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Status</div>
                <div className="mt-1 text-sm">
                  {browserStatus.data
                    ? browserStatus.data.runnable
                      ? "Ready"
                      : browserStatus.data.enabled
                        ? "Blocked"
                        : "Disabled"
                    : "Unknown"}
                </div>
                <div className="tcp-subtle text-xs">
                  Headless default: {browserStatus.data?.headless_default ? "yes" : "no"}
                </div>
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Sidecar</div>
                <div className="mt-1 break-all text-sm">
                  {browserStatus.data?.sidecar?.path || "Not found"}
                </div>
                <div className="tcp-subtle text-xs">
                  {browserStatus.data?.sidecar?.version || "No version detected"}
                </div>
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Browser</div>
                <div className="mt-1 break-all text-sm">
                  {browserStatus.data?.browser?.path || "Not found"}
                </div>
                <div className="tcp-subtle text-xs">
                  {browserStatus.data?.browser?.version ||
                    browserStatus.data?.browser?.channel ||
                    "No version detected"}
                </div>
              </div>
            </div>

            <Toolbar>
              <button className="tcp-btn" onClick={() => void browserStatus.refetch()}>
                <i data-lucide="refresh-cw"></i>
                Refresh browser status
              </button>
              <button
                className="tcp-btn"
                onClick={() => installBrowserMutation.mutate()}
                disabled={installBrowserMutation.isPending}
              >
                <i data-lucide="download"></i>
                {installBrowserMutation.isPending ? "Installing sidecar..." : "Install sidecar"}
              </button>
              <button
                className="tcp-btn"
                onClick={() => smokeTestBrowserMutation.mutate()}
                disabled={smokeTestBrowserMutation.isPending}
              >
                <i data-lucide="globe"></i>
                {smokeTestBrowserMutation.isPending ? "Running smoke test..." : "Run smoke test"}
              </button>
              <button
                className="tcp-btn"
                onClick={() =>
                  api("/api/engine/browser/status", { method: "GET" })
                    .then(() => toast("ok", "Browser diagnostics refreshed."))
                    .catch((error) =>
                      toast("err", error instanceof Error ? error.message : String(error))
                    )
                }
              >
                <i data-lucide="activity"></i>
                Re-run diagnostics
              </button>
            </Toolbar>

            {browserStatus.isLoading ? (
              <EmptyState text="Loading browser diagnostics..." />
            ) : browserStatus.data ? (
              <>
                {browserIssues.length ? (
                  <div className="grid gap-2">
                    <div className="text-sm font-medium">Blocking issues</div>
                    {browserIssues.map((issue, index) => (
                      <div key={`${issue.code || "issue"}-${index}`} className="tcp-list-item">
                        <div className="text-sm font-medium">{issue.code || "browser_issue"}</div>
                        <div className="tcp-subtle text-xs">
                          {issue.message || "Unknown browser issue."}
                        </div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm">
                    Browser automation is ready on this machine.
                  </div>
                )}

                {browserSmokeResult ? (
                  <div className="grid gap-2">
                    <div className="text-sm font-medium">Latest smoke test</div>
                    <div className="tcp-list-item">
                      <div className="text-sm font-medium">
                        {browserSmokeResult.title || "Smoke test"}
                      </div>
                      <div className="tcp-subtle text-xs">
                        {browserSmokeResult.final_url ||
                          browserSmokeResult.url ||
                          "No URL returned"}
                      </div>
                      <div className="tcp-subtle text-xs">
                        Load state: {browserSmokeResult.load_state || "unknown"} · elements:{" "}
                        {String(browserSmokeResult.element_count ?? 0)} · closed:{" "}
                        {browserSmokeResult.closed ? "yes" : "no"}
                      </div>
                      {browserSmokeResult.excerpt ? (
                        <pre className="tcp-code mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words">
                          {browserSmokeResult.excerpt}
                        </pre>
                      ) : null}
                    </div>
                  </div>
                ) : null}

                {browserRecommendations.length ? (
                  <div className="grid gap-2">
                    <div className="text-sm font-medium">Recommendations</div>
                    {browserRecommendations.map((row, index) => (
                      <div
                        key={`browser-recommendation-${index}`}
                        className="tcp-list-item text-sm"
                      >
                        {row}
                      </div>
                    ))}
                  </div>
                ) : null}

                {browserInstallHints.length ? (
                  <div className="grid gap-2">
                    <div className="text-sm font-medium">Install hints</div>
                    {browserInstallHints.map((row, index) => (
                      <div key={`browser-install-hint-${index}`} className="tcp-list-item text-sm">
                        {row}
                      </div>
                    ))}
                  </div>
                ) : null}

                {browserStatus.data?.last_error ? (
                  <div className="tcp-subtle rounded-lg border border-slate-700/60 bg-slate-900/20 p-3 text-xs">
                    Last error: {browserStatus.data.last_error}
                  </div>
                ) : null}
              </>
            ) : (
              <EmptyState text="Browser diagnostics are unavailable." />
            )}
          </div>
        </DetailDrawer>

        <AnimatePresence>
          {mcpModalOpen ? (
            <motion.div
              className="tcp-confirm-overlay"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <button
                type="button"
                className="tcp-confirm-backdrop"
                aria-label="Close MCP server dialog"
                onClick={() => setMcpModalOpen(false)}
              />
              <motion.div
                className="tcp-confirm-dialog tcp-verification-modal"
                initial={{ opacity: 0, y: 8, scale: 0.98 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 6, scale: 0.98 }}
              >
                <div className="mb-3 flex items-start justify-between gap-3">
                  <div>
                    <h3 className="tcp-confirm-title">
                      {mcpEditingName ? "Edit MCP Server" : "Add MCP Server"}
                    </h3>
                    <p className="tcp-confirm-message">
                      Configure transport and auth without leaving Settings.
                    </p>
                  </div>
                  <button
                    type="button"
                    className="tcp-btn h-8 px-2"
                    onClick={() => setMcpModalOpen(false)}
                  >
                    <i data-lucide="x"></i>
                  </button>
                </div>

                <form
                  className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
                  onSubmit={(event) => {
                    event.preventDefault();
                    mcpSaveMutation.mutate();
                  }}
                >
                  <div className="tcp-settings-tabs">
                    <button
                      type="button"
                      className={`tcp-settings-tab tcp-settings-tab-underline ${
                        mcpModalTab === "catalog" ? "active" : ""
                      }`}
                      onClick={() => setMcpModalTab("catalog")}
                    >
                      <i data-lucide="blocks"></i>
                      Built-in packs
                    </button>
                    <button
                      type="button"
                      className={`tcp-settings-tab tcp-settings-tab-underline ${
                        mcpModalTab === "manual" ? "active" : ""
                      }`}
                      onClick={() => setMcpModalTab("manual")}
                    >
                      <i data-lucide="square-pen"></i>
                      Manual
                    </button>
                  </div>

                  {mcpModalTab === "catalog" ? (
                    <div className="grid min-h-0 flex-1 content-start gap-3 overflow-hidden">
                      <div className="flex items-center justify-between gap-3">
                        <div className="tcp-subtle text-sm">
                          {mcpCatalog.generatedAt
                            ? `Built-in MCP packs · generated ${mcpCatalog.generatedAt}`
                            : "Built-in MCP packs"}
                        </div>
                        <button
                          type="button"
                          className="tcp-btn h-8 px-3 text-xs"
                          onClick={() => void mcpCatalogQuery.refetch()}
                        >
                          <i data-lucide="refresh-cw"></i>
                          Refresh
                        </button>
                      </div>
                      <input
                        className="tcp-input"
                        value={mcpCatalogSearch}
                        onInput={(event) =>
                          setMcpCatalogSearch((event.target as HTMLInputElement).value)
                        }
                        placeholder="Search built-in MCP packs"
                      />
                      <div className="grid min-h-0 flex-1 auto-rows-max content-start gap-2 overflow-y-auto pr-1 md:grid-cols-2">
                        {filteredMcpCatalog.length ? (
                          filteredMcpCatalog.map((row) => {
                            const alreadyConfigured = configuredMcpServerNames.has(
                              String(row.serverConfigName || row.slug || "").toLowerCase()
                            );
                            return (
                              <div
                                key={row.slug}
                                className="tcp-list-item grid h-full min-h-[8.5rem] content-start gap-2"
                              >
                                <div className="flex flex-wrap items-start justify-between gap-2">
                                  <div>
                                    <div className="font-semibold">{row.name}</div>
                                    <div className="tcp-subtle text-xs">
                                      {row.slug}
                                      {row.requiresSetup ? " · setup required" : ""}
                                    </div>
                                  </div>
                                  <div className="flex flex-wrap gap-2">
                                    <Badge tone="info">{row.toolCount} tools</Badge>
                                    <Badge tone={row.requiresAuth ? "warn" : "ok"}>
                                      {row.requiresAuth ? "Auth" : "Authless"}
                                    </Badge>
                                  </div>
                                </div>
                                <div className="tcp-subtle line-clamp-2 text-xs">
                                  {row.description || row.transportUrl}
                                </div>
                                <div className="tcp-subtle break-all text-xs">
                                  {row.transportUrl}
                                </div>
                                <div className="mt-auto flex flex-wrap gap-2">
                                  <button
                                    type="button"
                                    className="tcp-btn h-8 px-3 text-xs"
                                    onClick={() => {
                                      const nextTransport = row.transportUrl;
                                      const nextName = normalizeMcpName(
                                        row.serverConfigName || row.slug || row.name
                                      );
                                      setMcpName(nextName);
                                      setMcpTransport(nextTransport);
                                      setMcpAuthMode(
                                        nextName === "github" ||
                                          isGithubCopilotMcpTransport(nextTransport)
                                          ? "bearer"
                                          : "none"
                                      );
                                      setMcpGithubToolsets(
                                        nextName === "github" ||
                                          isGithubCopilotMcpTransport(nextTransport)
                                          ? "default"
                                          : ""
                                      );
                                      setMcpExtraHeaders([]);
                                      setMcpModalTab("manual");
                                      toast(
                                        "ok",
                                        `Loaded ${row.name}. Review and save when ready.`
                                      );
                                    }}
                                  >
                                    Use pack
                                  </button>
                                  {row.documentationUrl ? (
                                    <a
                                      className="tcp-btn h-8 px-3 text-xs"
                                      href={row.documentationUrl}
                                      target="_blank"
                                      rel="noreferrer"
                                    >
                                      <i data-lucide="external-link"></i>
                                      Docs
                                    </a>
                                  ) : null}
                                  {alreadyConfigured ? <Badge tone="ok">added</Badge> : null}
                                </div>
                              </div>
                            );
                          })
                        ) : (
                          <EmptyState text="No built-in MCP packs match this search." />
                        )}
                      </div>
                    </div>
                  ) : (
                    <>
                      <div className="grid gap-3 md:grid-cols-2">
                        <div className="grid gap-2">
                          <label className="text-sm font-medium">Name</label>
                          <input
                            className="tcp-input"
                            value={mcpName}
                            onInput={(event) =>
                              setMcpName((event.target as HTMLInputElement).value)
                            }
                            placeholder="mcp-server"
                          />
                        </div>
                        <div className="grid gap-2">
                          <label className="text-sm font-medium">Auth mode</label>
                          <select
                            className="tcp-select"
                            value={mcpAuthMode}
                            onChange={(event) =>
                              setMcpAuthMode((event.target as HTMLSelectElement).value)
                            }
                          >
                            <option value="none">No Auth Header</option>
                            <option value="auto">Auto</option>
                            <option value="x-api-key">x-api-key</option>
                            <option value="bearer">Authorization Bearer</option>
                            <option value="custom">Custom Header</option>
                          </select>
                        </div>
                      </div>

                      <div className="grid gap-2">
                        <label className="text-sm font-medium">Transport URL</label>
                        <input
                          className="tcp-input"
                          value={mcpTransport}
                          onInput={(event) => {
                            const value = (event.target as HTMLInputElement).value;
                            setMcpTransport(value);
                            if (
                              isGithubCopilotMcpTransport(value) &&
                              !String(mcpGithubToolsets || "").trim()
                            ) {
                              setMcpGithubToolsets("default");
                            }
                            if (!String(mcpName || "").trim() || mcpName === "mcp-server") {
                              const inferred = inferMcpNameFromTransport(value);
                              if (inferred) setMcpName(inferred);
                            }
                          }}
                          placeholder="https://example.com/mcp"
                        />
                      </div>

                      {mcpAuthMode === "custom" ? (
                        <div className="grid gap-2">
                          <label className="text-sm font-medium">Custom header name</label>
                          <input
                            className="tcp-input"
                            value={mcpCustomHeader}
                            onInput={(event) =>
                              setMcpCustomHeader((event.target as HTMLInputElement).value)
                            }
                            placeholder="X-My-Token"
                          />
                        </div>
                      ) : null}

                      <div className="grid gap-2">
                        <label className="text-sm font-medium">Token</label>
                        <input
                          className="tcp-input"
                          type="password"
                          value={mcpToken}
                          onInput={(event) => setMcpToken((event.target as HTMLInputElement).value)}
                          placeholder="token"
                        />
                        <div className="tcp-subtle text-xs">{mcpAuthPreviewText}</div>
                      </div>

                      {mcpIsGithubTransport ? (
                        <div className="grid gap-2">
                          <label className="text-sm font-medium">GitHub toolsets</label>
                          <input
                            className="tcp-input"
                            value={mcpGithubToolsets}
                            onInput={(event) =>
                              setMcpGithubToolsets((event.target as HTMLInputElement).value)
                            }
                            placeholder="default,projects"
                          />
                          <div className="tcp-subtle text-xs">
                            Sent as `X-MCP-Toolsets`. Built-in GitHub starts with `default`; add
                            values like `projects`, `issues`, or `pull_requests`.
                          </div>
                        </div>
                      ) : null}

                      <div className="grid gap-2">
                        <div className="flex items-center justify-between gap-2">
                          <label className="text-sm font-medium">Additional headers</label>
                          <button
                            type="button"
                            className="tcp-btn h-8 px-3 text-xs"
                            onClick={() =>
                              setMcpExtraHeaders((prev) => [...prev, { key: "", value: "" }])
                            }
                          >
                            <i data-lucide="plus"></i>
                            Add header
                          </button>
                        </div>
                        {mcpExtraHeaders.length ? (
                          <div className="grid gap-2">
                            {mcpExtraHeaders.map((row, index) => (
                              <div
                                key={`mcp-header-${index}`}
                                className="grid gap-2 md:grid-cols-[1fr_1fr_auto]"
                              >
                                <input
                                  className="tcp-input"
                                  value={row.key}
                                  onInput={(event) =>
                                    setMcpExtraHeaders((prev) =>
                                      prev.map((entry, entryIndex) =>
                                        entryIndex === index
                                          ? {
                                              ...entry,
                                              key: (event.target as HTMLInputElement).value,
                                            }
                                          : entry
                                      )
                                    )
                                  }
                                  placeholder="Header name"
                                />
                                <input
                                  className="tcp-input"
                                  value={row.value}
                                  onInput={(event) =>
                                    setMcpExtraHeaders((prev) =>
                                      prev.map((entry, entryIndex) =>
                                        entryIndex === index
                                          ? {
                                              ...entry,
                                              value: (event.target as HTMLInputElement).value,
                                            }
                                          : entry
                                      )
                                    )
                                  }
                                  placeholder="Header value"
                                />
                                <button
                                  type="button"
                                  className="tcp-btn"
                                  onClick={() =>
                                    setMcpExtraHeaders((prev) =>
                                      prev.filter((_, entryIndex) => entryIndex !== index)
                                    )
                                  }
                                >
                                  Remove
                                </button>
                              </div>
                            ))}
                          </div>
                        ) : (
                          <div className="tcp-subtle text-xs">
                            Add arbitrary request headers such as `X-MCP-Insiders` or vendor feature
                            flags.
                          </div>
                        )}
                      </div>

                      <label className="inline-flex items-center gap-2 text-sm text-slate-200">
                        <input
                          type="checkbox"
                          className="accent-slate-400"
                          checked={mcpConnectAfterAdd}
                          onChange={(event) =>
                            setMcpConnectAfterAdd((event.target as HTMLInputElement).checked)
                          }
                        />
                        Connect after save
                      </label>
                    </>
                  )}

                  <div className="tcp-confirm-actions mt-2">
                    <button
                      type="button"
                      className="tcp-btn"
                      onClick={() => setMcpModalOpen(false)}
                    >
                      Cancel
                    </button>
                    <button
                      type="submit"
                      className="tcp-btn-primary"
                      disabled={mcpSaveMutation.isPending}
                    >
                      <i data-lucide="save"></i>
                      Save MCP server
                    </button>
                  </div>
                </form>
              </motion.div>
            </motion.div>
          ) : null}
        </AnimatePresence>
      </div>
    </AnimatedPage>
  );
}
