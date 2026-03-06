import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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

type SettingsSection = "providers" | "identity" | "theme" | "mcp" | "browser";

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
  const [modelSearchByProvider, setModelSearchByProvider] = useState<Record<string, string>>({});
  const [botName, setBotName] = useState(String(identity?.botName || "Tandem"));
  const [botAvatarUrl, setBotAvatarUrl] = useState(String(identity?.botAvatarUrl || ""));
  const [botControlPanelAlias, setBotControlPanelAlias] = useState("Control Center");
  const [activeSection, setActiveSection] = useState<SettingsSection>("providers");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [providerDefaultsOpen, setProviderDefaultsOpen] = useState(false);
  const [mcpModalOpen, setMcpModalOpen] = useState(false);
  const [mcpName, setMcpName] = useState("");
  const [mcpTransport, setMcpTransport] = useState("");
  const [mcpAuthMode, setMcpAuthMode] = useState("none");
  const [mcpToken, setMcpToken] = useState("");
  const [mcpCustomHeader, setMcpCustomHeader] = useState("");
  const [mcpConnectAfterAdd, setMcpConnectAfterAdd] = useState(true);
  const [mcpEditingName, setMcpEditingName] = useState("");
  const [mcpModalTab, setMcpModalTab] = useState<"manual" | "catalog">("manual");
  const [mcpCatalogSearch, setMcpCatalogSearch] = useState("");
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
  }, [currentRoute]);

  const providersCatalog = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [], connected: [] })),
  });

  const providersConfig = useQuery({
    queryKey: ["settings", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({ default: "", providers: {} })),
  });

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
      const payload: any = {
        name: normalizedName,
        transport: transportValue,
        enabled: true,
      };
      if (Object.keys(headers).length) payload.headers = headers;

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
  const browserIssues = Array.isArray(browserStatus.data?.blocking_issues)
    ? browserStatus.data?.blocking_issues || []
    : [];
  const browserRecommendations = Array.isArray(browserStatus.data?.recommendations)
    ? browserStatus.data?.recommendations || []
    : [];
  const browserInstallHints = Array.isArray(browserStatus.data?.install_hints)
    ? browserStatus.data?.install_hints || []
    : [];

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
      setMcpEditingName(server.name);
      setMcpName(server.name);
      setMcpTransport(server.transport || "");
      setMcpConnectAfterAdd(server.connected || false);
      if (apiKey) {
        setMcpAuthMode("x-api-key");
        setMcpCustomHeader("");
        setMcpToken(String(headers[apiKey] || "").trim());
      } else if (authKey) {
        setMcpAuthMode("bearer");
        setMcpCustomHeader("");
        setMcpToken(
          String(headers[authKey] || "")
            .replace(/^bearer\s+/i, "")
            .trim()
        );
      } else if (keys.length === 1) {
        setMcpAuthMode("custom");
        setMcpCustomHeader(keys[0]);
        setMcpToken(String(headers[keys[0]] || "").trim());
      } else {
        setMcpAuthMode("none");
        setMcpCustomHeader("");
        setMcpToken("");
      }
    } else {
      setMcpModalTab("catalog");
      setMcpEditingName("");
      setMcpName("");
      setMcpTransport("");
      setMcpAuthMode("none");
      setMcpCustomHeader("");
      setMcpToken("");
      setMcpConnectAfterAdd(true);
    }
    setMcpModalOpen(true);
  };

  const sectionTabs: Array<{ id: SettingsSection; label: string; icon: string }> = [
    { id: "providers", label: "Providers", icon: "cpu" },
    { id: "identity", label: "Identity", icon: "badge-check" },
    { id: "theme", label: "Themes", icon: "palette" },
    { id: "mcp", label: "MCP", icon: "plug-zap" },
    { id: "browser", label: "Browser", icon: "monitor-cog" },
  ];
  const mcpAuthPreviewText = useMemo(
    () => mcpAuthPreview(mcpAuthMode, mcpToken, mcpCustomHeader, mcpTransport),
    [mcpAuthMode, mcpCustomHeader, mcpToken, mcpTransport]
  );

  return (
    <AnimatedPage className="grid gap-4">
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
                          {providers.length} providers available for configuration. Expand to change
                          models and API keys.
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
                              onClick={() => mcpActionMutation.mutate({ action: "delete", server })}
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
                      {browserSmokeResult.final_url || browserSmokeResult.url || "No URL returned"}
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
                      {browserSmokeResult.final_url || browserSmokeResult.url || "No URL returned"}
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
                    <div key={`browser-recommendation-${index}`} className="tcp-list-item text-sm">
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
                className="grid gap-3"
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
                  <div className="grid gap-3">
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
                    <div className="grid max-h-[26rem] gap-2 overflow-auto pr-1 md:grid-cols-2">
                      {filteredMcpCatalog.length ? (
                        filteredMcpCatalog.map((row) => {
                          const alreadyConfigured = configuredMcpServerNames.has(
                            String(row.serverConfigName || row.slug || "").toLowerCase()
                          );
                          return (
                            <div key={row.slug} className="tcp-list-item grid gap-2">
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
                              <div className="tcp-subtle break-all text-xs">{row.transportUrl}</div>
                              <div className="mt-auto flex flex-wrap gap-2">
                                <button
                                  type="button"
                                  className="tcp-btn h-8 px-3 text-xs"
                                  onClick={() => {
                                    setMcpName(
                                      normalizeMcpName(row.serverConfigName || row.slug || row.name)
                                    );
                                    setMcpTransport(row.transportUrl);
                                    setMcpModalTab("manual");
                                    toast("ok", `Loaded ${row.name}. Review and save when ready.`);
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
                          onInput={(event) => setMcpName((event.target as HTMLInputElement).value)}
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
                  <button type="button" className="tcp-btn" onClick={() => setMcpModalOpen(false)}>
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
    </AnimatedPage>
  );
}
