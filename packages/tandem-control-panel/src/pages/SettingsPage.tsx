import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { JsonObject } from "@frumu/tandem-client";
import { renderIcons } from "../app/icons.js";
import { renderMarkdownSafe } from "../lib/markdown";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
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
import { APP_NAV_ROUTES } from "../app/routes";
import { providerHints } from "../app/store.js";
import { ACA_CORE_NAV_ROUTE_IDS, getDefaultNavigationVisibility } from "../app/navigation";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";
import type { RouteId } from "../app/routes";
import { buildPlannerProviderOptions } from "../features/planner/plannerShared";

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
  | "install"
  | "navigation"
  | "providers"
  | "search"
  | "scheduler"
  | "identity"
  | "theme"
  | "channels"
  | "mcp"
  | "bug_monitor"
  | "browser";

type SearchSettingsResponse = {
  available?: boolean;
  local_engine?: boolean;
  writable?: boolean;
  managed_env_path?: string | null;
  restart_required?: boolean;
  restart_hint?: string | null;
  reason?: string | null;
  settings?: {
    backend?: string;
    tandem_url?: string | null;
    searxng_url?: string | null;
    timeout_ms?: number | null;
    has_brave_key?: boolean;
    has_exa_key?: boolean;
  } | null;
};

type SchedulerSettingsResponse = {
  available?: boolean;
  local_engine?: boolean;
  writable?: boolean;
  managed_env_path?: string | null;
  restart_required?: boolean;
  restart_hint?: string | null;
  reason?: string | null;
  settings?: {
    mode?: string;
    max_concurrent_runs?: number | null;
  } | null;
};

type SearchTestResponse = {
  ok?: boolean;
  query?: string;
  markdown?: string;
  output?: string;
  parsed_output?: {
    query?: string;
    backend?: string;
    configured_backend?: string;
    attempted_backends?: string[];
    result_count?: number;
    partial?: boolean;
    results?: Array<{
      title?: string;
      url?: string;
      snippet?: string;
      source?: string;
    }>;
  } | null;
  metadata?: {
    backend?: string;
    configured_backend?: string;
    attempted_backends?: string[];
    count?: number;
    error?: string;
  } | null;
};

type BugMonitorConfigRow = {
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

type BugMonitorStatusRow = {
  config?: BugMonitorConfigRow;
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

type BugMonitorIncidentRow = {
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

type BugMonitorDraftRow = {
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

type BugMonitorPostRow = {
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
  model_provider_id?: string;
  model_id?: string;
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
  modelProviderId: string;
  modelId: string;
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
  "openai-codex",
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

const OPENAI_CODEX_PROVIDER_ID = "openai-codex";

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
    modelProviderId: String(row.model_provider_id || "").trim(),
    modelId: String(row.model_id || "").trim(),
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

function normalizeChannelAllowedUsers(input: string[] | string | null | undefined) {
  const rawValues = Array.isArray(input) ? input : String(input || "").split(",");
  const users = rawValues.map((row) => String(row || "").trim()).filter(Boolean);
  return users.length ? Array.from(new Set(users)) : ["*"];
}

function sameChannelAllowedUsers(
  left: string[] | string | null | undefined,
  right: string[] | string | null | undefined
) {
  const a = normalizeChannelAllowedUsers(left).slice().sort();
  const b = normalizeChannelAllowedUsers(right).slice().sort();
  if (a.length !== b.length) return false;
  return a.every((value, index) => value === b[index]);
}

function channelConfigHasSavedSettings(
  channel: string,
  config: ChannelConfigRow | null | undefined
) {
  const row = config && typeof config === "object" ? config : {};
  const allowedUsers = normalizeChannelAllowedUsers(row.allowed_users);
  return (
    !!row.has_token ||
    allowedUsers.some((user) => user !== "*") ||
    !!row.mention_only ||
    !!String(row.guild_id || "").trim() ||
    !!String(row.channel_id || "").trim() ||
    !!String(row.model_provider_id || "").trim() ||
    !!String(row.model_id || "").trim() ||
    String(row.style_profile || "default").trim() !== "default" ||
    String(row.security_profile || "operator").trim() !== "operator"
  );
}

function channelDraftMatchesConfig(
  channel: string,
  draft: ChannelDraft,
  config: ChannelConfigRow | null | undefined
) {
  const savedDraft = normalizeChannelDraft(channel, config);
  return (
    !String(draft.botToken || "").trim() &&
    sameChannelAllowedUsers(draft.allowedUsers, savedDraft.allowedUsers) &&
    !!draft.mentionOnly === !!savedDraft.mentionOnly &&
    String(draft.guildId || "").trim() === String(savedDraft.guildId || "").trim() &&
    String(draft.channelId || "").trim() === String(savedDraft.channelId || "").trim() &&
    String(draft.modelProviderId || "").trim() ===
      String(savedDraft.modelProviderId || "").trim() &&
    String(draft.modelId || "").trim() === String(savedDraft.modelId || "").trim() &&
    String(draft.styleProfile || "default").trim() ===
      String(savedDraft.styleProfile || "default").trim() &&
    String(draft.securityProfile || "operator").trim() ===
      String(savedDraft.securityProfile || "operator").trim()
  );
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

const NAV_ROUTE_DESCRIPTIONS: Record<string, string> = {
  dashboard: "Command status, activity, and fast paths.",
  chat: "Session-driven conversation, uploads, and live responses.",
  planner: "Advanced long-horizon planning and governed handoff.",
  studio: "Advanced template-first workflow builder.",
  automations: "Reusable routines, approvals, and execution history.",
  experiments: "Opt-in automation experiments, optimization campaigns, and team approvals.",
  coding: "ACA intake, task launchers, and coding runs.",
  agents: "Reusable agent roles and workflow drafts.",
  orchestrator: "Task board planning, approvals, and execution.",
  memory: "Searchable memory records and operational context.",
  runs: "Live operations overview with queue state and per-run inspection.",
  settings: "Provider defaults, themes, and runtime diagnostics.",
};

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
  navigation,
}: AppPageProps) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [modelSearchByProvider, setModelSearchByProvider] = useState<Record<string, string>>({});
  const [botName, setBotName] = useState(String(identity?.botName || "Tandem"));
  const [botAvatarUrl, setBotAvatarUrl] = useState(String(identity?.botAvatarUrl || ""));
  const [botControlPanelAlias, setBotControlPanelAlias] = useState("Control Center");
  const [activeSection, setActiveSection] = useState<SettingsSection>(
    navigation?.acaMode ? "navigation" : "install"
  );
  const [installConfigText, setInstallConfigText] = useState("");
  const [installConfigError, setInstallConfigError] = useState("");
  const [searchBackend, setSearchBackend] = useState("auto");
  const [searchTandemUrl, setSearchTandemUrl] = useState("");
  const [searchSearxngUrl, setSearchSearxngUrl] = useState("");
  const [searchTimeoutMs, setSearchTimeoutMs] = useState("10000");
  const [searchBraveKey, setSearchBraveKey] = useState("");
  const [searchExaKey, setSearchExaKey] = useState("");
  const [searchTestQuery, setSearchTestQuery] = useState("autonomous AI agentic workflows");
  const [searchTestResult, setSearchTestResult] = useState<SearchTestResponse | null>(null);
  const [schedulerMode, setSchedulerMode] = useState("multi");
  const [schedulerMaxConcurrent, setSchedulerMaxConcurrent] = useState("");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [githubMcpGuideOpen, setGithubMcpGuideOpen] = useState(false);
  const [providerDefaultsOpen, setProviderDefaultsOpen] = useState(false);
  const [customProviderFormOpen, setCustomProviderFormOpen] = useState(false);
  const [oauthSessionByProvider, setOauthSessionByProvider] = useState<Record<string, string>>({});
  const [customProviderId, setCustomProviderId] = useState("custom");
  const [customProviderUrl, setCustomProviderUrl] = useState("");
  const [customProviderModel, setCustomProviderModel] = useState("");
  const [customProviderApiKey, setCustomProviderApiKey] = useState("");
  const [customProviderMakeDefault, setCustomProviderMakeDefault] = useState(true);
  const [channelDrafts, setChannelDrafts] = useState<Record<string, ChannelDraft>>({});
  const channelDraftsHydratedRef = useRef<Record<string, boolean>>({
    telegram: false,
    discord: false,
    slack: false,
  });
  const [channelToolScopeOpen, setChannelToolScopeOpen] = useState<Record<string, boolean>>({
    telegram: false,
    discord: false,
    slack: false,
  });
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
  const [bugMonitorEnabled, setBugMonitorEnabled] = useState(false);
  const [bugMonitorPaused, setBugMonitorPaused] = useState(false);
  const [bugMonitorWorkspaceRoot, setBugMonitorWorkspaceRoot] = useState("");
  const [bugMonitorRepo, setBugMonitorRepo] = useState("");
  const [bugMonitorMcpServer, setBugMonitorMcpServer] = useState("");
  const [bugMonitorProviderPreference, setBugMonitorProviderPreference] = useState("auto");
  const [bugMonitorProviderId, setBugMonitorProviderId] = useState("");
  const [bugMonitorModelId, setBugMonitorModelId] = useState("");
  const [bugMonitorAutoCreateIssues, setBugMonitorAutoCreateIssues] = useState(true);
  const [bugMonitorRequireApproval, setBugMonitorRequireApproval] = useState(false);
  const [bugMonitorAutoComment, setBugMonitorAutoComment] = useState(true);
  const [bugMonitorWorkspaceBrowserOpen, setBugMonitorWorkspaceBrowserOpen] = useState(false);
  const [bugMonitorWorkspaceBrowserDir, setBugMonitorWorkspaceBrowserDir] = useState("");
  const [bugMonitorWorkspaceBrowserSearch, setBugMonitorWorkspaceBrowserSearch] = useState("");
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
    if (currentRoute === "bug-monitor") setActiveSection("bug_monitor");
  }, [currentRoute]);

  const installProfileQuery = useQuery({
    queryKey: ["settings", "install", "profile"],
    queryFn: () => api("/api/install/profile", { method: "GET" }).catch(() => null),
    refetchInterval: 30_000,
  });
  const installConfigQuery = useQuery({
    queryKey: ["settings", "install", "config"],
    queryFn: () => api("/api/control-panel/config", { method: "GET" }).catch(() => null),
  });
  useEffect(() => {
    const payload = (installConfigQuery.data as any)?.config || null;
    if (!payload) return;
    try {
      setInstallConfigText(JSON.stringify(payload, null, 2));
      setInstallConfigError("");
    } catch {
      setInstallConfigError("Loaded config could not be rendered as JSON.");
    }
  }, [installConfigQuery.data]);

  useEffect(() => {
    if (currentRoute !== "settings") return;
    if (!navigation?.acaMode) {
      setActiveSection("install");
      return;
    }
    const preferInstall =
      installProfileQuery.data?.aca_integration !== true ||
      installProfileQuery.data?.control_panel_config_ready !== true;
    setActiveSection(preferInstall ? "install" : "navigation");
  }, [
    currentRoute,
    navigation?.acaMode,
    installProfileQuery.data?.aca_integration,
    installProfileQuery.data?.control_panel_config_ready,
  ]);

  const providersCatalog = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [], connected: [] })),
  });

  const providersConfig = useQuery({
    queryKey: ["settings", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({ default: "", providers: {} })),
  });
  const providersAuth = useQuery({
    queryKey: ["settings", "providers", "auth"],
    queryFn: () => client.providers.authStatus().catch(() => ({ providers: {} })),
    refetchInterval: 15_000,
  });
  const systemHealthQuery = useQuery({
    queryKey: ["settings", "system", "health"],
    queryFn: () => api("/api/system/health", { method: "GET" }).catch(() => null),
    refetchInterval: 30_000,
  });
  const channelProviderOptions = useMemo(
    () =>
      buildPlannerProviderOptions({
        providerCatalog: providersCatalog.data,
        providerConfig: providersConfig.data,
        defaultProvider: String(providersConfig.data?.default || "").trim(),
        defaultModel: String(
          providersConfig.data?.providers?.[String(providersConfig.data?.default || "").trim()]
            ?.default_model ||
            providersConfig.data?.providers?.[String(providersConfig.data?.default || "").trim()]
              ?.defaultModel ||
            ""
        ).trim(),
      }),
    [providersCatalog.data, providersConfig.data]
  );
  const providerAuthById = useMemo(() => {
    const nested = (providersAuth.data as Record<string, any> | undefined)?.providers;
    if (nested && typeof nested === "object") return nested as Record<string, any>;
    if (providersAuth.data && typeof providersAuth.data === "object") {
      return providersAuth.data as Record<string, any>;
    }
    return {} as Record<string, any>;
  }, [providersAuth.data]);
  const localEngine = systemHealthQuery.data?.localEngine === true;
  const channelDefaultModel = useMemo(() => {
    const defaultProvider = String(providersConfig.data?.default || "").trim();
    const defaultModel = String(
      providersConfig.data?.providers?.[defaultProvider]?.default_model ||
        providersConfig.data?.providers?.[defaultProvider]?.defaultModel ||
        ""
    ).trim();
    return { provider: defaultProvider, model: defaultModel };
  }, [providersConfig.data]);
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
  const searchSettingsQuery = useQuery<SearchSettingsResponse | null>({
    queryKey: ["settings", "search", "config"],
    queryFn: () => api("/api/system/search-settings", { method: "GET" }).catch(() => null),
  });
  const schedulerSettingsQuery = useQuery<SchedulerSettingsResponse | null>({
    queryKey: ["settings", "scheduler", "config"],
    queryFn: () => api("/api/system/scheduler-settings", { method: "GET" }).catch(() => null),
  });

  useEffect(() => {
    const settings = searchSettingsQuery.data?.settings || null;
    if (!settings) return;
    setSearchBackend(String(settings.backend || "auto").trim() || "auto");
    setSearchTandemUrl(String(settings.tandem_url || "").trim());
    setSearchSearxngUrl(String(settings.searxng_url || "").trim());
    setSearchTimeoutMs(String(settings.timeout_ms || 10000));
  }, [searchSettingsQuery.data]);

  useEffect(() => {
    const settings = schedulerSettingsQuery.data?.settings || null;
    if (!settings) return;
    setSchedulerMode(String(settings.mode || "multi").trim() || "multi");
    setSchedulerMaxConcurrent(
      settings.max_concurrent_runs != null ? String(settings.max_concurrent_runs) : ""
    );
  }, [schedulerSettingsQuery.data]);

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
  const bugMonitorConfigQuery = useQuery({
    queryKey: ["settings", "bug-monitor", "config"],
    queryFn: () =>
      api("/api/engine/config/bug-monitor", { method: "GET" }).catch(() => ({
        bug_monitor: {},
      })),
  });
  const bugMonitorStatusQuery = useQuery({
    queryKey: ["settings", "bug-monitor", "status"],
    queryFn: () =>
      api("/api/engine/bug-monitor/status", { method: "GET" }).catch(() => ({
        status: {},
      })),
    refetchInterval: 10_000,
  });
  const bugMonitorDraftsQuery = useQuery({
    queryKey: ["settings", "bug-monitor", "drafts"],
    queryFn: () =>
      api("/api/engine/bug-monitor/drafts?limit=10", { method: "GET" }).catch(() => ({
        drafts: [],
      })),
    refetchInterval: 15_000,
  });
  const bugMonitorIncidentsQuery = useQuery({
    queryKey: ["settings", "bug-monitor", "incidents"],
    queryFn: () =>
      api("/api/engine/bug-monitor/incidents?limit=10", { method: "GET" }).catch(() => ({
        incidents: [],
      })),
    refetchInterval: 10_000,
  });
  const bugMonitorPostsQuery = useQuery({
    queryKey: ["settings", "bug-monitor", "posts"],
    queryFn: () =>
      api("/api/engine/bug-monitor/posts?limit=10", { method: "GET" }).catch(() => ({
        posts: [],
      })),
    refetchInterval: 15_000,
  });
  const bugMonitorWorkspaceBrowserQuery = useQuery({
    queryKey: ["settings", "bug-monitor", "workspace-browser", bugMonitorWorkspaceBrowserDir],
    enabled: bugMonitorWorkspaceBrowserOpen && !!bugMonitorWorkspaceBrowserDir,
    queryFn: () =>
      api(
        `/api/orchestrator/workspaces/list?dir=${encodeURIComponent(
          bugMonitorWorkspaceBrowserDir
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
  const getCodexDefaultModelId = () =>
    String(
      providersConfig.data?.providers?.[OPENAI_CODEX_PROVIDER_ID]?.default_model ||
        providersConfig.data?.providers?.[OPENAI_CODEX_PROVIDER_ID]?.defaultModel ||
        "gpt-5.4"
    ).trim() || "gpt-5.4";
  const promoteCodexAsDefaultProvider = async () => {
    await client.providers.setDefaults(OPENAI_CODEX_PROVIDER_ID, getCodexDefaultModelId());
  };
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
  const saveBugMonitorMutation = useMutation({
    mutationFn: async () =>
      api("/api/engine/config/bug-monitor", {
        method: "PATCH",
        body: JSON.stringify({
          bug_monitor: {
            enabled: bugMonitorEnabled,
            paused: bugMonitorPaused,
            workspace_root: String(bugMonitorWorkspaceRoot || "").trim() || null,
            repo: String(bugMonitorRepo || "").trim() || null,
            mcp_server: String(bugMonitorMcpServer || "").trim() || null,
            provider_preference: String(bugMonitorProviderPreference || "auto").trim(),
            model_policy:
              bugMonitorProviderId && bugMonitorModelId
                ? {
                    default_model: {
                      provider_id: bugMonitorProviderId,
                      model_id: bugMonitorModelId,
                    },
                  }
                : null,
            auto_create_new_issues: bugMonitorAutoCreateIssues,
            require_approval_for_new_issues: bugMonitorRequireApproval,
            auto_comment_on_matched_open_issues: bugMonitorAutoComment,
            label_mode: "reporter_only",
          },
        }),
      }),
    onSuccess: async () => {
      toast("ok", "Bug Monitor settings saved.");
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["settings", "bug-monitor"] })]);
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const refreshBugMonitorBindingsMutation = useMutation({
    mutationFn: async () =>
      api("/api/engine/capabilities/bindings/refresh-builtins", {
        method: "POST",
      }),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "bug-monitor"] }),
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
  const bugMonitorDraftDecisionMutation = useMutation({
    mutationFn: async ({ draftId, decision }: { draftId: string; decision: "approve" | "deny" }) =>
      api(`/api/engine/bug-monitor/drafts/${encodeURIComponent(draftId)}/${decision}`, {
        method: "POST",
        body: JSON.stringify({
          reason: `${decision}d from control panel settings`,
        }),
      }),
    onSuccess: async (_payload, vars) => {
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["settings", "bug-monitor"] })]);
      toast("ok", `Bug Monitor draft ${vars.decision === "approve" ? "approved" : "denied"}.`);
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const bugMonitorTriageRunMutation = useMutation({
    mutationFn: async ({ draftId }: { draftId: string }) =>
      api(`/api/engine/bug-monitor/drafts/${encodeURIComponent(draftId)}/triage-run`, {
        method: "POST",
      }),
    onSuccess: async (payload: any) => {
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["settings", "bug-monitor"] })]);
      toast(
        "ok",
        payload?.deduped
          ? `Bug Monitor triage run already exists: ${payload?.run?.run_id || "unknown"}`
          : `Bug Monitor triage run created: ${payload?.run?.run_id || "unknown"}`
      );
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const bugMonitorPauseResumeMutation = useMutation({
    mutationFn: async ({ action }: { action: "pause" | "resume" }) =>
      api(`/api/engine/bug-monitor/${action}`, {
        method: "POST",
      }),
    onSuccess: async (_payload, vars) => {
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["settings", "bug-monitor"] })]);
      toast("ok", `Bug Monitor ${vars.action === "pause" ? "paused" : "resumed"}.`);
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const bugMonitorReplayIncidentMutation = useMutation({
    mutationFn: async ({ incidentId }: { incidentId: string }) =>
      api(`/api/engine/bug-monitor/incidents/${encodeURIComponent(incidentId)}/replay`, {
        method: "POST",
      }),
    onSuccess: async (payload: any) => {
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["settings", "bug-monitor"] })]);
      toast(
        "ok",
        payload?.deduped
          ? `Bug Monitor triage run already exists: ${payload?.run?.run_id || "unknown"}`
          : `Bug Monitor replay queued triage: ${payload?.run?.run_id || "unknown"}`
      );
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const bugMonitorPublishDraftMutation = useMutation({
    mutationFn: async ({ draftId }: { draftId: string }) =>
      api(`/api/engine/bug-monitor/drafts/${encodeURIComponent(draftId)}/publish`, {
        method: "POST",
      }),
    onSuccess: async (payload: any) => {
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["settings", "bug-monitor"] })]);
      toast(
        "ok",
        payload?.action === "comment_issue"
          ? `Bug Monitor commented on issue #${payload?.draft?.issue_number || "unknown"}.`
          : `Bug Monitor published issue #${payload?.draft?.issue_number || "unknown"}.`
      );
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const bugMonitorRecheckMatchMutation = useMutation({
    mutationFn: async ({ draftId }: { draftId: string }) =>
      api(`/api/engine/bug-monitor/drafts/${encodeURIComponent(draftId)}/recheck-match`, {
        method: "POST",
      }),
    onSuccess: async (payload: any) => {
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["settings", "bug-monitor"] })]);
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
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "providers"] }),
        refreshProviderStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const authorizeProviderOAuthMutation = useMutation({
    mutationFn: ({ providerId }: { providerId: string }) =>
      client.providers.oauthAuthorize(providerId),
    onSuccess: async (payload: any, vars) => {
      const providerId = String(vars.providerId || "")
        .trim()
        .toLowerCase();
      const ok = payload?.ok !== false;
      const error = String(payload?.error || payload?.message || "").trim();
      const sessionId = String(payload?.session_id || payload?.sessionId || "").trim();
      const authorizationUrl = String(
        payload?.authorization_url || payload?.authorizationUrl || payload?.url || ""
      ).trim();
      if (!ok || !providerId || !sessionId || !authorizationUrl) {
        toast("err", error || "OAuth authorize response was incomplete.");
        return;
      }
      setOauthSessionByProvider((current) => ({ ...current, [providerId]: sessionId }));
      window.open(authorizationUrl, "_blank", "noopener,noreferrer");
      toast(
        "info",
        providerId === OPENAI_CODEX_PROVIDER_ID
          ? "Browser sign-in opened for Codex. Finish the flow there, then return to Tandem."
          : `Browser sign-in opened for ${providerId}.`
      );
      await queryClient.invalidateQueries({ queryKey: ["settings", "providers", "auth"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const disconnectProviderOAuthMutation = useMutation({
    mutationFn: ({ providerId }: { providerId: string }) =>
      client.providers.oauthDisconnect(providerId),
    onSuccess: async (_payload, vars) => {
      const providerId = String(vars.providerId || "")
        .trim()
        .toLowerCase();
      setOauthSessionByProvider((current) => {
        if (!current[providerId]) return current;
        const next = { ...current };
        delete next[providerId];
        return next;
      });
      toast(
        "ok",
        providerId === OPENAI_CODEX_PROVIDER_ID
          ? "Codex account disconnected."
          : `${providerId} disconnected.`
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "providers"] }),
        refreshProviderStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const useLocalCodexSessionMutation = useMutation({
    mutationFn: ({ providerId }: { providerId: string }) =>
      client.providers.oauthUseLocalSession(providerId),
    onSuccess: async (payload: any, vars) => {
      const providerId = String(vars.providerId || "")
        .trim()
        .toLowerCase();
      const ok = payload?.ok !== false;
      const error = String(payload?.error || payload?.message || "").trim();
      if (!ok) {
        toast(
          "err",
          error ||
            "Unable to import the local Codex session. Make sure Codex CLI is signed in on this machine."
        );
        return;
      }
      setOauthSessionByProvider((current) => {
        if (!current[providerId]) return current;
        const next = { ...current };
        delete next[providerId];
        return next;
      });
      const email = String(payload?.email || "").trim();
      toast(
        "ok",
        providerId === OPENAI_CODEX_PROVIDER_ID
          ? email
            ? `Local Codex session imported: ${email}`
            : "Local Codex session imported."
          : `${providerId} local session imported.`
      );
      if (providerId === OPENAI_CODEX_PROVIDER_ID) {
        try {
          await promoteCodexAsDefaultProvider();
          toast("ok", "Codex is now the default provider for Tandem runs.");
        } catch (error) {
          toast(
            "warn",
            error instanceof Error
              ? error.message
              : "Codex session imported, but Tandem could not switch the default provider."
          );
        }
      }
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "providers"] }),
        refreshProviderStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  useEffect(() => {
    const entries = Object.entries(oauthSessionByProvider).filter(
      ([providerId, sessionId]) => !!providerId && !!sessionId
    );
    if (!entries.length) return;

    let cancelled = false;
    const finished = new Set<string>();

    const poll = async () => {
      const results = await Promise.all(
        entries.map(async ([providerId, sessionId]) => {
          try {
            const payload = await client.providers.oauthStatus(providerId, sessionId);
            return { providerId, sessionId, payload, error: null as Error | null };
          } catch (error) {
            return {
              providerId,
              sessionId,
              payload: null,
              error: error instanceof Error ? error : new Error(String(error)),
            };
          }
        })
      );

      for (const result of results) {
        if (cancelled || finished.has(result.providerId)) continue;
        if (result.error) {
          finished.add(result.providerId);
          setOauthSessionByProvider((current) => {
            if (current[result.providerId] !== result.sessionId) return current;
            const next = { ...current };
            delete next[result.providerId];
            return next;
          });
          toast("err", result.error.message);
          continue;
        }

        const payload = result.payload as Record<string, any> | null;
        const status = String(payload?.status || "")
          .trim()
          .toLowerCase();
        if (!status || status === "pending") continue;

        finished.add(result.providerId);
        setOauthSessionByProvider((current) => {
          if (current[result.providerId] !== result.sessionId) return current;
          const next = { ...current };
          delete next[result.providerId];
          return next;
        });

        if (status === "connected") {
          const email = String(payload?.email || "").trim();
          toast("ok", email ? `Codex account connected: ${email}` : "Codex account connected.");
          if (result.providerId === OPENAI_CODEX_PROVIDER_ID) {
            try {
              await promoteCodexAsDefaultProvider();
              toast("ok", "Codex is now the default provider for Tandem runs.");
            } catch (error) {
              toast(
                "warn",
                error instanceof Error
                  ? error.message
                  : "Codex connected, but Tandem could not switch the default provider."
              );
            }
          }
        } else if (status === "expired") {
          toast("warn", "Codex sign-in expired. Start the connection again.");
        } else {
          const detail = String(payload?.error || payload?.message || "").trim();
          toast("err", detail || "Codex sign-in did not complete. Please try again.");
        }

        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ["settings", "providers"] }),
          refreshProviderStatus(),
        ]);
      }
    };

    void poll();
    const timer = window.setInterval(() => {
      void poll();
    }, 2000);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [client, oauthSessionByProvider, queryClient, refreshProviderStatus, toast]);
  const saveSearchSettingsMutation = useMutation({
    mutationFn: async (
      payload: Partial<{
        backend: string;
        tandem_url: string;
        searxng_url: string;
        timeout_ms: number;
        brave_api_key: string;
        exa_api_key: string;
        clear_brave_key: boolean;
        clear_exa_key: boolean;
      }>
    ) =>
      api("/api/system/search-settings", {
        method: "PATCH",
        body: JSON.stringify(payload),
      }),
    onSuccess: async (result: SearchSettingsResponse) => {
      setSearchBraveKey("");
      setSearchExaKey("");
      await queryClient.invalidateQueries({ queryKey: ["settings", "search"] });
      toast(
        "ok",
        result?.restart_required
          ? "Search settings saved. Restart tandem-engine to apply them."
          : "Search settings saved."
      );
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const saveSchedulerSettingsMutation = useMutation({
    mutationFn: async (payload: Partial<{ mode: string; max_concurrent_runs: number | null }>) =>
      api("/api/system/scheduler-settings", {
        method: "PATCH",
        body: JSON.stringify(payload),
      }),
    onSuccess: async (result: SchedulerSettingsResponse) => {
      await queryClient.invalidateQueries({ queryKey: ["settings", "scheduler"] });
      toast(
        "ok",
        result?.restart_required
          ? "Scheduler settings saved. Restart tandem-engine to apply them."
          : "Scheduler settings saved."
      );
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const testSearchMutation = useMutation({
    mutationFn: async ({ query }: { query: string }) =>
      api("/api/system/search-settings/test", {
        method: "POST",
        body: JSON.stringify({ query, limit: 5 }),
      }) as Promise<SearchTestResponse>,
    onSuccess: (result) => {
      setSearchTestResult(result);
      toast("ok", "Websearch test completed.");
    },
    onError: (error) => {
      setSearchTestResult(null);
      toast("err", error instanceof Error ? error.message : String(error));
    },
  });

  const saveInstallConfigMutation = useMutation({
    mutationFn: async () => {
      let parsed: unknown;
      try {
        parsed = JSON.parse(installConfigText);
      } catch (error) {
        throw new Error(
          error instanceof Error
            ? `Config JSON is invalid: ${error.message}`
            : "Config JSON is invalid."
        );
      }
      return api("/api/control-panel/config", {
        method: "PATCH",
        body: JSON.stringify({ config: parsed }),
      });
    },
    onSuccess: async () => {
      setInstallConfigError("");
      toast("ok", "Control panel config saved.");
      await queryClient.invalidateQueries({ queryKey: ["settings", "install"] });
      await queryClient.invalidateQueries({ queryKey: ["system", "capabilities"] });
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setInstallConfigError(message);
      toast("err", message);
    },
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
      const modelProviderId = String(draft.modelProviderId || "").trim();
      const modelId = String(draft.modelId || "").trim();
      const payload: Record<string, unknown> = {
        allowed_users: parseAllowedUsers(draft.allowedUsers),
        mention_only: !!draft.mentionOnly,
        security_profile: String(draft.securityProfile || "operator").trim() || "operator",
        model_provider_id: modelProviderId || null,
        model_id: modelId || null,
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
      channelDraftsHydratedRef.current[channel] = false;
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
      const token = String(draft?.botToken || "").trim();
      const payload: JsonObject = {};
      if (token) payload.bot_token = token;
      return client.channels.verify(channel, payload);
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
  const bugMonitorStatus = useMemo(
    () => ((bugMonitorStatusQuery.data as any)?.status || {}) as BugMonitorStatusRow,
    [bugMonitorStatusQuery.data]
  );
  const bugMonitorDrafts = useMemo(
    () =>
      Array.isArray((bugMonitorDraftsQuery.data as any)?.drafts)
        ? ((bugMonitorDraftsQuery.data as any).drafts as BugMonitorDraftRow[]) || []
        : [],
    [bugMonitorDraftsQuery.data]
  );
  const bugMonitorIncidents = useMemo(
    () =>
      Array.isArray((bugMonitorIncidentsQuery.data as any)?.incidents)
        ? ((bugMonitorIncidentsQuery.data as any).incidents as BugMonitorIncidentRow[]) || []
        : [],
    [bugMonitorIncidentsQuery.data]
  );
  const bugMonitorPosts = useMemo(
    () =>
      Array.isArray((bugMonitorPostsQuery.data as any)?.posts)
        ? ((bugMonitorPostsQuery.data as any).posts as BugMonitorPostRow[]) || []
        : [],
    [bugMonitorPostsQuery.data]
  );
  const selectedBugMonitorServer = useMemo(
    () =>
      mcpServers.find(
        (server) =>
          server.name.toLowerCase() ===
          String(bugMonitorMcpServer || "")
            .trim()
            .toLowerCase()
      ) || null,
    [bugMonitorMcpServer, mcpServers]
  );
  const selectedBugMonitorProvider = useMemo(
    () =>
      providers.find(
        (provider: any) =>
          String(provider?.id || "").toLowerCase() ===
          String(bugMonitorProviderId || "")
            .trim()
            .toLowerCase()
      ) || null,
    [bugMonitorProviderId, providers]
  );
  const bugMonitorProviderModels = useMemo(() => {
    const modelMap =
      selectedBugMonitorProvider && typeof selectedBugMonitorProvider === "object"
        ? selectedBugMonitorProvider.models || {}
        : {};
    return Object.keys(modelMap).sort((a, b) => a.localeCompare(b));
  }, [selectedBugMonitorProvider]);
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
  const bugMonitorWorkspaceDirectories = Array.isArray(
    bugMonitorWorkspaceBrowserQuery.data?.directories
  )
    ? bugMonitorWorkspaceBrowserQuery.data.directories
    : [];
  const bugMonitorWorkspaceSearchQuery = String(bugMonitorWorkspaceBrowserSearch || "")
    .trim()
    .toLowerCase();
  const filteredBugMonitorWorkspaceDirectories = useMemo(() => {
    if (!bugMonitorWorkspaceSearchQuery) return bugMonitorWorkspaceDirectories;
    return bugMonitorWorkspaceDirectories.filter((entry: any) => {
      const name = String(entry?.name || entry?.path || "")
        .trim()
        .toLowerCase();
      return name.includes(bugMonitorWorkspaceSearchQuery);
    });
  }, [bugMonitorWorkspaceDirectories, bugMonitorWorkspaceSearchQuery]);
  const bugMonitorWorkspaceParentDir = String(
    bugMonitorWorkspaceBrowserQuery.data?.parent || ""
  ).trim();
  const bugMonitorCurrentBrowseDir = String(
    bugMonitorWorkspaceBrowserQuery.data?.dir || bugMonitorWorkspaceBrowserDir || ""
  ).trim();

  useEffect(() => {
    const config =
      (bugMonitorConfigQuery.data as any)?.bug_monitor &&
      typeof (bugMonitorConfigQuery.data as any)?.bug_monitor === "object"
        ? ((bugMonitorConfigQuery.data as any).bug_monitor as BugMonitorConfigRow)
        : {};
    setBugMonitorEnabled(!!config.enabled);
    setBugMonitorPaused(!!config.paused);
    setBugMonitorWorkspaceRoot(String(config.workspace_root || "").trim());
    setBugMonitorRepo(String(config.repo || "").trim());
    setBugMonitorMcpServer(String(config.mcp_server || "").trim());
    setBugMonitorProviderPreference(String(config.provider_preference || "auto").trim() || "auto");
    setBugMonitorProviderId(String(config.model_policy?.default_model?.provider_id || "").trim());
    setBugMonitorModelId(String(config.model_policy?.default_model?.model_id || "").trim());
    setBugMonitorAutoCreateIssues(config.auto_create_new_issues !== false);
    setBugMonitorRequireApproval(!!config.require_approval_for_new_issues);
    setBugMonitorAutoComment(config.auto_comment_on_matched_open_issues !== false);
  }, [bugMonitorConfigQuery.data]);

  useEffect(() => {
    const config =
      channelsConfigQuery.data && typeof channelsConfigQuery.data === "object"
        ? (channelsConfigQuery.data as Record<string, ChannelConfigRow>)
        : {};
    if (!channelsConfigQuery.data || typeof channelsConfigQuery.data !== "object") return;
    setChannelDrafts((prev) => {
      const next = { ...prev };
      for (const channel of channelNames) {
        if (!next[channel]) {
          next[channel] = normalizeChannelDraft(channel, config[channel]);
          channelDraftsHydratedRef.current[channel] = true;
        }
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

  const copyBugMonitorDebugPayload = async () => {
    const payload = await api("/api/engine/bug-monitor/debug", { method: "GET" });
    await navigator.clipboard.writeText(JSON.stringify(payload, null, 2));
    toast("ok", "Bug Monitor debug payload copied.");
  };

  const sectionTabs: Array<{ id: SettingsSection; label: string; icon: string }> = [
    { id: "install", label: "Install", icon: "clipboard-list" },
    { id: "navigation", label: "Navigation", icon: "panel-left" },
    { id: "providers", label: "Providers", icon: "cpu" },
    { id: "search", label: "Web Search", icon: "globe" },
    { id: "scheduler", label: "Scheduler", icon: "layers" },
    { id: "identity", label: "Identity", icon: "badge-check" },
    { id: "theme", label: "Themes", icon: "paint-bucket" },
    { id: "channels", label: "Channels", icon: "message-circle" },
    { id: "mcp", label: "MCP", icon: "plug-zap" },
    { id: "bug_monitor", label: "Bug Monitor", icon: "bug-play" },
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
  const navigationVisibility = navigation?.routeVisibility || {};
  const defaultNavigationVisibility = getDefaultNavigationVisibility(!!navigation?.acaMode);
  const navigationRows = APP_NAV_ROUTES.map(([routeId, label, icon]) => {
    const typedRouteId = routeId as RouteId;
    const enabled = navigationVisibility[typedRouteId] !== false;
    const pinned = !!navigation?.acaMode && ACA_CORE_NAV_ROUTE_IDS.has(typedRouteId);
    const defaultVisible = defaultNavigationVisibility[typedRouteId] !== false;
    return {
      routeId,
      label,
      icon,
      enabled,
      pinned,
      defaultVisible,
      description:
        NAV_ROUTE_DESCRIPTIONS[routeId] ||
        `Open the ${String(label || routeId).toLowerCase()} section.`,
    };
  });
  const visibleNavigationCount = navigationRows.filter((row) => row.enabled).length;
  const defaultNavigationRows = navigationRows.filter((row) => row.defaultVisible);
  const advancedNavigationRows = navigationRows.filter((row) => !row.defaultVisible);
  const hiddenAdvancedNavigationCount = advancedNavigationRows.filter((row) => !row.enabled).length;

  useEffect(() => {
    const root = rootRef.current;
    if (root) renderIcons(root);
    else renderIcons();
  }, [
    activeSection,
    bugMonitorEnabled,
    bugMonitorPaused,
    bugMonitorWorkspaceRoot,
    bugMonitorMcpServer,
    bugMonitorStatus.readiness?.runtime_ready,
    bugMonitorStatus.runtime?.monitoring_active,
    bugMonitorStatus.runtime?.paused,
    bugMonitorStatus.runtime?.pending_incidents,
    bugMonitorStatus.pending_drafts,
    bugMonitorDrafts.length,
    bugMonitorIncidents.length,
    refreshBugMonitorBindingsMutation.isPending,
    bugMonitorPauseResumeMutation.isPending,
    bugMonitorDraftDecisionMutation.isPending,
    bugMonitorReplayIncidentMutation.isPending,
    bugMonitorTriageRunMutation.isPending,
    saveBugMonitorMutation.isPending,
    mcpActionMutation.isPending,
    saveSearchSettingsMutation.isPending,
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
              {activeSection === "navigation" ? (
                <PanelCard
                  title="Sidebar navigation"
                  subtitle={
                    navigation?.acaMode
                      ? "ACA mode keeps Dashboard, Chat, Coding, and Settings visible by default."
                      : "Choose which sections appear in the sidebar. Advanced and experimental surfaces stay hidden until you turn them on."
                  }
                  actions={
                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Badge tone={navigation?.acaMode ? "ok" : "info"}>
                        {navigation?.acaMode ? "ACA compact default" : "Core-first default"}
                      </Badge>
                      <Badge tone="ghost">
                        {visibleNavigationCount}/{navigationRows.length} visible
                      </Badge>
                      <button
                        className="tcp-btn"
                        type="button"
                        onClick={() => navigation?.showAllSections()}
                      >
                        Show all sections
                      </button>
                      <button
                        className="tcp-btn-primary"
                        type="button"
                        onClick={() => navigation?.resetNavigation()}
                      >
                        Reset {navigation?.acaMode ? "ACA compact" : "default"}
                      </button>
                    </div>
                  }
                >
                  <div className="grid gap-4">
                    <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                      <div className="flex items-center justify-between gap-3">
                        <div>
                          <div className="font-medium">Default sections</div>
                          <div className="tcp-subtle mt-1 text-xs">
                            These sections are part of the standard control panel and start on by
                            default.
                          </div>
                        </div>
                        <Badge tone="ok">
                          {defaultNavigationRows.filter((row) => row.enabled).length}/
                          {defaultNavigationRows.length} shown
                        </Badge>
                      </div>
                      <div className="mt-3 grid gap-2">
                        {defaultNavigationRows.map((row) => (
                          <button
                            key={row.routeId}
                            type="button"
                            className={`flex items-center justify-between rounded-xl border px-3 py-3 text-left transition ${
                              row.enabled
                                ? "border-lime-500/40 bg-lime-500/10 hover:border-lime-400/70"
                                : "border-slate-700/60 bg-slate-900/20 hover:border-slate-500/70"
                            }`}
                            onClick={() =>
                              navigation?.setRouteVisibility(row.routeId as RouteId, !row.enabled)
                            }
                          >
                            <div className="flex min-w-0 items-center gap-3">
                              <span
                                className={`flex h-9 w-9 items-center justify-center rounded-lg border ${
                                  row.enabled
                                    ? "border-lime-500/30 bg-lime-500/10 text-lime-200"
                                    : "border-slate-700/70 bg-slate-950/30 text-slate-300"
                                }`}
                              >
                                <i data-lucide={row.icon}></i>
                              </span>
                              <div className="min-w-0">
                                <div className="font-medium">{row.label}</div>
                                <div className="tcp-subtle truncate text-xs">{row.description}</div>
                              </div>
                            </div>
                            <div className="flex items-center gap-2">
                              {row.pinned ? <Badge tone="ok">Pinned</Badge> : null}
                              <Badge tone={row.enabled ? "ok" : "ghost"}>
                                {row.enabled ? "Shown" : "Hidden"}
                              </Badge>
                            </div>
                          </button>
                        ))}
                      </div>
                    </div>

                    <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                      <div className="flex items-center justify-between gap-3">
                        <div>
                          <div className="font-medium">Advanced / experimental sections</div>
                          <div className="tcp-subtle mt-1 text-xs">
                            Only routes that ship hidden by default live here.
                          </div>
                        </div>
                        <Badge tone={hiddenAdvancedNavigationCount > 0 ? "warn" : "ok"}>
                          {hiddenAdvancedNavigationCount} hidden
                        </Badge>
                      </div>
                      <div className="mt-3 grid gap-2">
                        {advancedNavigationRows.map((row) => (
                          <button
                            key={row.routeId}
                            type="button"
                            className={`flex items-center justify-between rounded-xl border px-3 py-3 text-left transition ${
                              row.enabled
                                ? "border-lime-500/40 bg-lime-500/10 hover:border-lime-400/70"
                                : "border-slate-700/60 bg-slate-900/20 hover:border-slate-500/70"
                            }`}
                            onClick={() =>
                              navigation?.setRouteVisibility(row.routeId as RouteId, !row.enabled)
                            }
                          >
                            <div className="flex min-w-0 items-center gap-3">
                              <span
                                className={`flex h-9 w-9 items-center justify-center rounded-lg border ${
                                  row.enabled
                                    ? "border-lime-500/30 bg-lime-500/10 text-lime-200"
                                    : "border-slate-700/70 bg-slate-950/30 text-slate-300"
                                }`}
                              >
                                <i data-lucide={row.icon}></i>
                              </span>
                              <div className="min-w-0">
                                <div className="font-medium">{row.label}</div>
                                <div className="tcp-subtle truncate text-xs">{row.description}</div>
                              </div>
                            </div>
                            <Badge tone={row.enabled ? "ok" : "ghost"}>
                              {row.enabled ? "Shown" : "Hidden"}
                            </Badge>
                          </button>
                        ))}
                      </div>
                    </div>

                    <div className="tcp-subtle text-xs">
                      These preferences are stored in this browser only.
                    </div>
                  </div>
                </PanelCard>
              ) : null}

              {activeSection === "install" ? (
                <PanelCard
                  title="Install config"
                  subtitle="Durable non-secret install preferences stored in tandem-data for Tandem startup and navigation defaults."
                  actions={
                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Badge
                        tone={
                          String(installProfileQuery.data?.control_panel_mode || "")
                            .trim()
                            .toLowerCase() === "aca"
                            ? "ok"
                            : "info"
                        }
                      >
                        {installProfileQuery.data?.control_panel_mode || "auto"}
                      </Badge>
                      <Badge
                        tone={installProfileQuery.data?.control_panel_config_ready ? "ok" : "warn"}
                      >
                        {installProfileQuery.data?.control_panel_config_ready
                          ? "Ready"
                          : "Needs setup"}
                      </Badge>
                      <button
                        type="button"
                        className="tcp-btn"
                        onClick={() =>
                          installConfigQuery
                            .refetch()
                            .then(() => toast("ok", "Install config refreshed."))
                        }
                      >
                        <i data-lucide="refresh-cw"></i>
                        Refresh
                      </button>
                      <button
                        type="button"
                        className="tcp-btn-primary"
                        onClick={() => saveInstallConfigMutation.mutate()}
                        disabled={saveInstallConfigMutation.isPending}
                      >
                        <i data-lucide="save"></i>
                        Save config
                      </button>
                    </div>
                  }
                >
                  <div className="grid gap-4">
                    <div className="grid gap-3 md:grid-cols-2">
                      <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                        <div className="font-medium">Startup profile</div>
                        <div className="tcp-subtle mt-1 text-xs">
                          {installProfileQuery.data?.control_panel_mode_reason ||
                            "The control panel auto-detects its startup mode and can be overridden with TANDEM_CONTROL_PANEL_MODE."}
                        </div>
                        <div className="mt-3 grid gap-2 text-xs">
                          <div className="flex items-center justify-between gap-3">
                            <span className="tcp-subtle">Mode source</span>
                            <span>
                              {installProfileQuery.data?.control_panel_mode_source || "detected"}
                            </span>
                          </div>
                          <div className="flex items-center justify-between gap-3">
                            <span className="tcp-subtle">Integration detected</span>
                            <span>{installProfileQuery.data?.aca_integration ? "yes" : "no"}</span>
                          </div>
                          <div className="flex items-center justify-between gap-3">
                            <span className="tcp-subtle">Compact nav</span>
                            <span>
                              {installProfileQuery.data?.control_panel_compact_nav ? "on" : "off"}
                            </span>
                          </div>
                        </div>
                      </div>
                      <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                        <div className="font-medium">Config file</div>
                        <div className="tcp-subtle mt-1 break-all text-xs">
                          {installProfileQuery.data?.control_panel_config_path ||
                            installConfigQuery.data?.path ||
                            "control-panel-config.json"}
                        </div>
                        <div className="mt-3 grid gap-2 text-xs">
                          <div className="flex items-center justify-between gap-3">
                            <span className="tcp-subtle">Ready</span>
                            <span>
                              {installProfileQuery.data?.control_panel_config_ready ? "yes" : "no"}
                            </span>
                          </div>
                          <div className="flex items-center justify-between gap-3">
                            <span className="tcp-subtle">Missing</span>
                            <span>
                              {Array.isArray(installProfileQuery.data?.control_panel_config_missing)
                                ? installProfileQuery.data?.control_panel_config_missing.join(
                                    ", "
                                  ) || "none"
                                : "unknown"}
                            </span>
                          </div>
                        </div>
                      </div>
                    </div>

                    <label className="grid gap-2">
                      <span className="text-sm font-medium">Control panel config JSON</span>
                      <textarea
                        className="tcp-input min-h-[28rem] font-mono text-xs leading-5"
                        value={installConfigText}
                        onInput={(event) =>
                          setInstallConfigText((event.target as HTMLTextAreaElement).value)
                        }
                        spellCheck={false}
                      />
                    </label>

                    {installConfigError ? (
                      <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-sm text-rose-200">
                        {installConfigError}
                      </div>
                    ) : null}

                    <div className="tcp-subtle text-xs">
                      This file holds non-secret install state: repo binding, provider defaults,
                      task source, swarm policy, GitHub MCP preferences, and navigation defaults.
                      Secrets should stay in `.env` or token files.
                    </div>
                  </div>
                </PanelCard>
              ) : null}

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
                          <div className="tcp-list-item grid gap-3">
                            <div className="flex flex-wrap items-start justify-between gap-3">
                              <div>
                                <div className="font-medium">Custom OpenAI-compatible provider</div>
                                <div className="tcp-subtle mt-1 text-xs">
                                  Add providers like MiniMax by ID, base URL, default model, and API
                                  key.
                                </div>
                              </div>
                              <Badge tone={customConfiguredProviders.length ? "ok" : "info"}>
                                {customConfiguredProviders.length} configured
                              </Badge>
                            </div>

                            <button
                              type="button"
                              className="tcp-list-item text-left"
                              onClick={() => setCustomProviderFormOpen((prev) => !prev)}
                              aria-expanded={customProviderFormOpen}
                            >
                              <div className="flex items-center justify-between gap-3">
                                <div>
                                  <div className="font-medium inline-flex items-center gap-2">
                                    <i
                                      data-lucide={
                                        customProviderFormOpen ? "chevron-down" : "chevron-right"
                                      }
                                    ></i>
                                    <span>
                                      {customProviderFormOpen
                                        ? "Hide custom provider form"
                                        : "Show custom provider form"}
                                    </span>
                                  </div>
                                  <div className="tcp-subtle mt-1 text-xs">
                                    Use this for OpenAI-compatible endpoints. Anthropic is handled
                                    by the built-in provider row below.
                                  </div>
                                </div>
                                <Badge tone="info">OpenAI-compatible only</Badge>
                              </div>
                            </button>

                            <AnimatePresence initial={false}>
                              {customProviderFormOpen ? (
                                <motion.div
                                  className="grid gap-3"
                                  initial={{ opacity: 0, y: -8 }}
                                  animate={{ opacity: 1, y: 0 }}
                                  exit={{ opacity: 0, y: -8 }}
                                >
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
                                            setCustomProviderId(
                                              (event.target as HTMLInputElement).value
                                            )
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
                                            setCustomProviderModel(
                                              (event.target as HTMLInputElement).value
                                            )
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
                                          setCustomProviderUrl(
                                            (event.target as HTMLInputElement).value
                                          )
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
                                          setCustomProviderApiKey(
                                            (event.target as HTMLInputElement).value
                                          )
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
                                </motion.div>
                              ) : null}
                            </AnimatePresence>

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
                                          setCustomProviderFormOpen(true);
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
                              const providerAuth = providerAuthById[providerId] || {};
                              const currentDefaultProvider = String(
                                providersConfig.data?.default || ""
                              )
                                .trim()
                                .toLowerCase();
                              const codexIsDefaultProvider =
                                providerId === OPENAI_CODEX_PROVIDER_ID &&
                                currentDefaultProvider === OPENAI_CODEX_PROVIDER_ID;
                              const authKind = String(
                                providerAuth?.auth_kind || providerAuth?.authKind || ""
                              )
                                .trim()
                                .toLowerCase();
                              const oauthStatus = String(providerAuth?.status || "")
                                .trim()
                                .toLowerCase();
                              const oauthEmail = String(providerAuth?.email || "").trim();
                              const oauthDisplayName = String(
                                providerAuth?.display_name || providerAuth?.displayName || ""
                              ).trim();
                              const oauthManagedBy = String(
                                providerAuth?.managed_by || providerAuth?.managedBy || ""
                              ).trim();
                              const oauthExpiresAtMs = Number(
                                providerAuth?.expires_at_ms || providerAuth?.expiresAtMs || 0
                              );
                              const localCodexSessionAvailable =
                                providerAuth?.local_session_available === true ||
                                providerAuth?.localSessionAvailable === true;
                              const oauthSessionId = String(
                                oauthSessionByProvider[providerId] || ""
                              ).trim();
                              const oauthPending =
                                !!oauthSessionId ||
                                (authorizeProviderOAuthMutation.isPending &&
                                  String(authorizeProviderOAuthMutation.variables?.providerId || "")
                                    .trim()
                                    .toLowerCase() === providerId);
                              const oauthConnected =
                                authKind === "oauth" &&
                                providerAuth?.connected === true &&
                                oauthStatus !== "reauth_required";
                              const supportsOAuth = providerId === OPENAI_CODEX_PROVIDER_ID;
                              const canUseOAuthHere = !supportsOAuth || localEngine;
                              const oauthBadge = oauthPending
                                ? { tone: "info" as const, text: "sign-in pending" }
                                : oauthConnected
                                  ? { tone: "ok" as const, text: "account connected" }
                                  : oauthStatus === "reauth_required"
                                    ? { tone: "warn" as const, text: "reauth required" }
                                    : { tone: "warn" as const, text: "not connected" };

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
                                    {keyUrl && !supportsOAuth ? (
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

                                    {supportsOAuth ? (
                                      <div className="grid gap-3 rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                                        <div className="flex flex-wrap items-start justify-between gap-3">
                                          <div className="min-w-0">
                                            <div className="font-medium">
                                              {String(providerHint?.label || "Codex Account")}
                                            </div>
                                            <div className="tcp-subtle text-xs">
                                              {String(
                                                providerHint?.description ||
                                                  "Use your ChatGPT/Codex subscription instead of a separate API key."
                                              )}
                                            </div>
                                          </div>
                                          <Badge tone={oauthBadge.tone}>{oauthBadge.text}</Badge>
                                        </div>

                                        <div className="grid gap-1 text-xs tcp-subtle">
                                          {oauthConnected ? (
                                            <div>
                                              {oauthDisplayName || oauthEmail
                                                ? `Connected as ${oauthDisplayName || oauthEmail}.`
                                                : "Connected to a Codex account."}
                                            </div>
                                          ) : null}
                                          {oauthManagedBy ? (
                                            <div>
                                              Managed by{" "}
                                              {oauthManagedBy === "codex-cli"
                                                ? "the local Codex CLI session"
                                                : "Tandem"}
                                              .
                                            </div>
                                          ) : null}
                                          {oauthExpiresAtMs > 0 ? (
                                            <div>
                                              Session status refreshes through{" "}
                                              {new Date(oauthExpiresAtMs).toLocaleString()}.
                                            </div>
                                          ) : null}
                                          {providerId === OPENAI_CODEX_PROVIDER_ID &&
                                          oauthConnected &&
                                          !codexIsDefaultProvider ? (
                                            <div>
                                              Tandem is connected to Codex, but new runs are still
                                              using a different default provider.
                                            </div>
                                          ) : null}
                                          {canUseOAuthHere ? (
                                            <div>
                                              {localCodexSessionAvailable
                                                ? "Local Codex CLI session detected on this machine."
                                                : "If the Codex CLI is already signed in on this machine, you can mirror that session here instead of starting a fresh browser login."}
                                            </div>
                                          ) : null}
                                          {!canUseOAuthHere ? (
                                            <div>
                                              Codex account sign-in is only enabled when this
                                              control panel is connected to a local engine.
                                            </div>
                                          ) : null}
                                        </div>

                                        <div className="flex flex-wrap gap-2">
                                          <button
                                            type="button"
                                            className="tcp-btn"
                                            disabled={
                                              !canUseOAuthHere ||
                                              oauthPending ||
                                              disconnectProviderOAuthMutation.isPending
                                            }
                                            onClick={() =>
                                              authorizeProviderOAuthMutation.mutate({ providerId })
                                            }
                                          >
                                            <i data-lucide="log-in"></i>
                                            {oauthConnected
                                              ? "Reconnect Codex Account"
                                              : "Connect Codex Account"}
                                          </button>
                                          <button
                                            type="button"
                                            className="tcp-btn h-10 px-4 text-sm"
                                            disabled={
                                              !canUseOAuthHere ||
                                              authorizeProviderOAuthMutation.isPending ||
                                              useLocalCodexSessionMutation.isPending
                                            }
                                            onClick={() => {
                                              setOauthSessionByProvider((current) => {
                                                if (!current[providerId]) return current;
                                                const next = { ...current };
                                                delete next[providerId];
                                                return next;
                                              });
                                              useLocalCodexSessionMutation.mutate({ providerId });
                                            }}
                                          >
                                            <i data-lucide="link-2"></i>
                                            Use Local Codex Session
                                          </button>
                                          {providerId === OPENAI_CODEX_PROVIDER_ID &&
                                          oauthConnected &&
                                          !codexIsDefaultProvider ? (
                                            <button
                                              type="button"
                                              className="tcp-btn h-10 px-4 text-sm"
                                              disabled={setDefaultsMutation.isPending}
                                              onClick={() =>
                                                setDefaultsMutation.mutate({
                                                  providerId,
                                                  modelId: defaultModel || "gpt-5.4",
                                                })
                                              }
                                            >
                                              <i data-lucide="sparkles"></i>
                                              Use for Tandem Runs
                                            </button>
                                          ) : null}
                                          <button
                                            type="button"
                                            className="tcp-btn h-10 px-4 text-sm"
                                            disabled={
                                              !oauthConnected ||
                                              disconnectProviderOAuthMutation.isPending ||
                                              oauthPending
                                            }
                                            onClick={() =>
                                              disconnectProviderOAuthMutation.mutate({ providerId })
                                            }
                                          >
                                            <i data-lucide="unlink"></i>
                                            Disconnect
                                          </button>
                                          {oauthPending ? (
                                            <button
                                              type="button"
                                              className="tcp-btn h-10 px-4 text-sm"
                                              onClick={() =>
                                                window.open(
                                                  "https://chatgpt.com/codex",
                                                  "_blank",
                                                  "noopener,noreferrer"
                                                )
                                              }
                                            >
                                              <i data-lucide="external-link"></i>
                                              Open Codex
                                            </button>
                                          ) : null}
                                        </div>
                                      </div>
                                    ) : (
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
                                    )}
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

              {activeSection === "search" ? (
                <PanelCard
                  title="Web Search"
                  subtitle="Configure the engine's `websearch` backend and provider keys."
                  actions={
                    <Toolbar>
                      <Badge
                        tone={searchSettingsQuery.data?.settings?.has_brave_key ? "ok" : "warn"}
                      >
                        Brave{" "}
                        {searchSettingsQuery.data?.settings?.has_brave_key
                          ? "configured"
                          : "missing"}
                      </Badge>
                      <Badge tone={searchSettingsQuery.data?.settings?.has_exa_key ? "ok" : "warn"}>
                        Exa{" "}
                        {searchSettingsQuery.data?.settings?.has_exa_key ? "configured" : "missing"}
                      </Badge>
                      <button
                        className="tcp-btn"
                        onClick={() =>
                          testSearchMutation.mutate({
                            query: searchTestQuery.trim(),
                          })
                        }
                        disabled={
                          !searchSettingsQuery.data?.available ||
                          !searchTestQuery.trim() ||
                          testSearchMutation.isPending
                        }
                      >
                        <i
                          data-lucide={testSearchMutation.isPending ? "loader-circle" : "search"}
                        ></i>
                        {testSearchMutation.isPending ? "Testing..." : "Test search"}
                      </button>
                      <button
                        className="tcp-btn-primary"
                        onClick={() =>
                          saveSearchSettingsMutation.mutate({
                            backend: searchBackend,
                            tandem_url: searchTandemUrl,
                            searxng_url: searchSearxngUrl,
                            timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                            brave_api_key: searchBraveKey.trim() || undefined,
                            exa_api_key: searchExaKey.trim() || undefined,
                          })
                        }
                        disabled={
                          !searchSettingsQuery.data?.available ||
                          saveSearchSettingsMutation.isPending
                        }
                      >
                        <i data-lucide="save"></i>
                        Save
                      </button>
                    </Toolbar>
                  }
                >
                  {!searchSettingsQuery.data?.available ? (
                    <EmptyState
                      text={
                        searchSettingsQuery.data?.reason ||
                        "Search settings are only editable here when the panel points at a local engine host."
                      }
                    />
                  ) : (
                    <div className="grid gap-4">
                      <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4 text-sm">
                        <div className="font-medium">Engine env file</div>
                        <div className="tcp-subtle mt-1 break-all">
                          {searchSettingsQuery.data?.managed_env_path || "/etc/tandem/engine.env"}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          {searchSettingsQuery.data?.restart_hint ||
                            "Restart tandem-engine after saving search settings."}
                        </div>
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <label className="grid gap-1 text-sm">
                          <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                            Backend
                          </span>
                          <select
                            className="tcp-select"
                            value={searchBackend}
                            onChange={(e) =>
                              setSearchBackend((e.target as HTMLSelectElement).value)
                            }
                          >
                            <option value="auto">Auto failover</option>
                            <option value="brave">Brave Search</option>
                            <option value="exa">Exa</option>
                            <option value="searxng">SearxNG</option>
                            <option value="tandem">Tandem hosted search</option>
                            <option value="none">Disable websearch</option>
                          </select>
                        </label>
                        <label className="grid gap-1 text-sm">
                          <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                            Timeout (ms)
                          </span>
                          <input
                            className="tcp-input"
                            type="number"
                            min={1000}
                            max={120000}
                            value={searchTimeoutMs}
                            onInput={(e) =>
                              setSearchTimeoutMs((e.target as HTMLInputElement).value)
                            }
                          />
                        </label>
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <label className="grid gap-1 text-sm">
                          <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                            Tandem search URL
                          </span>
                          <input
                            className="tcp-input"
                            placeholder="https://search.tandem.ac"
                            value={searchTandemUrl}
                            onInput={(e) =>
                              setSearchTandemUrl((e.target as HTMLInputElement).value)
                            }
                          />
                          <span className="tcp-subtle text-xs">
                            Only used when backend is set to `tandem` or `auto`. This is the hosted
                            Tandem search router, not the SearXNG endpoint.
                          </span>
                        </label>
                        <label className="grid gap-1 text-sm">
                          <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                            SearxNG URL
                          </span>
                          <input
                            className="tcp-input"
                            placeholder="http://127.0.0.1:8080"
                            value={searchSearxngUrl}
                            onInput={(e) =>
                              setSearchSearxngUrl((e.target as HTMLInputElement).value)
                            }
                          />
                          <span className="tcp-subtle text-xs">
                            Only used when backend is `searxng` or `auto`.
                          </span>
                        </label>
                      </div>

                      <div className="grid gap-3 rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                        <div className="flex items-center justify-between gap-3">
                          <div>
                            <div className="font-medium">Search test</div>
                            <div className="tcp-subtle mt-1 text-xs">
                              Runs `websearch` against the currently running engine config and
                              renders the result as markdown below.
                            </div>
                          </div>
                          <Badge tone="warn">Tests live engine config</Badge>
                        </div>
                        <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
                          <input
                            className="tcp-input"
                            placeholder="Try a test query like autonomous AI agentic workflows"
                            value={searchTestQuery}
                            onInput={(e) =>
                              setSearchTestQuery((e.target as HTMLInputElement).value)
                            }
                          />
                          <button
                            className="tcp-btn"
                            onClick={() =>
                              testSearchMutation.mutate({
                                query: searchTestQuery.trim(),
                              })
                            }
                            disabled={
                              !searchSettingsQuery.data?.available ||
                              !searchTestQuery.trim() ||
                              testSearchMutation.isPending
                            }
                          >
                            <i
                              data-lucide={testSearchMutation.isPending ? "loader-circle" : "play"}
                            ></i>
                            {testSearchMutation.isPending ? "Running..." : "Run test"}
                          </button>
                        </div>
                        {searchTestResult?.markdown ? (
                          <div className="grid gap-2">
                            <div className="flex flex-wrap items-center gap-2 text-xs">
                              <Badge tone="ok">
                                Backend{" "}
                                {String(
                                  searchTestResult.parsed_output?.backend ||
                                    searchTestResult.metadata?.backend ||
                                    "unknown"
                                )}
                              </Badge>
                              {searchTestResult.parsed_output?.configured_backend ? (
                                <Badge tone="info">
                                  Configured{" "}
                                  {String(searchTestResult.parsed_output.configured_backend)}
                                </Badge>
                              ) : null}
                              {searchTestResult.metadata?.error ? (
                                <Badge tone="warn">
                                  {String(searchTestResult.metadata.error).replaceAll("_", " ")}
                                </Badge>
                              ) : null}
                            </div>
                            <div
                              className="tcp-markdown tcp-markdown-ai max-h-[320px] overflow-auto rounded-xl border border-slate-700/60 bg-black/20 p-3 text-sm"
                              dangerouslySetInnerHTML={{
                                __html: renderMarkdownSafe(searchTestResult.markdown || ""),
                              }}
                            />
                          </div>
                        ) : null}
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <div className="grid gap-2 rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                          <div className="flex items-center justify-between gap-2">
                            <div className="font-medium">Brave Search key</div>
                            <Badge
                              tone={
                                searchSettingsQuery.data?.settings?.has_brave_key ? "ok" : "warn"
                              }
                            >
                              {searchSettingsQuery.data?.settings?.has_brave_key
                                ? "Saved"
                                : "Missing"}
                            </Badge>
                          </div>
                          <input
                            className="tcp-input"
                            type="password"
                            placeholder="Paste Brave Search key"
                            value={searchBraveKey}
                            onInput={(e) => setSearchBraveKey((e.target as HTMLInputElement).value)}
                          />
                          <div className="flex flex-wrap gap-2">
                            <button
                              className="tcp-btn"
                              onClick={() =>
                                saveSearchSettingsMutation.mutate({
                                  backend: searchBackend,
                                  tandem_url: searchTandemUrl,
                                  searxng_url: searchSearxngUrl,
                                  timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                                  brave_api_key: searchBraveKey.trim() || undefined,
                                })
                              }
                              disabled={
                                !searchBraveKey.trim() || saveSearchSettingsMutation.isPending
                              }
                            >
                              Save Brave Key
                            </button>
                            {searchSettingsQuery.data?.settings?.has_brave_key ? (
                              <button
                                className="tcp-btn"
                                onClick={() =>
                                  saveSearchSettingsMutation.mutate({
                                    backend: searchBackend,
                                    tandem_url: searchTandemUrl,
                                    searxng_url: searchSearxngUrl,
                                    timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                                    clear_brave_key: true,
                                  })
                                }
                                disabled={saveSearchSettingsMutation.isPending}
                              >
                                Remove
                              </button>
                            ) : null}
                          </div>
                        </div>

                        <div className="grid gap-2 rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                          <div className="flex items-center justify-between gap-2">
                            <div className="font-medium">Exa key</div>
                            <Badge
                              tone={searchSettingsQuery.data?.settings?.has_exa_key ? "ok" : "warn"}
                            >
                              {searchSettingsQuery.data?.settings?.has_exa_key
                                ? "Saved"
                                : "Missing"}
                            </Badge>
                          </div>
                          <input
                            className="tcp-input"
                            type="password"
                            placeholder="Paste Exa API key"
                            value={searchExaKey}
                            onInput={(e) => setSearchExaKey((e.target as HTMLInputElement).value)}
                          />
                          <div className="flex flex-wrap gap-2">
                            <button
                              className="tcp-btn"
                              onClick={() =>
                                saveSearchSettingsMutation.mutate({
                                  backend: searchBackend,
                                  tandem_url: searchTandemUrl,
                                  searxng_url: searchSearxngUrl,
                                  timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                                  exa_api_key: searchExaKey.trim() || undefined,
                                })
                              }
                              disabled={
                                !searchExaKey.trim() || saveSearchSettingsMutation.isPending
                              }
                            >
                              Save Exa Key
                            </button>
                            {searchSettingsQuery.data?.settings?.has_exa_key ? (
                              <button
                                className="tcp-btn"
                                onClick={() =>
                                  saveSearchSettingsMutation.mutate({
                                    backend: searchBackend,
                                    tandem_url: searchTandemUrl,
                                    searxng_url: searchSearxngUrl,
                                    timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                                    clear_exa_key: true,
                                  })
                                }
                                disabled={saveSearchSettingsMutation.isPending}
                              >
                                Remove
                              </button>
                            ) : null}
                          </div>
                        </div>
                      </div>

                      <div className="tcp-subtle text-xs">
                        `auto` tries the configured backends with failover. If Brave is rate-limited
                        and Exa is configured, the engine can continue with Exa instead of returning
                        a generic unavailable message.
                      </div>
                    </div>
                  )}
                </PanelCard>
              ) : null}

              {activeSection === "scheduler" ? (
                <PanelCard
                  title="Automation Scheduler"
                  subtitle="Controls parallel execution of automation runs. Restart tandem-engine after changing."
                  actions={
                    <Toolbar>
                      <button
                        className="tcp-btn-primary"
                        onClick={() =>
                          saveSchedulerSettingsMutation.mutate({
                            mode: schedulerMode,
                            max_concurrent_runs: schedulerMaxConcurrent
                              ? Number.parseInt(schedulerMaxConcurrent, 10)
                              : null,
                          })
                        }
                        disabled={
                          !schedulerSettingsQuery.data?.available ||
                          saveSchedulerSettingsMutation.isPending
                        }
                      >
                        <i data-lucide="save"></i>
                        Save
                      </button>
                    </Toolbar>
                  }
                >
                  {!schedulerSettingsQuery.data?.available ? (
                    <EmptyState
                      text={
                        schedulerSettingsQuery.data?.reason ||
                        "Scheduler settings are only editable here when the panel points at a local engine host."
                      }
                    />
                  ) : (
                    <div className="grid gap-4">
                      <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4 text-sm">
                        <div className="font-medium">Engine env file</div>
                        <div className="tcp-subtle mt-1 break-all">
                          {schedulerSettingsQuery.data?.managed_env_path ||
                            "/etc/tandem/engine.env"}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          {schedulerSettingsQuery.data?.restart_hint ||
                            "Restart tandem-engine after changing scheduler mode."}
                        </div>
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <label className="grid gap-1 text-sm">
                          <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                            Mode
                          </span>
                          <select
                            className="tcp-select"
                            value={schedulerMode}
                            onChange={(e) =>
                              setSchedulerMode((e.target as HTMLSelectElement).value)
                            }
                          >
                            <option value="multi">Multi — parallel runs (recommended)</option>
                            <option value="single">Single — one run at a time</option>
                          </select>
                        </label>
                        <label className="grid gap-1 text-sm">
                          <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                            Max concurrent runs
                          </span>
                          <input
                            className="tcp-input"
                            type="number"
                            min={1}
                            max={32}
                            placeholder="8 (default)"
                            value={schedulerMaxConcurrent}
                            onInput={(e) =>
                              setSchedulerMaxConcurrent((e.target as HTMLInputElement).value)
                            }
                          />
                        </label>
                      </div>

                      <div className="tcp-subtle text-xs">
                        Multi mode allows several automation runs to execute concurrently. Max
                        concurrent runs caps parallelism. Changes require a tandem-engine restart to
                        take effect.
                      </div>
                    </div>
                  )}
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
                            <img
                              src={botAvatarUrl || "/icon.png"}
                              alt={botName || "Tandem"}
                              className="block h-full w-full object-contain p-1"
                            />
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
                      const hasSavedConfig = channelConfigHasSavedSettings(channel, config);
                      const channelSettingsDirty = !channelDraftMatchesConfig(
                        channel,
                        draft,
                        config
                      );

                      return (
                        <div key={channel} className="tcp-list-item grid gap-3">
                          <div className="flex flex-wrap items-center justify-between gap-3">
                            <div>
                              <div className="font-semibold capitalize">{channel}</div>
                              <div className="tcp-subtle text-xs">
                                {channel === "telegram"
                                  ? "Bot token, allowed users, style profile, and optional model override."
                                  : channel === "discord"
                                    ? "Bot token, allowed users, mention policy, guild targeting, and optional model override."
                                    : "Bot token, allowed users, mention policy, default channel, and optional model override."}
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
                                  ? String(config.token_masked || "****")
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

                          <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                            <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
                              <div>
                                <div className="font-medium">Channel model override</div>
                                <div className="tcp-subtle text-xs">
                                  Pick the provider and model this channel should use. Leave both
                                  blank to inherit Tandem&apos;s global default.
                                </div>
                              </div>
                              <Badge tone={draft.modelProviderId && draft.modelId ? "ok" : "info"}>
                                {draft.modelProviderId && draft.modelId
                                  ? "Custom model"
                                  : "Global default"}
                              </Badge>
                            </div>
                            <ProviderModelSelector
                              providerLabel="Provider"
                              modelLabel="Model"
                              draft={{
                                provider: draft.modelProviderId,
                                model: draft.modelId,
                              }}
                              providers={channelProviderOptions}
                              onChange={({ provider, model }) =>
                                setChannelDrafts((prev) => ({
                                  ...prev,
                                  [channel]: {
                                    ...draft,
                                    modelProviderId: provider,
                                    modelId: model,
                                  },
                                }))
                              }
                              inheritLabel="Use global default"
                            />
                            <div className="mt-2 tcp-subtle text-xs">
                              {draft.modelProviderId && draft.modelId ? (
                                <span>
                                  Selected model: <strong>{draft.modelProviderId}</strong> /{" "}
                                  <strong>{draft.modelId}</strong>
                                </span>
                              ) : channelDefaultModel.provider && channelDefaultModel.model ? (
                                <span>
                                  Inheriting Tandem default:{" "}
                                  <strong>{channelDefaultModel.provider}</strong> /{" "}
                                  <strong>{channelDefaultModel.model}</strong>
                                </span>
                              ) : (
                                <span>No global default model is configured yet.</span>
                              )}
                            </div>
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

                          <div className="flex flex-wrap gap-2">
                            <button
                              className="tcp-btn-primary"
                              disabled={saveChannelMutation.isPending || !channelSettingsDirty}
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

                          <motion.div
                            layout
                            className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3"
                          >
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
                              <div className="flex flex-wrap items-center gap-2">
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
                                <button
                                  className="tcp-btn"
                                  aria-expanded={!!channelToolScopeOpen[channel]}
                                  onClick={() =>
                                    setChannelToolScopeOpen((prev) => ({
                                      ...prev,
                                      [channel]: !prev[channel],
                                    }))
                                  }
                                >
                                  <span>{channelToolScopeOpen[channel] ? "Hide" : "Show"}</span>
                                  <i
                                    data-lucide="chevron-down"
                                    className={`h-4 w-4 transition-transform duration-200 ${
                                      channelToolScopeOpen[channel] ? "rotate-180" : ""
                                    }`}
                                  ></i>
                                </button>
                              </div>
                            </div>

                            <div className="tcp-subtle text-xs">
                              {toolPrefs.enabled_mcp_servers.length
                                ? `${toolPrefs.enabled_mcp_servers.length} MCP server${
                                    toolPrefs.enabled_mcp_servers.length === 1 ? "" : "s"
                                  } enabled.`
                                : publicDemo
                                  ? "MCP servers stay disabled in public demo mode."
                                  : "No MCP servers enabled for this channel."}
                            </div>

                            <AnimatePresence initial={false}>
                              {channelToolScopeOpen[channel] ? (
                                <motion.div
                                  key={`${channel}-tool-scope-body`}
                                  initial={{ opacity: 0, height: 0, y: -6 }}
                                  animate={{ opacity: 1, height: "auto", y: 0 }}
                                  exit={{ opacity: 0, height: 0, y: -6 }}
                                  transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
                                  className="overflow-hidden"
                                >
                                  <div className="grid gap-3 pt-3">
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
                                                key={`${channel}-tool-${tool}`}
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
                                                  <span className="font-mono text-xs">
                                                    {server.name}
                                                  </span>
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
                                </motion.div>
                              ) : null}
                            </AnimatePresence>
                          </motion.div>
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

              {activeSection === "bug_monitor" ? (
                <PanelCard
                  title="Bug monitor"
                  actions={
                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Badge
                        tone={
                          bugMonitorStatus.runtime?.monitoring_active
                            ? bugMonitorStatus.readiness?.publish_ready
                              ? "ok"
                              : "info"
                            : bugMonitorStatus.readiness?.ingest_ready
                              ? "info"
                              : "warn"
                        }
                      >
                        {bugMonitorStatus.runtime?.monitoring_active
                          ? bugMonitorStatus.readiness?.publish_ready
                            ? "Monitoring"
                            : "Watching locally"
                          : bugMonitorStatus.readiness?.ingest_ready
                            ? "Ready"
                            : "Not ready"}
                      </Badge>
                      {bugMonitorPaused || bugMonitorStatus.runtime?.paused ? (
                        <Badge tone="warn">Paused</Badge>
                      ) : null}
                      <Badge tone="info">
                        {Number(bugMonitorStatus.runtime?.pending_incidents || 0)} incidents
                      </Badge>
                      <Badge tone="info">
                        {Number(bugMonitorStatus.pending_drafts || 0)} pending drafts
                      </Badge>
                      <Badge tone="info">
                        {Number(bugMonitorStatus.pending_posts || 0)} post attempts
                      </Badge>
                      <button
                        className="tcp-icon-btn"
                        title="Reload status"
                        aria-label="Reload status"
                        onClick={() =>
                          Promise.all([
                            bugMonitorStatusQuery.refetch(),
                            bugMonitorDraftsQuery.refetch(),
                            bugMonitorIncidentsQuery.refetch(),
                            bugMonitorPostsQuery.refetch(),
                          ]).then(() => toast("ok", "Bug Monitor status refreshed."))
                        }
                      >
                        <i data-lucide="refresh-cw"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title={
                          bugMonitorPaused || bugMonitorStatus.runtime?.paused
                            ? "Resume monitoring"
                            : "Pause monitoring"
                        }
                        aria-label={
                          bugMonitorPaused || bugMonitorStatus.runtime?.paused
                            ? "Resume monitoring"
                            : "Pause monitoring"
                        }
                        disabled={bugMonitorPauseResumeMutation.isPending}
                        onClick={() =>
                          bugMonitorPauseResumeMutation.mutate({
                            action:
                              bugMonitorPaused || bugMonitorStatus.runtime?.paused
                                ? "resume"
                                : "pause",
                          })
                        }
                      >
                        <i
                          data-lucide={
                            bugMonitorPaused || bugMonitorStatus.runtime?.paused ? "play" : "pause"
                          }
                        ></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Refresh capability bindings"
                        aria-label="Refresh capability bindings"
                        disabled={refreshBugMonitorBindingsMutation.isPending}
                        onClick={() => refreshBugMonitorBindingsMutation.mutate()}
                      >
                        <i data-lucide="rotate-cw"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Copy debug payload"
                        aria-label="Copy debug payload"
                        onClick={() => void copyBugMonitorDebugPayload()}
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
                          className={`tcp-list-item text-left ${bugMonitorEnabled ? "ring-1 ring-emerald-400/40" : ""}`}
                          onClick={() => setBugMonitorEnabled((prev) => !prev)}
                        >
                          <div className="font-medium">
                            {bugMonitorEnabled
                              ? bugMonitorPaused
                                ? "Paused"
                                : "Enabled"
                              : "Disabled"}
                          </div>
                          <div className="tcp-subtle text-xs">
                            {bugMonitorEnabled
                              ? bugMonitorPaused
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
                              const seed = String(bugMonitorWorkspaceRoot || "/").trim();
                              setBugMonitorWorkspaceBrowserDir(seed || "/");
                              setBugMonitorWorkspaceBrowserSearch("");
                              setBugMonitorWorkspaceBrowserOpen(true);
                            }}
                          >
                            <i data-lucide="folder-open"></i>
                            Browse
                          </button>
                          <input
                            className="tcp-input"
                            readOnly
                            value={bugMonitorWorkspaceRoot}
                            placeholder="No local directory selected. Use Browse."
                          />
                          <button
                            className="tcp-btn"
                            type="button"
                            onClick={() => setBugMonitorWorkspaceRoot("")}
                            disabled={!bugMonitorWorkspaceRoot}
                          >
                            <i data-lucide="x"></i>
                            Clear
                          </button>
                        </div>
                        <div className="tcp-subtle text-xs">
                          {bugMonitorWorkspaceRoot
                            ? `Reporter analysis root: ${bugMonitorWorkspaceRoot}`
                            : "Defaults to the engine workspace root if not set."}
                        </div>
                      </label>

                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          Target repo
                        </span>
                        <input
                          className="tcp-input"
                          value={bugMonitorRepo}
                          onChange={(event) => setBugMonitorRepo(event.target.value)}
                          placeholder="owner/repo"
                        />
                      </label>

                      <label className="grid gap-2">
                        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                          MCP server
                        </span>
                        <select
                          className="tcp-input"
                          value={bugMonitorMcpServer}
                          onChange={(event) => setBugMonitorMcpServer(event.target.value)}
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
                          value={bugMonitorProviderPreference}
                          onChange={(event) => setBugMonitorProviderPreference(event.target.value)}
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
                          value={bugMonitorProviderId}
                          onChange={(event) => {
                            const nextProvider = event.target.value;
                            setBugMonitorProviderId(nextProvider);
                            setBugMonitorModelId("");
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
                          value={bugMonitorModelId}
                          onChange={(event) => setBugMonitorModelId(event.target.value)}
                          list="bug-monitor-models"
                          disabled={!bugMonitorProviderId}
                          placeholder={
                            bugMonitorProviderId
                              ? "Type or paste a model id"
                              : "Choose a provider first"
                          }
                          spellCheck={false}
                        />
                        <datalist id="bug-monitor-models">
                          {bugMonitorProviderModels.map((modelId) => (
                            <option key={modelId} value={modelId} />
                          ))}
                        </datalist>
                        <div className="tcp-subtle text-xs">
                          {bugMonitorProviderId
                            ? bugMonitorProviderModels.length
                              ? `${bugMonitorProviderModels.length} suggested models from provider catalog`
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
                            className={`tcp-list-item text-left ${bugMonitorAutoCreateIssues && !bugMonitorRequireApproval ? "ring-1 ring-emerald-400/40" : ""}`}
                            onClick={() => {
                              setBugMonitorAutoCreateIssues((prev) => !prev);
                              if (bugMonitorRequireApproval && bugMonitorAutoCreateIssues) {
                                setBugMonitorRequireApproval(false);
                              }
                            }}
                          >
                            <div className="font-medium">Auto-create new issues</div>
                            <div className="tcp-subtle text-xs">
                              {bugMonitorAutoCreateIssues
                                ? "New drafts post to GitHub automatically."
                                : "New drafts stay internal until published manually."}
                            </div>
                          </button>
                          <button
                            type="button"
                            className={`tcp-list-item text-left ${bugMonitorRequireApproval ? "ring-1 ring-amber-400/40" : ""}`}
                            onClick={() => {
                              setBugMonitorRequireApproval((prev) => {
                                const next = !prev;
                                if (next) setBugMonitorAutoCreateIssues(false);
                                return next;
                              });
                            }}
                          >
                            <div className="font-medium">Require approval</div>
                            <div className="tcp-subtle text-xs">
                              {bugMonitorRequireApproval
                                ? "New drafts wait for a manual publish click."
                                : "Approval gate disabled."}
                            </div>
                          </button>
                          <button
                            type="button"
                            className={`tcp-list-item text-left ${bugMonitorAutoComment ? "ring-1 ring-sky-400/40" : ""}`}
                            onClick={() => setBugMonitorAutoComment((prev) => !prev)}
                          >
                            <div className="font-medium">Auto-comment matches</div>
                            <div className="tcp-subtle text-xs">
                              {bugMonitorAutoComment
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
                        disabled={saveBugMonitorMutation.isPending}
                        title="Save Bug Monitor settings"
                        aria-label="Save Bug Monitor settings"
                        onClick={() => saveBugMonitorMutation.mutate()}
                      >
                        <i data-lucide="save"></i>
                        {saveBugMonitorMutation.isPending ? "Saving..." : null}
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
                        disabled={refreshBugMonitorBindingsMutation.isPending}
                        onClick={() => refreshBugMonitorBindingsMutation.mutate()}
                      >
                        <i data-lucide="rotate-cw"></i>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        title="Copy debug payload"
                        aria-label="Copy debug payload"
                        onClick={() => void copyBugMonitorDebugPayload()}
                      >
                        <i data-lucide="copy"></i>
                      </button>
                      {selectedBugMonitorServer ? (
                        <button
                          className="tcp-icon-btn"
                          title={
                            selectedBugMonitorServer.connected
                              ? "Refresh selected MCP"
                              : "Connect selected MCP"
                          }
                          aria-label={
                            selectedBugMonitorServer.connected
                              ? "Refresh selected MCP"
                              : "Connect selected MCP"
                          }
                          disabled={mcpActionMutation.isPending}
                          onClick={() =>
                            mcpActionMutation.mutate({
                              action: selectedBugMonitorServer.connected ? "refresh" : "connect",
                              server: selectedBugMonitorServer,
                            })
                          }
                        >
                          <i
                            data-lucide={
                              selectedBugMonitorServer.connected ? "refresh-cw" : "plug-zap"
                            }
                          ></i>
                        </button>
                      ) : null}
                    </div>

                    <div className="grid gap-3 md:grid-cols-3">
                      <div className="tcp-list-item">
                        <div className="text-sm font-medium">Readiness</div>
                        <div className="mt-1 text-sm">
                          {bugMonitorStatus.runtime?.monitoring_active
                            ? bugMonitorStatus.readiness?.publish_ready
                              ? "Monitoring"
                              : "Watching locally"
                            : bugMonitorStatus.runtime?.paused || bugMonitorPaused
                              ? "Paused"
                              : bugMonitorStatus.readiness?.ingest_ready
                                ? "Ready"
                                : "Blocked"}
                        </div>
                        <div className="tcp-subtle text-xs">
                          {bugMonitorStatus.runtime?.last_runtime_error ||
                            bugMonitorStatus.last_error ||
                            "No blocking issue reported."}
                        </div>
                        {!bugMonitorStatus.readiness?.publish_ready &&
                        Array.isArray(bugMonitorStatus.missing_required_capabilities) &&
                        bugMonitorStatus.missing_required_capabilities.length ? (
                          <div className="tcp-subtle mt-2 text-xs">
                            Missing: {bugMonitorStatus.missing_required_capabilities.join(", ")}
                          </div>
                        ) : null}
                      </div>
                      <div className="tcp-list-item">
                        <div className="text-sm font-medium">Selected MCP</div>
                        <div className="mt-1 text-sm">
                          {selectedBugMonitorServer?.name || "None selected"}
                        </div>
                        <div className="tcp-subtle text-xs">
                          {selectedBugMonitorServer
                            ? selectedBugMonitorServer.connected
                              ? "Connected"
                              : "Disconnected"
                            : "No server selected"}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          Bindings: {bugMonitorStatus.binding_source_version || "unknown version"}
                          {bugMonitorStatus.bindings_last_merged_at_ms
                            ? ` · merged ${new Date(bugMonitorStatus.bindings_last_merged_at_ms).toLocaleString()}`
                            : ""}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          Local directory:{" "}
                          {bugMonitorWorkspaceRoot ||
                            String(bugMonitorStatus.config?.workspace_root || "").trim() ||
                            "engine workspace root"}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          Last event:{" "}
                          {String(
                            bugMonitorStatus.runtime?.last_incident_event_type || ""
                          ).trim() || "No incidents processed yet"}
                        </div>
                      </div>
                      <div className="tcp-list-item">
                        <div className="text-sm font-medium">Model route</div>
                        <div className="mt-1 break-all text-sm">
                          {bugMonitorStatus.selected_model?.provider_id &&
                          bugMonitorStatus.selected_model?.model_id
                            ? `${bugMonitorStatus.selected_model.provider_id} / ${bugMonitorStatus.selected_model.model_id}`
                            : "No dedicated model selected"}
                        </div>
                        <div className="tcp-subtle text-xs">
                          {bugMonitorStatus.readiness?.selected_model_ready
                            ? "Available"
                            : "Fail-closed when unavailable"}
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">
                          Last processed:{" "}
                          {bugMonitorStatus.runtime?.last_processed_at_ms
                            ? new Date(
                                Number(bugMonitorStatus.runtime.last_processed_at_ms)
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
                            {bugMonitorStatus.required_capabilities?.github_list_issues
                              ? "ready"
                              : "missing"}
                          </div>
                          <div>
                            github.get_issue:{" "}
                            {bugMonitorStatus.required_capabilities?.github_get_issue
                              ? "ready"
                              : "missing"}
                          </div>
                          <div>
                            github.create_issue:{" "}
                            {bugMonitorStatus.required_capabilities?.github_create_issue
                              ? "ready"
                              : "missing"}
                          </div>
                          <div>
                            github.comment_on_issue:{" "}
                            {bugMonitorStatus.required_capabilities?.github_comment_on_issue
                              ? "ready"
                              : "missing"}
                          </div>
                        </div>
                        {Array.isArray(bugMonitorStatus.resolved_capabilities) &&
                        bugMonitorStatus.resolved_capabilities.length ? (
                          <div className="tcp-subtle mt-3 grid gap-1 text-xs">
                            {bugMonitorStatus.resolved_capabilities.map((row, index) => (
                              <div key={`${row.capability_id || "cap"}-${index}`}>
                                {String(row.capability_id || "unknown")}:{" "}
                                {String(row.tool_name || "unresolved")}
                              </div>
                            ))}
                          </div>
                        ) : null}
                        {Array.isArray(bugMonitorStatus.selected_server_binding_candidates) &&
                        bugMonitorStatus.selected_server_binding_candidates.length ? (
                          <div className="tcp-subtle mt-3 grid gap-1 text-xs">
                            {bugMonitorStatus.selected_server_binding_candidates.map(
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
                        {Array.isArray(bugMonitorStatus.discovered_mcp_tools) &&
                        bugMonitorStatus.discovered_mcp_tools.length ? (
                          <div className="mt-3">
                            <div className="tcp-subtle text-xs font-medium">
                              Discovered MCP tools
                            </div>
                            <pre className="tcp-code mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs">
                              {bugMonitorStatus.discovered_mcp_tools.join("\n")}
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
                            {bugMonitorRequireApproval
                              ? "Manual publish"
                              : bugMonitorAutoCreateIssues
                                ? "Auto-create"
                                : "Internal draft only"}
                          </div>
                          <div>
                            Matched open issues:{" "}
                            {bugMonitorAutoComment ? "Auto-comment" : "Detect only"}
                          </div>
                          <div>Dedupe: Fingerprint marker + label</div>
                          <div>Labels: bug-monitor</div>
                          <div>Workspace write tools: Disabled</div>
                          <div>Model fallback: Fail closed</div>
                        </div>
                      </div>
                    </div>

                    <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                      <div className="mb-2 font-medium">Recent incidents</div>
                      {bugMonitorIncidents.length ? (
                        <div className="grid gap-2">
                          {bugMonitorIncidents.map((incident) => (
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
                                  disabled={bugMonitorReplayIncidentMutation.isPending}
                                  onClick={() =>
                                    bugMonitorReplayIncidentMutation.mutate({
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
                        <EmptyState text="No Bug Monitor incidents yet." />
                      )}
                    </div>

                    <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                      <div className="mb-2 font-medium">Recent reporter drafts</div>
                      {bugMonitorDrafts.length ? (
                        <div className="grid gap-2">
                          {bugMonitorDrafts.map((draft) => (
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
                                    disabled={bugMonitorDraftDecisionMutation.isPending}
                                    title="Approve draft"
                                    aria-label="Approve draft"
                                    onClick={() =>
                                      bugMonitorDraftDecisionMutation.mutate({
                                        draftId: draft.draft_id,
                                        decision: "approve",
                                      })
                                    }
                                  >
                                    <i data-lucide="check"></i>
                                    {bugMonitorDraftDecisionMutation.isPending
                                      ? "Updating..."
                                      : null}
                                  </button>
                                  <button
                                    className="tcp-icon-btn"
                                    title="Deny draft"
                                    aria-label="Deny draft"
                                    disabled={bugMonitorDraftDecisionMutation.isPending}
                                    onClick={() =>
                                      bugMonitorDraftDecisionMutation.mutate({
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
                                    disabled={bugMonitorPublishDraftMutation.isPending}
                                    onClick={() =>
                                      bugMonitorPublishDraftMutation.mutate({
                                        draftId: draft.draft_id,
                                      })
                                    }
                                  >
                                    <i data-lucide="bug-play"></i>
                                  </button>
                                  <button
                                    className="tcp-icon-btn"
                                    title="Recheck GitHub for an existing matching issue"
                                    aria-label="Recheck GitHub for an existing matching issue"
                                    disabled={bugMonitorRecheckMatchMutation.isPending}
                                    onClick={() =>
                                      bugMonitorRecheckMatchMutation.mutate({
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
                                    disabled={bugMonitorTriageRunMutation.isPending}
                                    onClick={() =>
                                      bugMonitorTriageRunMutation.mutate({
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
                        <EmptyState text="No Bug Monitor drafts yet." />
                      )}
                    </div>

                    <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                      <div className="mb-2 font-medium">Recent GitHub posts</div>
                      {bugMonitorPosts.length ? (
                        <div className="grid gap-2">
                          {bugMonitorPosts.map((post) => (
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
        />

        <DetailDrawer
          open={githubMcpGuideOpen}
          onClose={() => setGithubMcpGuideOpen(false)}
          title="Official GitHub MCP guide"
        >
          <div className="grid gap-3">
            <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm">
              Recommended for Bug Monitor: use the official GitHub MCP endpoint instead of a
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
                6. Save, connect, then select that MCP server in Bug Monitor settings.
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
          {bugMonitorWorkspaceBrowserOpen ? (
            <motion.div
              className="tcp-confirm-overlay"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <button
                type="button"
                className="tcp-confirm-backdrop"
                aria-label="Close Bug Monitor workspace dialog"
                onClick={() => {
                  setBugMonitorWorkspaceBrowserOpen(false);
                  setBugMonitorWorkspaceBrowserSearch("");
                }}
              />
              <motion.div
                className="tcp-confirm-dialog max-w-2xl"
                initial={{ opacity: 0, y: 8, scale: 0.98 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 6, scale: 0.98 }}
              >
                <h3 className="tcp-confirm-title">Select Bug Monitor Directory</h3>
                <p className="tcp-confirm-message">
                  Current: {bugMonitorCurrentBrowseDir || "n/a"}
                </p>
                <div className="mb-2 flex flex-wrap gap-2">
                  <button
                    className="tcp-btn"
                    onClick={() => {
                      if (!bugMonitorWorkspaceParentDir) return;
                      setBugMonitorWorkspaceBrowserDir(bugMonitorWorkspaceParentDir);
                    }}
                    disabled={!bugMonitorWorkspaceParentDir}
                  >
                    <i data-lucide="arrow-up-circle"></i>
                    Up
                  </button>
                  <button
                    className="tcp-btn-primary"
                    onClick={() => {
                      if (!bugMonitorCurrentBrowseDir) return;
                      setBugMonitorWorkspaceRoot(bugMonitorCurrentBrowseDir);
                      setBugMonitorWorkspaceBrowserOpen(false);
                      setBugMonitorWorkspaceBrowserSearch("");
                      toast("ok", `Bug Monitor directory selected: ${bugMonitorCurrentBrowseDir}`);
                    }}
                  >
                    <i data-lucide="badge-check"></i>
                    Select This Folder
                  </button>
                  <button
                    className="tcp-btn"
                    onClick={() => {
                      setBugMonitorWorkspaceBrowserOpen(false);
                      setBugMonitorWorkspaceBrowserSearch("");
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
                    value={bugMonitorWorkspaceBrowserSearch}
                    onInput={(e) =>
                      setBugMonitorWorkspaceBrowserSearch((e.target as HTMLInputElement).value)
                    }
                  />
                </div>
                <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
                  {filteredBugMonitorWorkspaceDirectories.length ? (
                    filteredBugMonitorWorkspaceDirectories.map((entry: any) => (
                      <button
                        key={String(entry?.path || entry?.name)}
                        className="tcp-list-item mb-1 w-full text-left"
                        onClick={() => setBugMonitorWorkspaceBrowserDir(String(entry?.path || ""))}
                      >
                        <i data-lucide="folder-open"></i>
                        {String(entry?.name || entry?.path || "")}
                      </button>
                    ))
                  ) : (
                    <EmptyState
                      text={
                        bugMonitorWorkspaceSearchQuery
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
