import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { JsonObject } from "@frumu/tandem-client";
import { renderMarkdownSafe } from "../lib/markdown";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
import { McpToolAllowlistEditor } from "../components/McpToolAllowlistEditor";
import {
  IncidentMonitorExternalProjectsPanel,
  type IncidentMonitorLogWatcherStatusDraft,
  type IncidentMonitorMonitoredProjectDraft,
  type IncidentMonitorProjectIntakeKeyDraft,
} from "../components/IncidentMonitorExternalProjectsPanel";
import { normalizeMcpNamespaceSegment } from "../features/mcp/mcpTools";
export { normalizeMcpNamespaceSegment };
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
import { getDefaultNavigationVisibility, saveNavigationVisibility } from "../app/navigation";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";
import type { NavigationVisibility } from "../app/navigation";
import type { RouteId } from "../app/routes";
import type { IconName } from "../ui/Icon";
import { buildPlannerProviderOptions } from "../features/planner/plannerShared";
import { parseSlackAllowedUsers, sameSlackAllowedUsers } from "./channelConnectionsModel.mjs";

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

export const HOSTED_CODER_REPO_ROOT = "/workspace/repos";
export const HOSTED_CODER_COMPAT_REPO_ROOT = "/workspace/aca/repos";
export const HOSTED_TANDEM_DATA_ROOT = "/workspace/tandem-data";

function repoNameFromSlug(repo: string): string {
  const parts = String(repo || "")
    .trim()
    .replace(/\.git$/i, "")
    .split(/[/:]/)
    .map((part) => part.trim())
    .filter(Boolean);
  return parts[parts.length - 1] || "";
}

function suggestedIncidentMonitorWorkspaceRoot(repo: string): string {
  const repoName = repoNameFromSlug(repo);
  return repoName ? `${HOSTED_CODER_REPO_ROOT}/${repoName}` : HOSTED_CODER_REPO_ROOT;
}

export function hostedWorkspaceDirectoryHint(path: string): string {
  const normalized = String(path || "").trim();
  if (normalized === HOSTED_CODER_REPO_ROOT) return "Shared Coder checkouts";
  if (normalized.startsWith(`${HOSTED_CODER_REPO_ROOT}/`)) return "Synced repo checkout";
  if (normalized === HOSTED_CODER_COMPAT_REPO_ROOT) return "Coder compatibility mount";
  if (normalized.startsWith(`${HOSTED_CODER_COMPAT_REPO_ROOT}/`)) return "Same repo via ACA path";
  if (normalized === HOSTED_TANDEM_DATA_ROOT) return "Runtime data, not source code";
  return "";
}

function incidentMonitorWorkspaceSetupWarning(workspaceRoot: string, repo: string): string {
  const root = String(workspaceRoot || "").trim();
  if (!root) return "Select the synced repo folder before enabling hosted Incident Monitor triage.";
  if (root === HOSTED_CODER_REPO_ROOT) {
    return `Select the repo folder under ${HOSTED_CODER_REPO_ROOT}, not the parent folder.`;
  }
  if (root === HOSTED_CODER_COMPAT_REPO_ROOT) {
    return `Select the repo folder under ${HOSTED_CODER_COMPAT_REPO_ROOT}, not the parent folder.`;
  }
  if (root === HOSTED_TANDEM_DATA_ROOT || root.startsWith(`${HOSTED_TANDEM_DATA_ROOT}/`)) {
    return `${HOSTED_TANDEM_DATA_ROOT} stores runtime state. Incident Monitor needs the source checkout under ${HOSTED_CODER_REPO_ROOT}.`;
  }
  const repoName = repoNameFromSlug(repo);
  if (
    repoName &&
    (root.startsWith(`${HOSTED_CODER_REPO_ROOT}/`) ||
      root.startsWith(`${HOSTED_CODER_COMPAT_REPO_ROOT}/`)) &&
    root.split("/").filter(Boolean).pop() !== repoName
  ) {
    return `Target repo looks like \`${repoName}\`, but the selected folder is \`${root}\`. Confirm this is the intended checkout.`;
  }
  return "";
}

type WorktreeCleanupActionRow = {
  path?: string;
  branch?: string | null;
  via?: string | null;
  code?: string | null;
  error?: string | null;
  stderr?: string | null;
  branch_deleted?: boolean | null;
  branch_delete_error?: string | null;
};

type WorktreeCleanupStaleRow = {
  path?: string;
  branch?: string | null;
};

type WorktreeCleanupResponse = {
  ok?: boolean;
  dry_run?: boolean;
  repo_root?: string;
  managed_root?: string;
  tracked_paths?: string[];
  active_paths?: string[];
  stale_paths?: WorktreeCleanupStaleRow[];
  cleaned_worktrees?: WorktreeCleanupActionRow[];
  orphan_dirs?: string[];
  orphan_dirs_removed?: WorktreeCleanupActionRow[];
  failures?: WorktreeCleanupActionRow[];
};

const PENDING_PROVIDER_OAUTH_STORAGE_KEY = "tandem_control_panel_pending_provider_oauth";

function loadPendingProviderOauthSessions() {
  if (typeof window === "undefined") return {} as Record<string, string>;
  try {
    const raw = window.sessionStorage.getItem(PENDING_PROVIDER_OAUTH_STORAGE_KEY) || "";
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    return Object.fromEntries(
      Object.entries(parsed)
        .map(([providerId, sessionId]) => [
          String(providerId || "")
            .trim()
            .toLowerCase(),
          String(sessionId || "").trim(),
        ])
        .filter(([providerId, sessionId]) => !!providerId && !!sessionId)
    );
  } catch {
    return {};
  }
}

function savePendingProviderOauthSessions(sessions: Record<string, string>) {
  if (typeof window === "undefined") return;
  try {
    const normalized = Object.fromEntries(
      Object.entries(sessions || {})
        .map(([providerId, sessionId]) => [
          String(providerId || "")
            .trim()
            .toLowerCase(),
          String(sessionId || "").trim(),
        ])
        .filter(([providerId, sessionId]) => !!providerId && !!sessionId)
    );
    if (!Object.keys(normalized).length) {
      window.sessionStorage.removeItem(PENDING_PROVIDER_OAUTH_STORAGE_KEY);
      return;
    }
    window.sessionStorage.setItem(PENDING_PROVIDER_OAUTH_STORAGE_KEY, JSON.stringify(normalized));
  } catch {
    // ignore storage failures
  }
}

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
  | "incident_monitor"
  | "browser"
  | "maintenance";

const SETTINGS_SECTIONS: SettingsSection[] = [
  "install",
  "navigation",
  "providers",
  "search",
  "scheduler",
  "identity",
  "theme",
  "channels",
  "mcp",
  "incident_monitor",
  "browser",
  "maintenance",
];

function normalizeSettingsSection(value: string | null | undefined): SettingsSection | null {
  const normalized = String(value || "")
    .trim()
    .toLowerCase()
    .replace(/-/g, "_");
  return SETTINGS_SECTIONS.includes(normalized as SettingsSection)
    ? (normalized as SettingsSection)
    : null;
}

function settingsSectionFromHash(): SettingsSection | null {
  if (typeof window === "undefined") return null;
  const raw = String(window.location.hash || "");
  const query = raw.includes("?") ? raw.slice(raw.indexOf("?") + 1) : "";
  if (!query) return null;
  const params = new URLSearchParams(query);
  return normalizeSettingsSection(params.get("section") || params.get("tab"));
}

function writeSettingsSectionHash(section: SettingsSection) {
  if (typeof window === "undefined") return;
  const next = `#/settings?section=${section}`;
  if (window.location.hash !== next) window.history.replaceState(null, "", next);
}

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

type InstallProfileResponse = {
  control_panel_mode?: string;
  control_panel_mode_source?: string;
  control_panel_mode_reason?: string;
  aca_integration?: boolean;
  control_panel_config_path?: string;
  control_panel_config_ready?: boolean;
  control_panel_config_missing?: string[];
  control_panel_compact_nav?: boolean;
  hosted_managed?: boolean;
  hosted_provider?: string;
  hosted_deployment_id?: string;
  hosted_deployment_slug?: string;
  hosted_hostname?: string;
  hosted_public_url?: string;
  hosted_control_plane_url?: string;
  hosted_auth_available?: boolean;
  hosted_release_version?: string;
  hosted_release_channel?: string;
  hosted_update_policy?: string;
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
  monitored_projects?: IncidentMonitorMonitoredProjectDraft[];
  destinations?: Array<Record<string, any>>;
  routes?: Array<Record<string, any>>;
  default_destination_ids?: string[];
  safety_defaults?: Record<string, any> | null;
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
  log_watcher?: IncidentMonitorLogWatcherStatusDraft;
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
  destinations?: Array<Record<string, any>>;
  destination_readiness?: Array<Record<string, any>>;
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
  strict_kb_grounding?: boolean;
  guild_id?: string;
  channel_id?: string;
  model_provider_id?: string;
  model_id?: string;
  style_profile?: string;
  security_profile?: string;
  // Slack-only snapshot fields (TAN-763): installation identity, signing
  // secret presence, and the per-connection summary rows.
  team_id?: string;
  app_id?: string;
  has_signing_secret?: boolean;
  connections_summary?: unknown[];
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
  strictKbGrounding: boolean;
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
  enabled_mcp_tools: string[];
};

type ChannelScopeRow = {
  scope_id: string;
  scope_kind: string;
  session_count: number;
  sender_count: number;
  last_seen_at_ms: number;
};

type ChannelScopesResponse = {
  channel?: string;
  scopes?: ChannelScopeRow[];
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

export const OPENAI_CODEX_PROVIDER_ID = "openai-codex";
export const OPENAI_CODEX_DEFAULT_MODEL_ID = "gpt-5.5";
export const CHANNEL_NAMES = ["telegram", "discord", "slack"] as const;

function isInternalConfigProviderId(providerId: string) {
  const normalized = String(providerId || "")
    .trim()
    .toLowerCase();
  return normalized.startsWith("mcp_header::") || normalized.startsWith("channel::");
}

export const CHANNEL_TOOL_GROUPS = [
  { label: "File", tools: ["read", "glob", "ls", "list", "grep", "codesearch", "search"] },
  { label: "Web", tools: ["websearch", "webfetch", "webfetch_html"] },
  { label: "Terminal", tools: ["bash", "write", "edit", "apply_patch"] },
  { label: "Memory", tools: ["memory_search", "memory_store", "memory_list"] },
  { label: "Other", tools: ["skill", "task", "question", "pack_builder"] },
] as const;
export const WORKFLOW_PLANNER_PSEUDO_TOOL = "tandem.workflow_planner";
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
  authKind: string;
  connected: boolean;
  enabled: boolean;
  lastError: string;
  lastAuthChallenge?: {
    message?: string;
    authorization_url?: string;
    authorizationUrl?: string;
  } | null;
  authorizationUrl?: string;
  headers: Record<string, string>;
  toolCache: any[];
  allowedTools: string[] | null;
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
  authKind: string;
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

export function normalizeMcpName(raw: string) {
  const cleaned = String(raw || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return cleaned || "mcp-server";
}

export function inferMcpNameFromTransport(transport: string) {
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

export function isGithubCopilotMcpTransport(transport: string) {
  const url = parseUrl(transport);
  if (!url) return false;
  const host = String(url.hostname || "").toLowerCase();
  return host === "api.githubcopilot.com" || host.endsWith(".githubcopilot.com");
}

function isNotionMcpTransport(transport: string) {
  const url = parseUrl(transport);
  if (!url) return false;
  const host = String(url.hostname || "").toLowerCase();
  return host === "mcp.notion.com" || host.endsWith(".notion.com");
}

function getMcpOauthGuidance(name: string, transport: string) {
  const normalizedName = String(name || "")
    .trim()
    .toLowerCase();
  if (normalizedName === "notion" || isNotionMcpTransport(transport)) {
    return "Notion uses browser OAuth. Save the server, finish the Notion sign-in in your browser, then come back and click Mark sign-in complete. Token paste is not the right flow for this MCP.";
  }
  return "This MCP server uses browser OAuth. Save it, complete the authorization page that opens in your browser, then come back and finish the connection in Tandem.";
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
  if (authMode === "oauth") return {};
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
  if (authMode === "oauth") {
    return "OAuth handoff: Tandem will open the server authorization flow on connect.";
  }
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
  const lastAuthChallenge = row.last_auth_challenge || row.lastAuthChallenge || null;
  return {
    name,
    transport: String(row.transport || "").trim(),
    authKind: String(row.auth_kind || row.authKind || "")
      .trim()
      .toLowerCase(),
    connected: !!row.connected,
    enabled: row.enabled !== false,
    lastError: String(row.last_error || row.lastError || "").trim(),
    lastAuthChallenge:
      lastAuthChallenge && typeof lastAuthChallenge === "object" ? lastAuthChallenge : null,
    authorizationUrl: String(row.authorization_url || row.authorizationUrl || "").trim(),
    headers: row.headers && typeof row.headers === "object" ? row.headers : {},
    toolCache: Array.isArray(row.tool_cache || row.toolCache)
      ? row.tool_cache || row.toolCache
      : [],
    allowedTools: Array.isArray(row.allowed_tools || row.allowedTools)
      ? (row.allowed_tools || row.allowedTools)
          .map((entry: any) => String(entry || "").trim())
          .filter(Boolean)
      : null,
  };
}

export function inferMcpCatalogAuthKind(catalog: McpCatalog, name: string, transport: string) {
  const normalizedName = String(name || "")
    .trim()
    .toLowerCase();
  const normalizedTransport = String(transport || "")
    .trim()
    .replace(/\/+$/, "")
    .toLowerCase();
  const match = catalog.servers.find((row) => {
    const rowTransport = String(row.transportUrl || "")
      .trim()
      .replace(/\/+$/, "")
      .toLowerCase();
    const rowSlug = String(row.slug || "")
      .trim()
      .toLowerCase();
    const rowConfigName = String(row.serverConfigName || "")
      .trim()
      .toLowerCase();
    return (
      (normalizedTransport && rowTransport && rowTransport === normalizedTransport) ||
      (normalizedTransport && rowTransport && rowTransport.includes(normalizedTransport)) ||
      (normalizedTransport && rowTransport && normalizedTransport.includes(rowTransport)) ||
      (!!normalizedName && (rowSlug === normalizedName || rowConfigName === normalizedName))
    );
  });
  return String(match?.authKind || "")
    .trim()
    .toLowerCase();
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

export function normalizeMcpTools(raw: any): string[] {
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
          authKind: String(row.auth_kind || row.authKind || "")
            .trim()
            .toLowerCase(),
        };
      })
      .filter((row): row is McpCatalogServer => !!row && !!row.slug && !!row.transportUrl)
      .sort((a, b) => a.name.localeCompare(b.name)),
  };
}

export function normalizeChannelDraft(
  channel: string,
  config: ChannelConfigRow | null | undefined
): ChannelDraft {
  const row = config && typeof config === "object" ? config : {};
  return {
    botToken: "",
    allowedUsers: Array.isArray(row.allowed_users) ? row.allowed_users.join(", ") : "",
    mentionOnly: row.mention_only !== false && channel === "discord" ? true : !!row.mention_only,
    strictKbGrounding: !!row.strict_kb_grounding,
    guildId: String(row.guild_id || "").trim(),
    channelId: String(row.channel_id || "").trim(),
    modelProviderId: String(row.model_provider_id || "").trim(),
    modelId: String(row.model_id || "").trim(),
    styleProfile: String(row.style_profile || "default").trim() || "default",
    securityProfile: String(row.security_profile || "operator").trim() || "operator",
  };
}

export function defaultChannelToolPreferences(): ChannelToolPreferencesRow {
  return {
    enabled_tools: [],
    disabled_tools: [],
    enabled_mcp_servers: [],
    enabled_mcp_tools: [],
  };
}

export function uniqueChannelValues(values: string[]) {
  return Array.from(new Set(values.map((value) => value.trim()).filter(Boolean)));
}

export function normalizeChannelToolPreferences(raw: any): ChannelToolPreferencesRow {
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
    enabled_mcp_tools: Array.isArray(row.enabled_mcp_tools)
      ? uniqueChannelValues(row.enabled_mcp_tools.map((value: any) => String(value)))
      : [],
  };
}

function normalizeChannelScopes(raw: any): ChannelScopeRow[] {
  const payload = raw && typeof raw === "object" ? raw : {};
  const rows = Array.isArray(payload.scopes) ? payload.scopes : [];
  return rows
    .map((value: any) => {
      if (!value || typeof value !== "object") return null;
      const scopeId = String(value.scope_id || value.scopeId || "").trim();
      if (!scopeId) return null;
      return {
        scope_id: scopeId,
        scope_kind: String(value.scope_kind || value.scopeKind || "").trim(),
        session_count: Number.isFinite(Number(value.session_count))
          ? Number(value.session_count)
          : Number.isFinite(Number(value.sessionCount))
            ? Number(value.sessionCount)
            : 0,
        sender_count: Number.isFinite(Number(value.sender_count))
          ? Number(value.sender_count)
          : Number.isFinite(Number(value.senderCount))
            ? Number(value.senderCount)
            : 0,
        last_seen_at_ms: Number.isFinite(Number(value.last_seen_at_ms))
          ? Number(value.last_seen_at_ms)
          : Number.isFinite(Number(value.lastSeenAtMs))
            ? Number(value.lastSeenAtMs)
            : 0,
      } satisfies ChannelScopeRow;
    })
    .filter((row): row is ChannelScopeRow => !!row)
    .sort((left, right) => {
      if (left.last_seen_at_ms !== right.last_seen_at_ms) {
        return right.last_seen_at_ms - left.last_seen_at_ms;
      }
      return left.scope_id.localeCompare(right.scope_id);
    });
}

export function formatChannelScopeLabel(scope: ChannelScopeRow) {
  const parts: string[] = [];
  if (scope.scope_kind) parts.push(scope.scope_kind);
  parts.push(scope.scope_id);
  if (scope.session_count > 1) {
    parts.push(`${scope.session_count} sessions`);
  } else if (scope.session_count === 1) {
    parts.push("1 session");
  }
  return parts.join(" · ");
}

export function channelToolEnabled(prefs: ChannelToolPreferencesRow, tool: string) {
  const explicitEnabled = prefs.enabled_tools.filter(
    (entry) => entry !== WORKFLOW_PLANNER_PSEUDO_TOOL
  );
  if (tool === WORKFLOW_PLANNER_PSEUDO_TOOL) {
    if (prefs.disabled_tools.includes(tool)) return false;
    return prefs.enabled_tools.includes(tool);
  }
  if (prefs.disabled_tools.includes(tool)) return false;
  return explicitEnabled.length === 0 || explicitEnabled.includes(tool);
}

export function nextChannelToolPreferences(
  prefs: ChannelToolPreferencesRow,
  tool: string,
  enabled: boolean
): ChannelToolPreferencesRow {
  const disabled = prefs.disabled_tools.filter((entry) => entry !== tool);
  const explicitEnabled = prefs.enabled_tools.filter(
    (entry) => entry !== WORKFLOW_PLANNER_PSEUDO_TOOL
  );
  if (tool === WORKFLOW_PLANNER_PSEUDO_TOOL) {
    if (enabled) {
      return {
        ...prefs,
        disabled_tools: disabled,
        enabled_tools: uniqueChannelValues([...prefs.enabled_tools, tool]),
      };
    }
    return {
      ...prefs,
      disabled_tools: uniqueChannelValues([...disabled, tool]),
      enabled_tools: prefs.enabled_tools.filter((entry) => entry !== tool),
    };
  }
  if (enabled) {
    return {
      ...prefs,
      disabled_tools: disabled,
      enabled_tools:
        explicitEnabled.length > 0
          ? uniqueChannelValues([...prefs.enabled_tools, tool])
          : prefs.enabled_tools,
    };
  }
  return {
    ...prefs,
    disabled_tools: uniqueChannelValues([...disabled, tool]),
    enabled_tools:
      explicitEnabled.length > 0
        ? prefs.enabled_tools.filter((entry) => entry !== tool)
        : prefs.enabled_tools,
  };
}

export function nextChannelMcpPreferences(
  prefs: ChannelToolPreferencesRow,
  server: string,
  enabled: boolean
): ChannelToolPreferencesRow {
  const servers = prefs.enabled_mcp_servers.filter((entry) => entry !== server);
  const namespace = normalizeMcpNamespaceSegment(server);
  const prefix = `mcp.${namespace}.`;
  return {
    ...prefs,
    enabled_mcp_servers: enabled ? uniqueChannelValues([...servers, server]) : servers,
    enabled_mcp_tools: enabled
      ? prefs.enabled_mcp_tools
      : prefs.enabled_mcp_tools.filter((tool) => !tool.startsWith(prefix)),
  };
}

export function channelExactMcpToolsForServer(
  prefs: ChannelToolPreferencesRow,
  serverName: string,
  discoveredTools: string[]
) {
  if (!prefs.enabled_mcp_servers.includes(serverName)) return null;
  const namespace = normalizeMcpNamespaceSegment(serverName);
  const prefix = `mcp.${namespace}.`;
  const exactTools = prefs.enabled_mcp_tools.filter((tool) => tool.startsWith(prefix));
  if (!exactTools.length) return null;
  const discoveredUseNamespaced = discoveredTools.some((tool) => tool.startsWith(prefix));
  if (discoveredUseNamespaced) return exactTools;
  return exactTools.map((tool) => tool.slice(prefix.length)).filter(Boolean);
}

export function nextChannelExactMcpPreferences(
  prefs: ChannelToolPreferencesRow,
  serverName: string,
  discoveredTools: string[],
  selectedTools: string[] | null
): ChannelToolPreferencesRow {
  const namespace = normalizeMcpNamespaceSegment(serverName);
  const prefix = `mcp.${namespace}.`;
  if (!prefs.enabled_mcp_servers.includes(serverName)) {
    return {
      ...prefs,
      enabled_mcp_tools: prefs.enabled_mcp_tools.filter((tool) => !tool.startsWith(prefix)),
    };
  }
  const toNamespaced = (tool: string) => {
    const trimmed = String(tool || "").trim();
    if (!trimmed) return "";
    return trimmed.startsWith("mcp.") ? trimmed : `${prefix}${trimmed}`;
  };
  const discovered = uniqueChannelValues(discoveredTools.map(toNamespaced).filter(Boolean));
  const retained = prefs.enabled_mcp_tools.filter((tool) => !tool.startsWith(prefix));
  const nextSelection =
    selectedTools === null
      ? discovered
      : uniqueChannelValues(selectedTools.map(toNamespaced).filter(Boolean));
  return {
    ...prefs,
    enabled_mcp_tools: uniqueChannelValues([...retained, ...nextSelection]),
  };
}

export function toolAllowedForSecurityProfile(securityProfile: string, tool: string) {
  if (securityProfile !== "public_demo") return true;
  return PUBLIC_DEMO_ALLOWED_TOOLS.includes(tool as (typeof PUBLIC_DEMO_ALLOWED_TOOLS)[number]);
}

export function toolEnabledForSecurityProfile(
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
  channel: string,
  left: string[] | string | null | undefined,
  right: string[] | string | null | undefined
) {
  // Slack allowlists are faithful: blank (deny-all) and "*" (open-to-all)
  // are different values, and the dirty check must see them as such or the
  // Save button never enables for exactly that toggle.
  if (channel === "slack") return sameSlackAllowedUsers(left, right);
  const a = normalizeChannelAllowedUsers(left).slice().sort();
  const b = normalizeChannelAllowedUsers(right).slice().sort();
  if (a.length !== b.length) return false;
  return a.every((value, index) => value === b[index]);
}

export function channelConfigHasSavedSettings(
  channel: string,
  config: ChannelConfigRow | null | undefined
) {
  const row: ChannelConfigRow = config && typeof config === "object" ? config : {};
  const allowedUsers = normalizeChannelAllowedUsers(row.allowed_users);
  return (
    !!row.has_token ||
    // Connection-only Slack configs keep everything in `connections[]`
    // (reported as `connections_summary`) with no top-level channel or
    // token — they are saved settings all the same, so Remove must enable.
    (Array.isArray(row.connections_summary) && row.connections_summary.length > 0) ||
    !!row.has_signing_secret ||
    !!String(row.team_id || "").trim() ||
    !!String(row.app_id || "").trim() ||
    allowedUsers.some((user) => user !== "*") ||
    !!row.mention_only ||
    !!row.strict_kb_grounding ||
    !!String(row.guild_id || "").trim() ||
    !!String(row.channel_id || "").trim() ||
    !!String(row.model_provider_id || "").trim() ||
    !!String(row.model_id || "").trim() ||
    String(row.style_profile || "default").trim() !== "default" ||
    String(row.security_profile || "operator").trim() !== "operator"
  );
}

export function channelDraftMatchesConfig(
  channel: string,
  draft: ChannelDraft,
  config: ChannelConfigRow | null | undefined
) {
  const savedDraft = normalizeChannelDraft(channel, config);
  return (
    !String(draft.botToken || "").trim() &&
    sameChannelAllowedUsers(channel, draft.allowedUsers, savedDraft.allowedUsers) &&
    !!draft.mentionOnly === !!savedDraft.mentionOnly &&
    !!draft.strictKbGrounding === !!savedDraft.strictKbGrounding &&
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

export function providerCatalogBadge(provider: any, modelCount: number) {
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

export function providerCatalogSubtitle(provider: any, defaultModel: string) {
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
  files: "Managed files plus the hosted knowledgebase upload surface.",
  runs: "Live operations overview with queue state and per-run inspection.",
  settings: "Provider defaults, themes, and runtime diagnostics.",
};

export function useSettingsPageController({
  client,
  api,
  toast,
  navigate,
  currentRoute,
  providerStatus,
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
    () =>
      settingsSectionFromHash() ||
      (providerStatus?.needsOnboarding
        ? "providers"
        : navigation?.acaMode
          ? "navigation"
          : "install")
  );
  const [navigationVisibilityDraft, setNavigationVisibilityDraft] = useState<NavigationVisibility>(
    () => navigation?.routeVisibility || getDefaultNavigationVisibility(!!navigation?.acaMode)
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
  const [providerDefaultsOpen, setProviderDefaultsOpen] = useState(true);
  const [customProviderFormOpen, setCustomProviderFormOpen] = useState(false);
  const [oauthSessionByProvider, setOauthSessionByProvider] = useState<Record<string, string>>(() =>
    loadPendingProviderOauthSessions()
  );
  const [customProviderId, setCustomProviderId] = useState("custom");
  const [customProviderUrl, setCustomProviderUrl] = useState("");
  const [customProviderModel, setCustomProviderModel] = useState("");
  const [customProviderApiKey, setCustomProviderApiKey] = useState("");
  const [customProviderMakeDefault, setCustomProviderMakeDefault] = useState(true);
  const [codexAuthJsonText, setCodexAuthJsonText] = useState("");
  const [channelDrafts, setChannelDrafts] = useState<Record<string, ChannelDraft>>({});
  const channelDraftsHydratedRef = useRef<Record<string, boolean>>({
    telegram: false,
    discord: false,
    slack: false,
  });
  const [channelToolScopeSelection, setChannelToolScopeSelection] = useState<
    Record<string, string>
  >({
    telegram: "",
    discord: "",
    slack: "",
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
  const [incidentMonitorMonitoredProjectsJson, setIncidentMonitorMonitoredProjectsJson] =
    useState("[]");
  const [incidentMonitorMonitoredProjectsError, setIncidentMonitorMonitoredProjectsError] =
    useState("");
  const [incidentMonitorDestinationsJson, setIncidentMonitorDestinationsJson] = useState("[]");
  const [incidentMonitorRoutesJson, setIncidentMonitorRoutesJson] = useState("[]");
  const [incidentMonitorDefaultDestinationsText, setIncidentMonitorDefaultDestinationsText] =
    useState("");
  const [incidentMonitorSafetyDefaultsJson, setIncidentMonitorSafetyDefaultsJson] = useState("{}");
  const [incidentMonitorRoutingConfigError, setIncidentMonitorRoutingConfigError] = useState("");
  const [incidentMonitorRoutePreviewJson, setIncidentMonitorRoutePreviewJson] = useState(
    JSON.stringify({ source: "manual", risk_level: "medium" }, null, 2)
  );
  const [incidentMonitorRoutePreviewError, setIncidentMonitorRoutePreviewError] = useState("");
  const [incidentMonitorRoutePreviewResult, setIncidentMonitorRoutePreviewResult] = useState<
    any | null
  >(null);
  const [incidentMonitorCreatedIntakeKey, setIncidentMonitorCreatedIntakeKey] = useState("");
  const [incidentMonitorDisablingIntakeKeyId, setIncidentMonitorDisablingIntakeKeyId] =
    useState("");
  const [incidentMonitorResettingSourceKey, setIncidentMonitorResettingSourceKey] = useState("");
  const [incidentMonitorReplayingSourceKey, setIncidentMonitorReplayingSourceKey] = useState("");
  const [incidentMonitorLogSourceActionResult, setIncidentMonitorLogSourceActionResult] =
    useState<Record<string, unknown> | null>(null);
  const [worktreeCleanupRepoRoot, setWorktreeCleanupRepoRoot] = useState("");
  const [worktreeCleanupDryRun, setWorktreeCleanupDryRun] = useState(false);
  const [worktreeCleanupPulse, setWorktreeCleanupPulse] = useState(0);
  const [worktreeCleanupResult, setWorktreeCleanupResult] =
    useState<WorktreeCleanupResponse | null>(null);
  const [incidentMonitorWorkspaceBrowserOpen, setIncidentMonitorWorkspaceBrowserOpen] =
    useState(false);
  const [incidentMonitorWorkspaceBrowserDir, setIncidentMonitorWorkspaceBrowserDir] = useState("");
  const [incidentMonitorWorkspaceBrowserSearch, setIncidentMonitorWorkspaceBrowserSearch] =
    useState("");
  const avatarInputRef = useRef<HTMLInputElement | null>(null);
  const codexAuthInputRef = useRef<HTMLInputElement | null>(null);
  const [codexAuthFileName, setCodexAuthFileName] = useState("");
  useEffect(() => {
    savePendingProviderOauthSessions(oauthSessionByProvider);
  }, [oauthSessionByProvider]);

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

  useEffect(() => {
    if (currentRoute !== "settings") return;
    const applyHashSection = () => {
      const section = settingsSectionFromHash();
      if (section) setActiveSection(section);
    };
    applyHashSection();
    window.addEventListener("hashchange", applyHashSection);
    return () => window.removeEventListener("hashchange", applyHashSection);
  }, [currentRoute]);

  const installProfileQuery = useQuery({
    queryKey: ["settings", "install", "profile"],
    queryFn: () =>
      (api("/api/install/profile", { method: "GET" }) as Promise<InstallProfileResponse>).catch(
        () => null
      ),
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
    if (settingsSectionFromHash()) return;
    if (providerStatus?.needsOnboarding) {
      setActiveSection("providers");
      setProviderDefaultsOpen(true);
      return;
    }
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
    providerStatus?.needsOnboarding,
    installProfileQuery.data?.aca_integration,
    installProfileQuery.data?.control_panel_config_ready,
  ]);

  useEffect(() => {
    if (activeSection === "providers") {
      setProviderDefaultsOpen(true);
    }
  }, [activeSection]);

  const providerCatalogEnabled =
    activeSection === "providers" ||
    activeSection === "channels" ||
    activeSection === "incident_monitor";

  const providersCatalog = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    enabled: providerCatalogEnabled,
    staleTime: 5 * 60 * 1000,
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
        includeUnconfiguredProviders: true,
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
  const hostedManaged = installProfileQuery.data?.hosted_managed === true;
  useEffect(() => {
    const workspaceRoot = String(systemHealthQuery.data?.workspace_root || "").trim();
    if (!workspaceRoot) return;
    setWorktreeCleanupRepoRoot((current) => (current.trim() ? current : workspaceRoot));
  }, [systemHealthQuery.data?.workspace_root]);
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
          return (
            normalized &&
            !BUILTIN_PROVIDER_IDS.has(normalized) &&
            !isInternalConfigProviderId(normalized)
          );
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
  const worktreeCleanupMutation = useMutation({
    mutationFn: (payload: { repoRoot?: string; dryRun?: boolean }) =>
      api("/api/engine/worktree/cleanup", {
        method: "POST",
        body: JSON.stringify({
          repo_root: payload.repoRoot,
          dry_run: payload.dryRun,
          remove_orphan_dirs: true,
        }),
      }) as Promise<WorktreeCleanupResponse>,
    onMutate: () => {
      setWorktreeCleanupResult(null);
    },
    onSuccess: (result) => {
      setWorktreeCleanupResult(result);
      const removedCount =
        (result.cleaned_worktrees?.length || 0) + (result.orphan_dirs_removed?.length || 0);
      const failureCount = result.failures?.length || 0;
      toast(
        failureCount ? "warn" : result.dry_run ? "info" : "ok",
        result.dry_run
          ? `Found ${result.stale_paths?.length || 0} stale worktrees and ${
              result.orphan_dirs?.length || 0
            } orphan directories.`
          : failureCount
            ? `Cleanup removed ${removedCount} entries with ${failureCount} failures.`
            : `Cleanup removed ${removedCount} stale worktree entries.`
      );
    },
    onError: (error) => {
      setWorktreeCleanupResult(null);
      toast("err", error instanceof Error ? error.message : String(error));
    },
  });
  useEffect(() => {
    if (!worktreeCleanupMutation.isPending) {
      setWorktreeCleanupPulse(0);
      return;
    }
    const timer = window.setInterval(() => {
      setWorktreeCleanupPulse((value) => value + 1);
    }, 800);
    return () => window.clearInterval(timer);
  }, [worktreeCleanupMutation.isPending]);
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
  const incidentMonitorIntakeKeysQuery = useQuery({
    queryKey: ["settings", "incident-monitor", "intake-keys"],
    queryFn: () =>
      api("/api/engine/incident-monitor/intake/keys", { method: "GET" }).catch(() => ({
        keys: [],
      })),
    refetchInterval: 30_000,
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
  const channelScopesQuery = useQuery({
    queryKey: ["settings", "channels", "scopes"],
    queryFn: async () => {
      const entries = await Promise.all(
        CHANNEL_NAMES.map(async (channel) => {
          const result = (await api(`/api/engine/channels/${channel}/scopes`, {
            method: "GET",
          }).catch(() => ({ scopes: [] }))) as ChannelScopesResponse;
          return [channel, normalizeChannelScopes(result)] as const;
        })
      );
      return Object.fromEntries(entries) as Record<string, ChannelScopeRow[]>;
    },
    refetchInterval: 15_000,
  });
  const channelToolPreferencesQuery = useQuery({
    queryKey: ["settings", "channels", "tool-preferences", channelToolScopeSelection],
    queryFn: async () => {
      const entries = await Promise.all(
        CHANNEL_NAMES.map(async (channel) => {
          const scopeId = String(channelToolScopeSelection[channel] || "").trim();
          const query = scopeId ? `?scope_id=${encodeURIComponent(scopeId)}` : "";
          const prefs = normalizeChannelToolPreferences(
            await api(`/api/engine/channels/${channel}/tool-preferences${query}`, {
              method: "GET",
            }).catch(() => defaultChannelToolPreferences())
          );
          return [channel, prefs] as const;
        })
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
        OPENAI_CODEX_DEFAULT_MODEL_ID
    ).trim() || OPENAI_CODEX_DEFAULT_MODEL_ID;
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
  const saveIncidentMonitorMutation = useMutation({
    mutationFn: async () => {
      let monitoredProjects: IncidentMonitorMonitoredProjectDraft[] = [];
      let destinations: Array<Record<string, any>> = [];
      let routes: Array<Record<string, any>> = [];
      let safetyDefaults: Record<string, any> = {};
      try {
        const parsed = JSON.parse(incidentMonitorMonitoredProjectsJson || "[]");
        if (!Array.isArray(parsed)) {
          throw new Error("monitored_projects must be a JSON array");
        }
        monitoredProjects = parsed as IncidentMonitorMonitoredProjectDraft[];
        setIncidentMonitorMonitoredProjectsError("");
      } catch (error) {
        const message =
          error instanceof Error ? error.message : "monitored_projects JSON is invalid";
        setIncidentMonitorMonitoredProjectsError(message);
        throw new Error(message);
      }
      try {
        const parsedDestinations = JSON.parse(incidentMonitorDestinationsJson || "[]");
        if (!Array.isArray(parsedDestinations)) {
          throw new Error("destinations must be a JSON array");
        }
        destinations = parsedDestinations as Array<Record<string, any>>;

        const parsedRoutes = JSON.parse(incidentMonitorRoutesJson || "[]");
        if (!Array.isArray(parsedRoutes)) {
          throw new Error("routes must be a JSON array");
        }
        routes = parsedRoutes as Array<Record<string, any>>;

        const parsedSafetyDefaults = JSON.parse(incidentMonitorSafetyDefaultsJson || "{}");
        if (
          !parsedSafetyDefaults ||
          typeof parsedSafetyDefaults !== "object" ||
          Array.isArray(parsedSafetyDefaults)
        ) {
          throw new Error("safety_defaults must be a JSON object");
        }
        safetyDefaults = parsedSafetyDefaults as Record<string, any>;
        setIncidentMonitorRoutingConfigError("");
      } catch (error) {
        const message =
          error instanceof Error ? error.message : "Incident Monitor router JSON is invalid";
        setIncidentMonitorRoutingConfigError(message);
        throw new Error(message);
      }
      const defaultDestinationIds = String(incidentMonitorDefaultDestinationsText || "")
        .split(/[\s,]+/)
        .map((value) => value.trim())
        .filter(Boolean);
      return api("/api/engine/config/incident-monitor", {
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
            monitored_projects: monitoredProjects,
            destinations,
            routes,
            default_destination_ids: defaultDestinationIds,
            safety_defaults: safetyDefaults,
          },
        }),
      });
    },
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
  const incidentMonitorRoutePreviewMutation = useMutation({
    mutationFn: async () => {
      try {
        const parsed = JSON.parse(incidentMonitorRoutePreviewJson || "{}");
        if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
          throw new Error("Route preview input must be a JSON object");
        }
        setIncidentMonitorRoutePreviewError("");
        return api("/api/engine/incident-monitor/route-preview", {
          method: "POST",
          body: JSON.stringify(parsed),
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : "Route preview JSON is invalid";
        setIncidentMonitorRoutePreviewError(message);
        throw new Error(message);
      }
    },
    onSuccess: (payload) => {
      setIncidentMonitorRoutePreviewResult(payload);
      toast("ok", "Incident Monitor route preview updated.");
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      setIncidentMonitorRoutePreviewError(detail);
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
  const createIncidentMonitorIntakeKeyMutation = useMutation({
    mutationFn: async (input: { project_id: string; name: string }) =>
      api("/api/engine/incident-monitor/intake/keys", {
        method: "POST",
        body: JSON.stringify({
          project_id: input.project_id,
          name: input.name,
          scopes: ["incident_monitor:report"],
        }),
      }),
    onSuccess: async (payload: any) => {
      setIncidentMonitorCreatedIntakeKey(String(payload?.raw_key || ""));
      toast("ok", "Incident Monitor intake key created.");
      await queryClient.invalidateQueries({
        queryKey: ["settings", "incident-monitor", "intake-keys"],
      });
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
  });
  const disableIncidentMonitorIntakeKeyMutation = useMutation({
    mutationFn: async (keyId: string) => {
      setIncidentMonitorDisablingIntakeKeyId(keyId);
      return api(`/api/engine/incident-monitor/intake/keys/${encodeURIComponent(keyId)}/disable`, {
        method: "POST",
      });
    },
    onSuccess: async () => {
      toast("ok", "Incident Monitor intake key disabled.");
      await queryClient.invalidateQueries({
        queryKey: ["settings", "incident-monitor", "intake-keys"],
      });
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
    onSettled: () => setIncidentMonitorDisablingIntakeKeyId(""),
  });
  const resetIncidentMonitorLogSourceMutation = useMutation({
    mutationFn: async (input: { project_id: string; source_id: string }) => {
      const rowKey = `${input.project_id || "project"}::${input.source_id || "source"}`;
      setIncidentMonitorResettingSourceKey(rowKey);
      return api(
        `/api/engine/incident-monitor/log-sources/${encodeURIComponent(
          input.project_id
        )}/${encodeURIComponent(input.source_id)}/reset-offset`,
        { method: "POST" }
      );
    },
    onSuccess: async (payload: any) => {
      setIncidentMonitorLogSourceActionResult({
        action: "reset-offset",
        project_id: payload?.project_id,
        source_id: payload?.source_id,
        offset: payload?.state?.offset,
        path: payload?.state?.path,
        updated_at_ms: payload?.state?.updated_at_ms,
      });
      toast("ok", "Incident Monitor log source offset reset.");
      await queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] });
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
    onSettled: () => setIncidentMonitorResettingSourceKey(""),
  });
  const replayIncidentMonitorLogSourceMutation = useMutation({
    mutationFn: async (input: { project_id: string; source_id: string }) => {
      const rowKey = `${input.project_id || "project"}::${input.source_id || "source"}`;
      setIncidentMonitorReplayingSourceKey(rowKey);
      return api(
        `/api/engine/incident-monitor/log-sources/${encodeURIComponent(
          input.project_id
        )}/${encodeURIComponent(input.source_id)}/replay-latest`,
        { method: "POST" }
      );
    },
    onSuccess: async (payload: any) => {
      setIncidentMonitorLogSourceActionResult({
        action: "replay-latest",
        project_id: payload?.project_id,
        source_id: payload?.source_id,
        incident_id: payload?.incident?.incident_id,
        occurrence_count: payload?.incident?.occurrence_count,
        draft_id: payload?.draft?.draft_id,
        draft_status: payload?.draft?.status,
      });
      toast("ok", "Incident Monitor latest log candidate replayed.");
      await queryClient.invalidateQueries({ queryKey: ["settings", "incident-monitor"] });
    },
    onError: (error: any) => {
      const detail =
        error instanceof Error ? error.message : String(error?.detail || error?.error || error);
      toast("err", detail);
    },
    onSettled: () => setIncidentMonitorReplayingSourceKey(""),
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
  const importCodexAuthJsonMutation = useMutation({
    mutationFn: ({ providerId, authJson }: { providerId: string; authJson: string }) =>
      api(`/api/engine/provider/${encodeURIComponent(providerId)}/oauth/session/import`, {
        method: "POST",
        body: JSON.stringify({ auth_json: authJson }),
      }),
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
            "Unable to import the Codex auth.json file. Make sure it was copied from a signed-in Codex CLI session."
        );
        return;
      }
      setCodexAuthJsonText("");
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
            ? `Codex auth.json imported: ${email}`
            : "Codex auth.json imported."
          : `${providerId} auth.json imported.`
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
              : "Codex auth.json imported, but Tandem could not switch the default provider."
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
  const importCodexAuthFile = useCallback(
    async (providerId: string, file: File | null | undefined) => {
      if (!file) return;
      try {
        const text = await file.text();
        setCodexAuthFileName(file.name || "auth.json");
        setCodexAuthJsonText(text);
        if (!text.trim()) {
          toast("warn", "Selected auth.json file was empty.");
          return;
        }
        importCodexAuthJsonMutation.mutate({
          providerId,
          authJson: text,
        });
      } catch (error) {
        toast("err", error instanceof Error ? error.message : String(error));
      } finally {
        if (codexAuthInputRef.current) {
          codexAuthInputRef.current.value = "";
        }
      }
    },
    [importCodexAuthJsonMutation, toast]
  );
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
        queryClient.invalidateQueries({ queryKey: ["settings", "channels", "scopes"] }),
        queryClient.invalidateQueries({ queryKey: ["settings", "channels", "tool-preferences"] }),
      ]),
    [queryClient]
  );
  const saveChannelToolPreferencesMutation = useMutation({
    mutationFn: async ({
      channel,
      scopeId,
      payload,
    }: {
      channel: "telegram" | "discord" | "slack";
      scopeId?: string | null;
      payload: ChannelToolPreferencesRow | { reset: true };
    }) => {
      const scopeQuery = String(scopeId || "").trim()
        ? `?scope_id=${encodeURIComponent(String(scopeId || "").trim())}`
        : "";
      if ("reset" in payload) {
        return api(`/api/engine/channels/${channel}/tool-preferences${scopeQuery}`, {
          method: "PUT",
          body: JSON.stringify({ reset: true }),
        });
      }
      return api(`/api/engine/channels/${channel}/tool-preferences${scopeQuery}`, {
        method: "PUT",
        body: JSON.stringify(payload),
      });
    },
    onSuccess: async (_, vars) => {
      await queryClient.invalidateQueries({
        queryKey: ["settings", "channels", "tool-preferences"],
      });
      await queryClient.invalidateQueries({ queryKey: ["settings", "channels", "scopes"] });
      const scopeId = String(vars.scopeId || "").trim();
      toast(
        "ok",
        scopeId
          ? `Saved ${vars.channel} scope ${scopeId}.`
          : `Saved ${vars.channel} channel tool scope.`
      );
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
        // Slack allowlists are faithful: a blank field means deny-all on
        // signed ingress and must not be widened to "*" by a routine save.
        allowed_users:
          channel === "slack"
            ? parseSlackAllowedUsers(draft.allowedUsers)
            : parseAllowedUsers(draft.allowedUsers),
        mention_only: !!draft.mentionOnly,
        strict_kb_grounding: !!draft.strictKbGrounding,
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
        // Connection-only configs carry their channels in
        // `connections[]` (reported as `connections_summary`) with no
        // top-level channel_id; the server accepts and starts them, so
        // the save guard must not demand a legacy channel ID for them.
        const slackSnapshot = (channelsConfigQuery.data as any)?.slack;
        const hasConnections =
          Array.isArray(slackSnapshot?.connections_summary) &&
          slackSnapshot.connections_summary.length > 0;
        if (!channelId && !slackSnapshot?.channel_id && !hasConnections) {
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
      setChannelToolScopeSelection((prev) => ({ ...prev, [channel]: "" }));
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
  const mcpToolPolicyMutation = useMutation({
    mutationFn: async ({
      serverName,
      allowedTools,
    }: {
      serverName: string;
      allowedTools: string[] | null;
    }) =>
      client.mcp.patch(serverName, {
        allowed_tools: allowedTools ?? undefined,
        clear_allowed_tools: allowedTools === null,
      }),
    onSuccess: async () => {
      await invalidateMcp();
      toast("ok", "MCP tool access updated.");
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const mcpActionMutation = useMutation({
    mutationFn: async ({ action, server }: { action: string; server?: McpServerRow }) => {
      if (!server) throw new Error("No MCP server selected.");
      if (action === "connect") return client.mcp.connect(server.name);
      if (action === "disconnect") return client.mcp.disconnect(server.name);
      if (action === "refresh") return client.mcp.refresh(server.name);
      if (action === "authenticate")
        return api(`/api/engine/mcp/${encodeURIComponent(server.name)}/auth/authenticate`, {
          method: "POST",
        });
      if (action === "toggle-enabled")
        return (client.mcp as any).setEnabled(server.name, !server.enabled);
      if (action === "delete")
        return api(`/api/engine/mcp/${encodeURIComponent(server.name)}`, { method: "DELETE" });
      throw new Error(`Unknown action: ${action}`);
    },
    onSuccess: async (result, vars) => {
      await invalidateMcp();
      const pendingAuth =
        !!(result as any)?.pendingAuth ||
        !!(result as any)?.lastAuthChallenge ||
        !!(result as any)?.authorizationUrl;
      const actionOk = (result as any)?.ok !== false;
      const serverAuthKind = String(vars.server?.authKind || "")
        .trim()
        .toLowerCase();
      if (vars.action === "connect") {
        if (pendingAuth || (!actionOk && serverAuthKind === "oauth")) {
          const challenge = (result as any)?.lastAuthChallenge || {};
          const message = String(challenge?.message || "").trim();
          toast(
            "warn",
            message
              ? `OAuth authorization required for ${vars.server?.name}: ${message}`
              : `OAuth authorization required for ${vars.server?.name}.`
          );
        } else if (!actionOk) {
          const errorMessage = String(
            (result as any)?.error?.message || (result as any)?.error || ""
          ).trim();
          toast(
            "err",
            errorMessage
              ? `Failed to connect ${vars.server?.name}: ${errorMessage}`
              : `Failed to connect ${vars.server?.name}.`
          );
        } else {
          toast("ok", `Connected ${vars.server?.name}.`);
        }
      }
      if (vars.action === "disconnect") toast("ok", `Disconnected ${vars.server?.name}.`);
      if (vars.action === "refresh") {
        if (pendingAuth || (!actionOk && serverAuthKind === "oauth")) {
          const challenge = (result as any)?.lastAuthChallenge || {};
          const message = String(challenge?.message || "").trim();
          toast(
            "warn",
            message
              ? `OAuth authorization required for ${vars.server?.name}: ${message}`
              : `OAuth authorization required for ${vars.server?.name}.`
          );
        } else if (!actionOk) {
          const errorMessage = String(
            (result as any)?.error?.message || (result as any)?.error || ""
          ).trim();
          toast(
            "err",
            errorMessage
              ? `Failed to refresh ${vars.server?.name}: ${errorMessage}`
              : `Failed to refresh ${vars.server?.name}.`
          );
        } else {
          toast("ok", `Refreshed ${vars.server?.name}.`);
        }
      }
      if (vars.action === "authenticate") {
        const actionOk = (result as any)?.ok !== false;
        const errorMessage = String(
          (result as any)?.error?.message || (result as any)?.error || ""
        ).trim();
        if (!actionOk && !pendingAuth) {
          toast(
            "err",
            errorMessage
              ? `OAuth authorization check failed for ${vars.server?.name}: ${errorMessage}`
              : `OAuth authorization check failed for ${vars.server?.name}.`
          );
        } else if (pendingAuth) {
          const challenge = (result as any)?.lastAuthChallenge || {};
          const message = String(challenge?.message || "").trim();
          toast(
            "warn",
            message
              ? `OAuth authorization still pending for ${vars.server?.name}: ${message}`
              : `OAuth authorization still pending for ${vars.server?.name}.`
          );
        } else {
          toast("ok", `Marked ${vars.server?.name} as signed in.`);
        }
      }
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
        auth_kind: mcpAuthMode === "oauth" ? "oauth" : "",
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
        const result: any = await client.mcp.connect(payload.name);
        const pendingAuth =
          result?.pendingAuth === true || !!result?.lastAuthChallenge || !!result?.authorizationUrl;
        if (!result?.ok && !pendingAuth && mcpAuthMode !== "oauth") {
          throw new Error(`Added "${payload.name}" but connect failed.`);
        }
        return {
          name: payload.name,
          connectResult: result,
          connectAfterAdd: true,
          authKind: mcpAuthMode === "oauth" ? "oauth" : "",
        };
      }
      return {
        name: payload.name,
        connectAfterAdd: false,
        authKind: mcpAuthMode === "oauth" ? "oauth" : "",
      };
    },
    onSuccess: async (result) => {
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
      const serverName = String((result as any)?.name || "").trim();
      const connectResult = (result as any)?.connectResult;
      const authKind = String((result as any)?.authKind || "")
        .trim()
        .toLowerCase();
      const pendingAuth =
        !!connectResult?.pendingAuth ||
        !!connectResult?.lastAuthChallenge ||
        !!connectResult?.authorizationUrl;
      if ((result as any)?.connectAfterAdd && pendingAuth) {
        const challenge = connectResult?.lastAuthChallenge || {};
        const message = String(challenge?.message || "").trim();
        toast(
          "warn",
          message
            ? `Saved MCP "${serverName}" and OAuth authorization is still required: ${message}`
            : `Saved MCP "${serverName}" and OAuth authorization is still required.`
        );
      } else if ((result as any)?.connectAfterAdd) {
        if (authKind === "oauth" && connectResult && connectResult.ok !== true) {
          toast(
            "warn",
            `Saved MCP "${serverName}" as OAuth-backed. If it still needs authorization, open the auth link from the server row and refresh after signing in.`
          );
        } else {
          toast("ok", `Saved MCP "${serverName}" and connected it.`);
        }
      } else {
        toast("ok", `Saved MCP "${serverName}".`);
      }
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
  const connectedProviderCount = Array.isArray(providerStatus?.connected)
    ? providerStatus.connected.length
    : 0;

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
  useEffect(() => {
    const pendingServers = mcpServers.filter((server) => {
      const authKind = String(server.authKind || "")
        .trim()
        .toLowerCase();
      const challengeUrl = String(
        server.lastAuthChallenge?.authorization_url ||
          server.lastAuthChallenge?.authorizationUrl ||
          server.authorizationUrl ||
          ""
      ).trim();
      return authKind === "oauth" && (!!server.lastAuthChallenge || !!challengeUrl);
    });
    if (!pendingServers.length) return;

    let cancelled = false;
    let inFlight = false;

    const poll = async () => {
      if (cancelled || inFlight) return;
      inFlight = true;
      try {
        const results = await Promise.all(
          pendingServers.map(async (server) => {
            try {
              const payload = await api(
                `/api/engine/mcp/${encodeURIComponent(server.name)}/auth/authenticate`,
                {
                  method: "POST",
                }
              );
              return { payload, error: null as Error | null };
            } catch (error) {
              return {
                payload: null,
                error: error instanceof Error ? error : new Error(String(error)),
              };
            }
          })
        );

        if (cancelled) return;
        const completed = results.some(({ payload, error }) => {
          if (error) return false;
          return !(
            payload?.pendingAuth === true ||
            payload?.lastAuthChallenge ||
            payload?.authorizationUrl
          );
        });

        if (completed) {
          await queryClient.invalidateQueries({ queryKey: ["settings", "mcp"] });
        }
      } finally {
        inFlight = false;
      }
    };

    void poll();
    const timer = window.setInterval(() => {
      void poll();
    }, 3000);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [api, mcpServers, queryClient]);
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
  const incidentMonitorMonitoredProjects = useMemo(() => {
    try {
      const parsed = JSON.parse(incidentMonitorMonitoredProjectsJson || "[]");
      if (Array.isArray(parsed)) return parsed as IncidentMonitorMonitoredProjectDraft[];
    } catch {
      // The inline editor shows the parse error; keep rendering the last saved config.
    }
    return Array.isArray(incidentMonitorStatus.config?.monitored_projects)
      ? incidentMonitorStatus.config.monitored_projects
      : [];
  }, [incidentMonitorMonitoredProjectsJson, incidentMonitorStatus.config?.monitored_projects]);
  const incidentMonitorDestinations = useMemo(() => {
    try {
      const parsed = JSON.parse(incidentMonitorDestinationsJson || "[]");
      if (Array.isArray(parsed)) return parsed as Array<Record<string, any>>;
    } catch {
      // The inline editor shows the parse error; keep rendering the last saved config.
    }
    if (Array.isArray(incidentMonitorStatus.destinations))
      return incidentMonitorStatus.destinations;
    return Array.isArray(incidentMonitorStatus.config?.destinations)
      ? incidentMonitorStatus.config.destinations
      : [];
  }, [
    incidentMonitorDestinationsJson,
    incidentMonitorStatus.config?.destinations,
    incidentMonitorStatus.destinations,
  ]);
  const incidentMonitorRoutes = useMemo(() => {
    try {
      const parsed = JSON.parse(incidentMonitorRoutesJson || "[]");
      if (Array.isArray(parsed)) return parsed as Array<Record<string, any>>;
    } catch {
      // The inline editor shows the parse error; keep rendering the last saved config.
    }
    return Array.isArray(incidentMonitorStatus.config?.routes)
      ? incidentMonitorStatus.config.routes
      : [];
  }, [incidentMonitorRoutesJson, incidentMonitorStatus.config?.routes]);
  const incidentMonitorDestinationReadiness = useMemo(
    () =>
      Array.isArray(incidentMonitorStatus.destination_readiness)
        ? incidentMonitorStatus.destination_readiness
        : [],
    [incidentMonitorStatus.destination_readiness]
  );
  const incidentMonitorLogWatcher = useMemo(
    () => (incidentMonitorStatus.log_watcher || {}) as IncidentMonitorLogWatcherStatusDraft,
    [incidentMonitorStatus.log_watcher]
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
  const incidentMonitorIntakeKeys = useMemo(
    () =>
      Array.isArray((incidentMonitorIntakeKeysQuery.data as any)?.keys)
        ? ((incidentMonitorIntakeKeysQuery.data as any)
            .keys as IncidentMonitorProjectIntakeKeyDraft[]) || []
        : [],
    [incidentMonitorIntakeKeysQuery.data]
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
  const connectedChannelCount = CHANNEL_NAMES.filter(
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
  const incidentMonitorSuggestedWorkspaceRoot = useMemo(
    () => suggestedIncidentMonitorWorkspaceRoot(incidentMonitorRepo),
    [incidentMonitorRepo]
  );
  const incidentMonitorWorkspaceRootHint = hostedWorkspaceDirectoryHint(
    incidentMonitorWorkspaceRoot
  );
  const incidentMonitorWorkspaceSetupWarningText = incidentMonitorWorkspaceSetupWarning(
    incidentMonitorWorkspaceRoot,
    incidentMonitorRepo
  );

  useEffect(() => {
    const configPayload = incidentMonitorConfigQuery.data as any;
    const config =
      configPayload?.incident_monitor && typeof configPayload.incident_monitor === "object"
        ? (configPayload.incident_monitor as IncidentMonitorConfigRow)
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
    const monitoredProjects = Array.isArray(config.monitored_projects)
      ? config.monitored_projects
      : [];
    setIncidentMonitorMonitoredProjectsJson(JSON.stringify(monitoredProjects, null, 2));
    setIncidentMonitorMonitoredProjectsError("");
    setIncidentMonitorDestinationsJson(JSON.stringify(config.destinations || [], null, 2));
    setIncidentMonitorRoutesJson(JSON.stringify(config.routes || [], null, 2));
    setIncidentMonitorDefaultDestinationsText((config.default_destination_ids || []).join(", "));
    setIncidentMonitorSafetyDefaultsJson(JSON.stringify(config.safety_defaults || {}, null, 2));
    setIncidentMonitorRoutingConfigError("");
  }, [incidentMonitorConfigQuery.data]);

  useEffect(() => {
    const config =
      channelsConfigQuery.data && typeof channelsConfigQuery.data === "object"
        ? (channelsConfigQuery.data as Record<string, ChannelConfigRow>)
        : {};
    if (!channelsConfigQuery.data || typeof channelsConfigQuery.data !== "object") return;
    setChannelDrafts((prev) => {
      const next = { ...prev };
      for (const channel of CHANNEL_NAMES) {
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
      const challengeUrl = String(
        server.lastAuthChallenge?.authorization_url ||
          server.lastAuthChallenge?.authorizationUrl ||
          server.authorizationUrl ||
          ""
      ).trim();
      const customKeys = keys.filter(
        (key) =>
          ![authKey, apiKey, toolsetsKey]
            .filter(Boolean)
            .map((value) => String(value).toLowerCase())
            .includes(String(key).toLowerCase())
      );
      const serverAuthKind = String(server.authKind || "")
        .trim()
        .toLowerCase();
      const inferredAuthKind =
        serverAuthKind || inferMcpCatalogAuthKind(mcpCatalog, server.name, server.transport);
      let nextAuthMode = challengeUrl || inferredAuthKind === "oauth" ? "oauth" : "none";
      let nextCustomHeader = "";
      let nextToken = "";
      if (challengeUrl) {
        nextToken = "";
      } else if (apiKey) {
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

  const worktreeCleanupPendingMessages = [
    "Scanning registered Git worktrees...",
    "Comparing managed worktrees against live runtime records...",
    "Removing stale worktrees and orphan directories...",
  ];
  const worktreeCleanupPendingMessage =
    worktreeCleanupPendingMessages[worktreeCleanupPulse % worktreeCleanupPendingMessages.length];
  const worktreeCleanupActionRows = useMemo(() => {
    const rows: Array<{
      kind: "removed" | "orphan_removed" | "stale" | "active" | "failure";
      title: string;
      detail: string;
      tone: "ok" | "warn" | "err" | "info";
    }> = [];
    for (const row of worktreeCleanupResult?.cleaned_worktrees || []) {
      rows.push({
        kind: "removed",
        title: row.path || "Removed worktree",
        detail:
          row.branch && row.branch_deleted === false && row.branch_delete_error
            ? `Removed worktree, but branch cleanup failed: ${row.branch_delete_error}`
            : row.branch
              ? `Removed registered worktree and branch ${row.branch}.`
              : "Removed registered worktree.",
        tone: "ok",
      });
    }
    for (const path of worktreeCleanupResult?.active_paths || []) {
      rows.push({
        kind: "active",
        title: path,
        detail: "Skipped because the current Tandem runtime still tracks it as active.",
        tone: "info",
      });
    }
    for (const row of worktreeCleanupResult?.stale_paths || []) {
      const alreadyRemoved = (worktreeCleanupResult?.cleaned_worktrees || []).some(
        (cleaned) => cleaned.path === row.path
      );
      if (alreadyRemoved) continue;
      rows.push({
        kind: "stale",
        title: row.path || "Stale worktree",
        detail: worktreeCleanupResult?.dry_run
          ? row.branch
            ? `Would remove managed worktree and branch ${row.branch}.`
            : "Would remove managed worktree."
          : "Marked stale but not removed.",
        tone: "warn",
      });
    }
    for (const row of worktreeCleanupResult?.orphan_dirs_removed || []) {
      rows.push({
        kind: "orphan_removed",
        title: row.path || "Removed orphan directory",
        detail: "Removed an unregistered directory left behind under .tandem/worktrees.",
        tone: "ok",
      });
    }
    if (worktreeCleanupResult?.dry_run) {
      for (const path of worktreeCleanupResult?.orphan_dirs || []) {
        rows.push({
          kind: "stale",
          title: path,
          detail: "Would remove orphaned directory left on disk.",
          tone: "warn",
        });
      }
    }
    for (const row of worktreeCleanupResult?.failures || []) {
      rows.push({
        kind: "failure",
        title: row.path || row.code || "Cleanup failure",
        detail: row.error || row.stderr || row.branch_delete_error || "Cleanup failed.",
        tone: "err",
      });
    }
    return rows;
  }, [worktreeCleanupResult]);

  const sectionTabs: Array<{ id: SettingsSection; label: string; icon: IconName }> = [
    { id: "install", label: "Install", icon: "clipboard-list" },
    { id: "navigation", label: "Navigation", icon: "panel-left" },
    { id: "providers", label: "Providers", icon: "cpu" },
    { id: "search", label: "Web Search", icon: "globe" },
    { id: "scheduler", label: "Scheduler", icon: "layers" },
    { id: "identity", label: "Identity", icon: "badge-check" },
    { id: "theme", label: "Themes", icon: "paint-bucket" },
    { id: "channels", label: "Channels", icon: "message-circle" },
    { id: "mcp", label: "MCP", icon: "plug-zap" },
    { id: "incident_monitor", label: "Incident Monitor", icon: "shield-alert" },
    { id: "browser", label: "Browser", icon: "monitor-cog" },
    { id: "maintenance", label: "Maintenance", icon: "wrench" },
  ];
  const mcpAuthPreviewText = useMemo(
    () => mcpAuthPreview(mcpAuthMode, mcpToken, mcpCustomHeader, mcpTransport),
    [mcpAuthMode, mcpCustomHeader, mcpToken, mcpTransport]
  );
  const mcpOauthGuidanceText = useMemo(
    () => getMcpOauthGuidance(mcpName, mcpTransport),
    [mcpName, mcpTransport]
  );
  const mcpOauthStartsAfterSave = mcpAuthMode === "oauth" && mcpConnectAfterAdd;
  const mcpIsGithubTransport = useMemo(
    () => isGithubCopilotMcpTransport(mcpTransport),
    [mcpTransport]
  );
  const selectActiveSection = useCallback(
    (section: SettingsSection) => {
      setActiveSection(section);
      if (currentRoute === "settings") writeSettingsSectionHash(section);
    },
    [currentRoute]
  );
  useEffect(() => {
    if (!navigation?.routeVisibility) return;
    setNavigationVisibilityDraft(navigation.routeVisibility);
  }, [navigation?.routeVisibility]);
  const setNavigationRouteVisibility = useCallback(
    (routeId: RouteId, visible: boolean) => {
      setNavigationVisibilityDraft((current) => {
        const next = { ...current, [routeId]: visible };
        saveNavigationVisibility(next);
        return next;
      });
      navigation?.setRouteVisibility(routeId, visible);
    },
    [navigation]
  );
  const showAllNavigationSections = useCallback(() => {
    const next = Object.fromEntries(APP_NAV_ROUTES.map(([routeId]) => [routeId, true]));
    setNavigationVisibilityDraft(next as NavigationVisibility);
    saveNavigationVisibility(next as NavigationVisibility);
    navigation?.showAllSections();
  }, [navigation]);
  const resetNavigationSections = useCallback(() => {
    const next = getDefaultNavigationVisibility(!!navigation?.acaMode);
    setNavigationVisibilityDraft(next);
    saveNavigationVisibility(next);
    navigation?.resetNavigation();
  }, [navigation]);
  const navigationVisibility = navigationVisibilityDraft;
  const defaultNavigationVisibility = getDefaultNavigationVisibility(!!navigation?.acaMode);
  const navigationRows = APP_NAV_ROUTES.map(([routeId, label, icon]) => {
    const typedRouteId = routeId as RouteId;
    const enabled = navigationVisibility[typedRouteId] !== false;
    const defaultVisible = defaultNavigationVisibility[typedRouteId] !== false;
    return {
      routeId,
      label,
      icon: icon as IconName,
      enabled,
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

  return {
    activeSection: activeSection,
    advancedNavigationRows: advancedNavigationRows,
    api: api,
    applyDefaultModel: applyDefaultModel,
    authorizeProviderOAuthMutation: authorizeProviderOAuthMutation,
    avatarInputRef: avatarInputRef,
    botAvatarUrl: botAvatarUrl,
    botControlPanelAlias: botControlPanelAlias,
    botName: botName,
    browserInstallHints: browserInstallHints,
    browserIssues: browserIssues,
    browserRecommendations: browserRecommendations,
    browserSmokeResult: browserSmokeResult,
    browserStatus: browserStatus,
    incidentMonitorAutoComment: incidentMonitorAutoComment,
    incidentMonitorAutoCreateIssues: incidentMonitorAutoCreateIssues,
    incidentMonitorConfigQuery: incidentMonitorConfigQuery,
    incidentMonitorCreatedIntakeKey: incidentMonitorCreatedIntakeKey,
    incidentMonitorCurrentBrowseDir: incidentMonitorCurrentBrowseDir,
    incidentMonitorDefaultDestinationsText: incidentMonitorDefaultDestinationsText,
    incidentMonitorDestinationReadiness: incidentMonitorDestinationReadiness,
    incidentMonitorDestinations: incidentMonitorDestinations,
    incidentMonitorDestinationsJson: incidentMonitorDestinationsJson,
    incidentMonitorDisablingIntakeKeyId: incidentMonitorDisablingIntakeKeyId,
    incidentMonitorDraftDecisionMutation: incidentMonitorDraftDecisionMutation,
    incidentMonitorDrafts: incidentMonitorDrafts,
    incidentMonitorDraftsQuery: incidentMonitorDraftsQuery,
    incidentMonitorEnabled: incidentMonitorEnabled,
    incidentMonitorIncidents: incidentMonitorIncidents,
    incidentMonitorIncidentsQuery: incidentMonitorIncidentsQuery,
    incidentMonitorIntakeKeys: incidentMonitorIntakeKeys,
    incidentMonitorIntakeKeysQuery: incidentMonitorIntakeKeysQuery,
    incidentMonitorLogSourceActionResult: incidentMonitorLogSourceActionResult,
    incidentMonitorLogWatcher: incidentMonitorLogWatcher,
    incidentMonitorMcpServer: incidentMonitorMcpServer,
    incidentMonitorModelId: incidentMonitorModelId,
    incidentMonitorMonitoredProjects: incidentMonitorMonitoredProjects,
    incidentMonitorMonitoredProjectsError: incidentMonitorMonitoredProjectsError,
    incidentMonitorMonitoredProjectsJson: incidentMonitorMonitoredProjectsJson,
    incidentMonitorPauseResumeMutation: incidentMonitorPauseResumeMutation,
    incidentMonitorPaused: incidentMonitorPaused,
    incidentMonitorPosts: incidentMonitorPosts,
    incidentMonitorPostsQuery: incidentMonitorPostsQuery,
    incidentMonitorProviderId: incidentMonitorProviderId,
    incidentMonitorProviderModels: incidentMonitorProviderModels,
    incidentMonitorProviderPreference: incidentMonitorProviderPreference,
    incidentMonitorPublishDraftMutation: incidentMonitorPublishDraftMutation,
    incidentMonitorRecheckMatchMutation: incidentMonitorRecheckMatchMutation,
    incidentMonitorReplayIncidentMutation: incidentMonitorReplayIncidentMutation,
    incidentMonitorReplayingSourceKey: incidentMonitorReplayingSourceKey,
    incidentMonitorRepo: incidentMonitorRepo,
    incidentMonitorRequireApproval: incidentMonitorRequireApproval,
    incidentMonitorResettingSourceKey: incidentMonitorResettingSourceKey,
    incidentMonitorRoutePreviewError: incidentMonitorRoutePreviewError,
    incidentMonitorRoutePreviewJson: incidentMonitorRoutePreviewJson,
    incidentMonitorRoutePreviewMutation: incidentMonitorRoutePreviewMutation,
    incidentMonitorRoutePreviewResult: incidentMonitorRoutePreviewResult,
    incidentMonitorRoutes: incidentMonitorRoutes,
    incidentMonitorRoutesJson: incidentMonitorRoutesJson,
    incidentMonitorRoutingConfigError: incidentMonitorRoutingConfigError,
    incidentMonitorSafetyDefaultsJson: incidentMonitorSafetyDefaultsJson,
    incidentMonitorStatus: incidentMonitorStatus,
    incidentMonitorStatusQuery: incidentMonitorStatusQuery,
    incidentMonitorSuggestedWorkspaceRoot: incidentMonitorSuggestedWorkspaceRoot,
    incidentMonitorTriageRunMutation: incidentMonitorTriageRunMutation,
    incidentMonitorWorkspaceBrowserDir: incidentMonitorWorkspaceBrowserDir,
    incidentMonitorWorkspaceBrowserOpen: incidentMonitorWorkspaceBrowserOpen,
    incidentMonitorWorkspaceBrowserQuery: incidentMonitorWorkspaceBrowserQuery,
    incidentMonitorWorkspaceBrowserSearch: incidentMonitorWorkspaceBrowserSearch,
    incidentMonitorWorkspaceDirectories: incidentMonitorWorkspaceDirectories,
    incidentMonitorWorkspaceParentDir: incidentMonitorWorkspaceParentDir,
    incidentMonitorWorkspaceRoot: incidentMonitorWorkspaceRoot,
    incidentMonitorWorkspaceRootHint: incidentMonitorWorkspaceRootHint,
    incidentMonitorWorkspaceSearchQuery: incidentMonitorWorkspaceSearchQuery,
    incidentMonitorWorkspaceSetupWarningText: incidentMonitorWorkspaceSetupWarningText,
    channelDefaultModel: channelDefaultModel,
    channelDrafts: channelDrafts,
    channelDraftsHydratedRef: channelDraftsHydratedRef,
    channelProviderOptions: channelProviderOptions,
    channelScopesQuery: channelScopesQuery,
    channelToolPreferencesQuery: channelToolPreferencesQuery,
    channelToolScopeOpen: channelToolScopeOpen,
    channelToolScopeSelection: channelToolScopeSelection,
    channelVerifyResult: channelVerifyResult,
    channelsConfigQuery: channelsConfigQuery,
    channelsStatusQuery: channelsStatusQuery,
    codexAuthFileName: codexAuthFileName,
    codexAuthInputRef: codexAuthInputRef,
    codexAuthJsonText: codexAuthJsonText,
    configuredMcpServerNames: configuredMcpServerNames,
    configuredProviders: configuredProviders,
    connectedChannelCount: connectedChannelCount,
    connectedMcpCount: connectedMcpCount,
    connectedProviderCount: connectedProviderCount,
    copyIncidentMonitorDebugPayload: copyIncidentMonitorDebugPayload,
    createIncidentMonitorIntakeKeyMutation: createIncidentMonitorIntakeKeyMutation,
    customConfiguredProviders: customConfiguredProviders,
    customProviderApiKey: customProviderApiKey,
    customProviderFormOpen: customProviderFormOpen,
    customProviderId: customProviderId,
    customProviderMakeDefault: customProviderMakeDefault,
    customProviderModel: customProviderModel,
    customProviderUrl: customProviderUrl,
    defaultNavigationRows: defaultNavigationRows,
    defaultNavigationVisibility: defaultNavigationVisibility,
    deleteChannelMutation: deleteChannelMutation,
    diagnosticsOpen: diagnosticsOpen,
    disableIncidentMonitorIntakeKeyMutation: disableIncidentMonitorIntakeKeyMutation,
    disconnectProviderOAuthMutation: disconnectProviderOAuthMutation,
    filteredIncidentMonitorWorkspaceDirectories: filteredIncidentMonitorWorkspaceDirectories,
    filteredMcpCatalog: filteredMcpCatalog,
    getCodexDefaultModelId: getCodexDefaultModelId,
    githubMcpGuideOpen: githubMcpGuideOpen,
    handleAvatarUpload: handleAvatarUpload,
    hiddenAdvancedNavigationCount: hiddenAdvancedNavigationCount,
    hostedManaged: hostedManaged,
    identity: identity,
    identityConfig: identityConfig,
    importCodexAuthFile: importCodexAuthFile,
    importCodexAuthJsonMutation: importCodexAuthJsonMutation,
    installBrowserMutation: installBrowserMutation,
    installConfigError: installConfigError,
    installConfigQuery: installConfigQuery,
    installConfigText: installConfigText,
    installProfileQuery: installProfileQuery,
    invalidateChannels: invalidateChannels,
    invalidateMcp: invalidateMcp,
    loadIdentityConfig: loadIdentityConfig,
    localEngine: localEngine,
    mcpActionMutation: mcpActionMutation,
    mcpAuthMode: mcpAuthMode,
    mcpAuthPreviewText: mcpAuthPreviewText,
    mcpCatalog: mcpCatalog,
    mcpCatalogQuery: mcpCatalogQuery,
    mcpCatalogSearch: mcpCatalogSearch,
    mcpConnectAfterAdd: mcpConnectAfterAdd,
    mcpCustomHeader: mcpCustomHeader,
    mcpEditingName: mcpEditingName,
    mcpExtraHeaders: mcpExtraHeaders,
    mcpGithubToolsets: mcpGithubToolsets,
    mcpIsGithubTransport: mcpIsGithubTransport,
    mcpModalOpen: mcpModalOpen,
    mcpModalTab: mcpModalTab,
    mcpName: mcpName,
    mcpOauthGuidanceText: mcpOauthGuidanceText,
    mcpOauthStartsAfterSave: mcpOauthStartsAfterSave,
    mcpSaveMutation: mcpSaveMutation,
    mcpServers: mcpServers,
    mcpServersQuery: mcpServersQuery,
    mcpToken: mcpToken,
    mcpToolIds: mcpToolIds,
    mcpToolPolicyMutation: mcpToolPolicyMutation,
    mcpToolsQuery: mcpToolsQuery,
    mcpTransport: mcpTransport,
    modelSearchByProvider: modelSearchByProvider,
    navigation: navigation,
    navigationRows: navigationRows,
    navigationVisibility: navigationVisibility,
    oauthSessionByProvider: oauthSessionByProvider,
    openMcpModal: openMcpModal,
    patchIdentityConfig: patchIdentityConfig,
    promoteCodexAsDefaultProvider: promoteCodexAsDefaultProvider,
    providerAuthById: providerAuthById,
    providerCatalogEnabled: providerCatalogEnabled,
    providerDefaultsOpen: providerDefaultsOpen,
    providers: providers,
    providersAuth: providersAuth,
    providersCatalog: providersCatalog,
    providersConfig: providersConfig,
    queryClient: queryClient,
    refreshIdentityStatus: refreshIdentityStatus,
    refreshProviderStatus: refreshProviderStatus,
    refreshIncidentMonitorBindingsMutation: refreshIncidentMonitorBindingsMutation,
    replayIncidentMonitorLogSourceMutation: replayIncidentMonitorLogSourceMutation,
    resetNavigationSections: resetNavigationSections,
    resetIncidentMonitorLogSourceMutation: resetIncidentMonitorLogSourceMutation,
    rootRef: rootRef,
    saveIncidentMonitorMutation: saveIncidentMonitorMutation,
    saveChannelMutation: saveChannelMutation,
    saveChannelToolPreferencesMutation: saveChannelToolPreferencesMutation,
    saveCustomProviderMutation: saveCustomProviderMutation,
    saveIdentityMutation: saveIdentityMutation,
    saveInstallConfigMutation: saveInstallConfigMutation,
    saveSchedulerSettingsMutation: saveSchedulerSettingsMutation,
    saveSearchSettingsMutation: saveSearchSettingsMutation,
    schedulerMaxConcurrent: schedulerMaxConcurrent,
    schedulerMode: schedulerMode,
    schedulerSettingsQuery: schedulerSettingsQuery,
    searchBackend: searchBackend,
    searchBraveKey: searchBraveKey,
    searchExaKey: searchExaKey,
    searchSearxngUrl: searchSearxngUrl,
    searchSettingsQuery: searchSettingsQuery,
    searchTandemUrl: searchTandemUrl,
    searchTestQuery: searchTestQuery,
    searchTestResult: searchTestResult,
    searchTimeoutMs: searchTimeoutMs,
    sectionTabs: sectionTabs,
    selectedIncidentMonitorProvider: selectedIncidentMonitorProvider,
    selectedIncidentMonitorServer: selectedIncidentMonitorServer,
    setActiveSection: selectActiveSection,
    setApiKeyMutation: setApiKeyMutation,
    setBotAvatarUrl: setBotAvatarUrl,
    setBotControlPanelAlias: setBotControlPanelAlias,
    setBotName: setBotName,
    setBrowserSmokeResult: setBrowserSmokeResult,
    setIncidentMonitorAutoComment: setIncidentMonitorAutoComment,
    setIncidentMonitorAutoCreateIssues: setIncidentMonitorAutoCreateIssues,
    setIncidentMonitorCreatedIntakeKey: setIncidentMonitorCreatedIntakeKey,
    setIncidentMonitorDefaultDestinationsText: setIncidentMonitorDefaultDestinationsText,
    setIncidentMonitorDestinationsJson: setIncidentMonitorDestinationsJson,
    setIncidentMonitorDisablingIntakeKeyId: setIncidentMonitorDisablingIntakeKeyId,
    setIncidentMonitorEnabled: setIncidentMonitorEnabled,
    setIncidentMonitorLogSourceActionResult: setIncidentMonitorLogSourceActionResult,
    setIncidentMonitorMcpServer: setIncidentMonitorMcpServer,
    setIncidentMonitorModelId: setIncidentMonitorModelId,
    setIncidentMonitorMonitoredProjectsError: setIncidentMonitorMonitoredProjectsError,
    setIncidentMonitorMonitoredProjectsJson: setIncidentMonitorMonitoredProjectsJson,
    setIncidentMonitorPaused: setIncidentMonitorPaused,
    setIncidentMonitorProviderId: setIncidentMonitorProviderId,
    setIncidentMonitorProviderPreference: setIncidentMonitorProviderPreference,
    setIncidentMonitorReplayingSourceKey: setIncidentMonitorReplayingSourceKey,
    setIncidentMonitorRepo: setIncidentMonitorRepo,
    setIncidentMonitorRequireApproval: setIncidentMonitorRequireApproval,
    setIncidentMonitorResettingSourceKey: setIncidentMonitorResettingSourceKey,
    setIncidentMonitorRoutePreviewError: setIncidentMonitorRoutePreviewError,
    setIncidentMonitorRoutePreviewJson: setIncidentMonitorRoutePreviewJson,
    setIncidentMonitorRoutingConfigError: setIncidentMonitorRoutingConfigError,
    setIncidentMonitorRoutesJson: setIncidentMonitorRoutesJson,
    setIncidentMonitorSafetyDefaultsJson: setIncidentMonitorSafetyDefaultsJson,
    setIncidentMonitorWorkspaceBrowserDir: setIncidentMonitorWorkspaceBrowserDir,
    setIncidentMonitorWorkspaceBrowserOpen: setIncidentMonitorWorkspaceBrowserOpen,
    setIncidentMonitorWorkspaceBrowserSearch: setIncidentMonitorWorkspaceBrowserSearch,
    setIncidentMonitorWorkspaceRoot: setIncidentMonitorWorkspaceRoot,
    setChannelDrafts: setChannelDrafts,
    setChannelToolScopeOpen: setChannelToolScopeOpen,
    setChannelToolScopeSelection: setChannelToolScopeSelection,
    setChannelVerifyResult: setChannelVerifyResult,
    setCodexAuthFileName: setCodexAuthFileName,
    setCodexAuthJsonText: setCodexAuthJsonText,
    setCustomProviderApiKey: setCustomProviderApiKey,
    setCustomProviderFormOpen: setCustomProviderFormOpen,
    setCustomProviderId: setCustomProviderId,
    setCustomProviderMakeDefault: setCustomProviderMakeDefault,
    setCustomProviderModel: setCustomProviderModel,
    setCustomProviderUrl: setCustomProviderUrl,
    setDefaultsMutation: setDefaultsMutation,
    setDiagnosticsOpen: setDiagnosticsOpen,
    setGithubMcpGuideOpen: setGithubMcpGuideOpen,
    setInstallConfigError: setInstallConfigError,
    setInstallConfigText: setInstallConfigText,
    setMcpAuthMode: setMcpAuthMode,
    setMcpCatalogSearch: setMcpCatalogSearch,
    setMcpConnectAfterAdd: setMcpConnectAfterAdd,
    setMcpCustomHeader: setMcpCustomHeader,
    setMcpEditingName: setMcpEditingName,
    setMcpExtraHeaders: setMcpExtraHeaders,
    setMcpGithubToolsets: setMcpGithubToolsets,
    setMcpModalOpen: setMcpModalOpen,
    setMcpModalTab: setMcpModalTab,
    setMcpName: setMcpName,
    setMcpToken: setMcpToken,
    setMcpTransport: setMcpTransport,
    setModelSearchByProvider: setModelSearchByProvider,
    setNavigationRouteVisibility: setNavigationRouteVisibility,
    setOauthSessionByProvider: setOauthSessionByProvider,
    setProviderDefaultsOpen: setProviderDefaultsOpen,
    setSchedulerMaxConcurrent: setSchedulerMaxConcurrent,
    setSchedulerMode: setSchedulerMode,
    setSearchBackend: setSearchBackend,
    setSearchBraveKey: setSearchBraveKey,
    setSearchExaKey: setSearchExaKey,
    setSearchSearxngUrl: setSearchSearxngUrl,
    setSearchTandemUrl: setSearchTandemUrl,
    setSearchTestQuery: setSearchTestQuery,
    setSearchTestResult: setSearchTestResult,
    setSearchTimeoutMs: setSearchTimeoutMs,
    setTheme: setTheme,
    setWorktreeCleanupDryRun: setWorktreeCleanupDryRun,
    setWorktreeCleanupPulse: setWorktreeCleanupPulse,
    setWorktreeCleanupRepoRoot: setWorktreeCleanupRepoRoot,
    setWorktreeCleanupResult: setWorktreeCleanupResult,
    showAllNavigationSections: showAllNavigationSections,
    smokeTestBrowserMutation: smokeTestBrowserMutation,
    systemHealthQuery: systemHealthQuery,
    testSearchMutation: testSearchMutation,
    themeId: themeId,
    themes: themes,
    toast: toast,
    useLocalCodexSessionMutation: useLocalCodexSessionMutation,
    verifyChannelMutation: verifyChannelMutation,
    visibleNavigationCount: visibleNavigationCount,
    worktreeCleanupActionRows: worktreeCleanupActionRows,
    worktreeCleanupDryRun: worktreeCleanupDryRun,
    worktreeCleanupMutation: worktreeCleanupMutation,
    worktreeCleanupPendingMessage: worktreeCleanupPendingMessage,
    worktreeCleanupPendingMessages: worktreeCleanupPendingMessages,
    worktreeCleanupPulse: worktreeCleanupPulse,
    worktreeCleanupRepoRoot: worktreeCleanupRepoRoot,
    worktreeCleanupResult: worktreeCleanupResult,
  };
}
