import { useCallback, useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { McpToolAllowlistEditor } from "../components/McpToolAllowlistEditor";
import type { AppPageProps } from "./pageTypes";
import { PageCard } from "./ui";

type McpServer = {
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

type CatalogServer = {
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

type Catalog = {
  generatedAt: string;
  count: number;
  servers: CatalogServer[];
};

function parseUrl(input: string) {
  try {
    return new URL(input);
  } catch {
    return null;
  }
}

function normalizeName(raw: string) {
  const cleaned = String(raw || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return cleaned || "mcp-server";
}

function inferNameFromTransport(transport: string) {
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
  return normalizeName(preferred);
}

function isComposioTransport(transport: string) {
  const url = parseUrl(transport);
  if (!url) return false;
  const host = String(url.hostname || "").toLowerCase();
  return host.endsWith("composio.dev");
}

function isNotionMcpTransport(transport: string) {
  const url = parseUrl(transport);
  if (!url) return false;
  const host = String(url.hostname || "").toLowerCase();
  return host === "mcp.notion.com" || host.endsWith(".notion.com");
}

function getOauthGuidance(name: string, transport: string) {
  const normalizedName = String(name || "")
    .trim()
    .toLowerCase();
  if (normalizedName === "notion" || isNotionMcpTransport(transport)) {
    return "Notion uses browser OAuth. Add the server, finish Notion sign-in in your browser, then return here and click Mark sign-in complete.";
  }
  return "OAuth-backed MCP servers start a browser sign-in on connect. Finish the authorization page, then return to Tandem to complete setup.";
}

const CONTROL_PANEL_READINESS_WORKFLOW_ID = "control-panel-readiness";
const AUTOMATION_WIZARD_DRAFT_KEY = "tandem.automations.createWizardDraft";

function normalizeServerRow(input: any, fallbackName = ""): McpServer | null {
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

function inferMcpCatalogAuthKind(catalog: Catalog, name: string, transport: string) {
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

function normalizeServers(raw: any): McpServer[] {
  if (Array.isArray(raw)) {
    return raw
      .map((entry) => normalizeServerRow(entry))
      .filter((row): row is McpServer => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  if (!raw || typeof raw !== "object") return [];
  if (Array.isArray(raw.servers)) {
    return raw.servers
      .map((entry: any) => normalizeServerRow(entry))
      .filter((row): row is McpServer => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  return Object.entries(raw)
    .map(([name, cfg]) =>
      normalizeServerRow(
        cfg && typeof cfg === "object" ? cfg : { transport: String(cfg || "") },
        name
      )
    )
    .filter((row): row is McpServer => !!row)
    .sort((a, b) => a.name.localeCompare(b.name));
}

function normalizeTools(raw: any): string[] {
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

function normalizeCatalog(raw: any): Catalog {
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
      .filter((row): row is CatalogServer => !!row && !!row.slug && !!row.transportUrl)
      .sort((a, b) => a.name.localeCompare(b.name)),
  };
}

function authPreview(authMode: string, token: string, customHeader: string, transport: string) {
  if (authMode === "oauth") {
    return getOauthGuidance("", transport);
  }
  const hasToken = !!String(token || "").trim();
  if (!hasToken || authMode === "none") return "No auth header will be sent.";

  if (authMode === "custom") {
    return customHeader ? `Header preview: ${customHeader}: <token>` : "Set a custom header name.";
  }

  if (authMode === "x-api-key") return "Header preview: x-api-key: <token>";
  if (authMode === "bearer") return "Header preview: Authorization: Bearer <token>";

  if (isComposioTransport(transport)) return "Auto mode: selected x-api-key for this endpoint";
  return "Auto mode: using Authorization Bearer token";
}

function buildHeaders({
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

function parseCsv(value: string) {
  return String(value || "")
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
}

export function McpPage({ client, api, toast, navigate }: AppPageProps) {
  const queryClient = useQueryClient();

  const [name, setName] = useState("");
  const [transport, setTransport] = useState("");
  const [authMode, setAuthMode] = useState("none");
  const [token, setToken] = useState("");
  const [customHeader, setCustomHeader] = useState("");
  const [catalogSearch, setCatalogSearch] = useState("");
  const [requiredCapabilities, setRequiredCapabilities] = useState("");
  const [readinessResult, setReadinessResult] = useState("No readiness check yet.");
  const [tomlModal, setTomlModal] = useState<{
    slug: string;
    title: string;
    body: string;
    loading: boolean;
  } | null>(null);
  const [hasSavedAutomationDraft, setHasSavedAutomationDraft] = useState(false);

  const serversQuery = useQuery({
    queryKey: ["mcp", "servers"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: 10000,
  });

  const toolsQuery = useQuery({
    queryKey: ["mcp", "tools"],
    queryFn: () => client.mcp.listTools().catch(() => []),
    refetchInterval: 15000,
  });

  const catalogQuery = useQuery({
    queryKey: ["mcp", "catalog"],
    queryFn: () => api("/api/engine/mcp/catalog", { method: "GET" }).catch(() => null),
    refetchInterval: 60000,
  });

  const servers = useMemo(() => normalizeServers(serversQuery.data), [serversQuery.data]);
  const toolIds = useMemo(() => normalizeTools(toolsQuery.data), [toolsQuery.data]);
  const catalog = useMemo(
    () => normalizeCatalog((catalogQuery.data as any)?.catalog || catalogQuery.data || null),
    [catalogQuery.data]
  );

  const configuredServerNames = useMemo(
    () => new Set(servers.map((row) => row.name.toLowerCase())),
    [servers]
  );

  const filteredCatalog = useMemo(() => {
    const query = String(catalogSearch || "")
      .trim()
      .toLowerCase();
    return catalog.servers
      .filter((row) => {
        if (!query) return true;
        return (
          row.name.toLowerCase().includes(query) ||
          row.slug.toLowerCase().includes(query) ||
          row.transportUrl.toLowerCase().includes(query)
        );
      })
      .slice(0, 50);
  }, [catalog.servers, catalogSearch]);

  useEffect(() => {
    try {
      setHasSavedAutomationDraft(!!sessionStorage.getItem(AUTOMATION_WIZARD_DRAFT_KEY));
    } catch {
      setHasSavedAutomationDraft(false);
    }
  }, []);
  useEffect(() => {
    const pendingServers = servers.filter((server) => {
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
          await queryClient.invalidateQueries({ queryKey: ["mcp"] });
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
  }, [api, queryClient, servers]);

  const invalidateMcp = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["mcp"] });
  }, [queryClient]);
  const mcpToolPolicyMutation = useMutation({
    mutationFn: async ({
      serverName,
      allowedTools,
    }: {
      serverName: string;
      allowedTools: string[] | null;
    }) => {
      return client.mcp.patch(serverName, {
        allowed_tools: allowedTools ?? undefined,
        clear_allowed_tools: allowedTools === null,
      });
    },
    onSuccess: async () => {
      await invalidateMcp();
      toast("ok", "MCP tool access updated.");
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const actionMutation = useMutation({
    mutationFn: async ({
      action,
      server,
      value,
    }: {
      action: string;
      server?: McpServer;
      value?: any;
    }) => {
      if (action === "add") {
        const payload = value;
        await (client.mcp as any).add(payload);
        if (value.connectAfterAdd) {
          const result: any = await client.mcp.connect(payload.name);
          const pendingAuth =
            result?.pendingAuth === true ||
            !!result?.lastAuthChallenge ||
            !!result?.authorizationUrl;
          if (
            !result?.ok &&
            !pendingAuth &&
            String(payload?.auth_kind || "")
              .trim()
              .toLowerCase() !== "oauth"
          ) {
            const snapshot = normalizeServers(await client.mcp.list().catch(() => ({})));
            const failed = snapshot.find((row) => row.name === payload.name);
            const detail = failed?.lastError ? ` ${failed.lastError}` : "";
            throw new Error(`Added \"${payload.name}\" but connect failed.${detail}`);
          }
          return {
            name: payload.name,
            connectAfterAdd: true,
            connectResult: result,
            authKind: String(payload?.auth_kind || "")
              .trim()
              .toLowerCase(),
          };
        }
        return {
          name: payload.name,
          connectAfterAdd: false,
          authKind: String(payload?.auth_kind || "")
            .trim()
            .toLowerCase(),
        };
      }

      if (!server) throw new Error("No server selected.");

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
      if (vars.action === "add") {
        const serverName = String((result as any)?.name || vars.value?.name || "").trim();
        const connectAfterAdd = (result as any)?.connectAfterAdd ?? vars.value?.connectAfterAdd;
        const connectResult = (result as any)?.connectResult;
        const authKind = String((result as any)?.authKind || vars.value?.auth_kind || "")
          .trim()
          .toLowerCase();
        const addPendingAuth =
          !!connectResult?.pendingAuth ||
          !!connectResult?.lastAuthChallenge ||
          !!connectResult?.authorizationUrl;
        if (connectAfterAdd && addPendingAuth) {
          const challenge = connectResult?.lastAuthChallenge || {};
          const message = String(challenge?.message || "").trim();
          toast(
            "warn",
            message
              ? `MCP "${serverName}" added and OAuth authorization is still required: ${message}`
              : `MCP "${serverName}" added and OAuth authorization is still required.`
          );
        } else if (connectAfterAdd && authKind === "oauth" && connectResult?.ok !== true) {
          toast(
            "warn",
            `MCP "${serverName}" was added as OAuth-backed. If it still needs authorization, open the auth link from the server row and refresh after signing in.`
          );
        } else if (connectAfterAdd) {
          toast("ok", `MCP "${serverName}" added and connected.`);
        } else {
          toast("ok", `MCP "${serverName}" added.`);
        }
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const readinessMutation = useMutation({
    mutationFn: async () => {
      const required = parseCsv(requiredCapabilities);
      if (!required.length) throw new Error("Enter at least one required capability.");

      if ((client as any).capabilities?.readiness) {
        return (client as any).capabilities.readiness({
          workflow_id: CONTROL_PANEL_READINESS_WORKFLOW_ID,
          required_capabilities: required,
        });
      }

      return api("/api/engine/capabilities/readiness", {
        method: "POST",
        body: JSON.stringify({
          workflow_id: CONTROL_PANEL_READINESS_WORKFLOW_ID,
          required_capabilities: required,
        }),
      });
    },
    onSuccess: (payload) => {
      const readiness = (payload as any)?.readiness || payload;
      setReadinessResult(JSON.stringify(readiness, null, 2));
      toast("ok", readiness?.runnable ? "Ready" : "Not ready");
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const authPreviewText = useMemo(
    () => authPreview(authMode, token, customHeader, transport),
    [authMode, customHeader, token, transport]
  );

  const loadServerIntoForm = (server: McpServer) => {
    setName(normalizeName(server.name));
    setTransport(server.transport || "");

    const headers = server.headers && typeof server.headers === "object" ? server.headers : {};
    const keys = Object.keys(headers);
    const authKey = keys.find((key) => String(key).toLowerCase() === "authorization");
    const apiKey = keys.find((key) => String(key).toLowerCase() === "x-api-key");
    const challengeUrl = String(
      server.lastAuthChallenge?.authorization_url ||
        server.lastAuthChallenge?.authorizationUrl ||
        server.authorizationUrl ||
        ""
    ).trim();
    const serverAuthKind = String(server.authKind || "")
      .trim()
      .toLowerCase();
    const inferredAuthKind =
      serverAuthKind || inferMcpCatalogAuthKind(catalog, server.name, server.transport);
    const nextAuthMode = challengeUrl || inferredAuthKind === "oauth" ? "oauth" : "none";

    setAuthMode(nextAuthMode);
    setToken("");
    setCustomHeader("");

    if (nextAuthMode === "oauth") {
      setToken("");
    } else if (apiKey) {
      setAuthMode("x-api-key");
      setToken(String(headers[apiKey] || "").trim());
    } else if (authKey) {
      setAuthMode("bearer");
      setToken(
        String(headers[authKey] || "")
          .replace(/^bearer\s+/i, "")
          .trim()
      );
    } else if (keys.length === 1) {
      setAuthMode("custom");
      setCustomHeader(keys[0]);
      setToken(String(headers[keys[0]] || "").trim());
    }
  };

  const handleAdd = async (connectAfterAdd: boolean) => {
    const transportValue = String(transport || "").trim();
    const inferredName = inferNameFromTransport(transportValue);
    const normalized = normalizeName(name || inferredName);

    if (!transportValue) {
      toast("err", "Transport URL is required.");
      return;
    }
    if (!parseUrl(transportValue) && !transportValue.startsWith("stdio:")) {
      toast("err", "Transport must be a valid URL or stdio:* transport.");
      return;
    }

    try {
      const headers = buildHeaders({ authMode, token, customHeader, transport: transportValue });
      const payload: any = {
        name: normalized,
        transport: transportValue,
        enabled: true,
        connectAfterAdd,
        auth_kind: authMode === "oauth" ? "oauth" : "",
      };
      if (Object.keys(headers).length) payload.headers = headers;

      await actionMutation.mutateAsync({ action: "add", value: payload });
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <div className="grid gap-4">
      {hasSavedAutomationDraft ? (
        <PageCard title="Automation draft saved" subtitle="Return to the wizard after MCP setup.">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="tcp-subtle text-sm">
              Your automation draft is preserved in this browser session. When you go back to
              Automations, the wizard will restore where you left off.
            </div>
            <button
              type="button"
              className="tcp-btn-primary"
              onClick={() => navigate("automations")}
            >
              Return to Automations
            </button>
          </div>
        </PageCard>
      ) : null}

      <div className="grid gap-4 xl:grid-cols-[440px_1fr]">
        <PageCard
          title="Add MCP Server"
          subtitle="Paste endpoint URL and optional auth token/header."
        >
          <div className="grid gap-3">
            <div>
              <label className="mb-1 block text-sm text-slate-300">Name</label>
              <input
                className="tcp-input"
                value={name}
                onInput={(event) => setName((event.target as HTMLInputElement).value)}
                placeholder="mcp-server"
              />
            </div>
            <div>
              <label className="mb-1 block text-sm text-slate-300">Transport URL</label>
              <input
                className="tcp-input"
                value={transport}
                onInput={(event) => {
                  const value = (event.target as HTMLInputElement).value;
                  setTransport(value);
                  if (!name.trim() || name.trim() === "mcp-server" || name.trim() === "composio") {
                    const inferred = inferNameFromTransport(value);
                    if (inferred) setName(inferred);
                  }
                  const inferredAuthKind = inferMcpCatalogAuthKind(catalog, name, value);
                  if (
                    inferredAuthKind === "oauth" &&
                    (authMode === "none" || authMode === "auto")
                  ) {
                    setAuthMode("oauth");
                    setToken("");
                  }
                }}
                placeholder="https://example.com/mcp"
              />
            </div>
            <div>
              <label className="mb-1 block text-sm text-slate-300">Auth Mode</label>
              <select
                className="tcp-select"
                value={authMode}
                onChange={(event) => {
                  const nextMode = (event.target as HTMLSelectElement).value;
                  setAuthMode(nextMode);
                  if (nextMode === "oauth") setToken("");
                }}
              >
                <option value="none">No Auth Header</option>
                <option value="auto">Auto (x-api-key for known providers, else Bearer)</option>
                <option value="oauth">OAuth</option>
                <option value="x-api-key">x-api-key</option>
                <option value="bearer">Authorization Bearer</option>
                <option value="custom">Custom Header</option>
              </select>
            </div>
            {authMode === "custom" ? (
              <div>
                <label className="mb-1 block text-sm text-slate-300">Custom Header Name</label>
                <input
                  className="tcp-input"
                  value={customHeader}
                  onInput={(event) => setCustomHeader((event.target as HTMLInputElement).value)}
                  placeholder="X-My-Token"
                />
              </div>
            ) : null}
            {authMode === "oauth" ? (
              <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 px-3 py-2 text-xs text-slate-200">
                {authPreviewText}
              </div>
            ) : (
              <div>
                <label className="mb-1 block text-sm text-slate-300">Token (optional)</label>
                <input
                  className="tcp-input"
                  type="password"
                  value={token}
                  onInput={(event) => setToken((event.target as HTMLInputElement).value)}
                  placeholder="token"
                />
                <p className="tcp-subtle mt-2 text-xs">{authPreviewText}</p>
              </div>
            )}
            <div className="flex flex-wrap gap-2">
              <button
                className="tcp-btn"
                onClick={() => void handleAdd(false)}
                disabled={actionMutation.isPending}
              >
                Add
              </button>
              <button
                className="tcp-btn-primary"
                onClick={() => void handleAdd(true)}
                disabled={actionMutation.isPending}
              >
                <i data-lucide="plug-zap"></i>
                Add + Connect
              </button>
            </div>
          </div>
        </PageCard>

        <div className="grid gap-4">
          <PageCard
            title={`Remote MCP Packs (${catalog.count})`}
            subtitle={
              catalog.generatedAt ? `Generated ${catalog.generatedAt}` : "Catalog unavailable"
            }
          >
            <p className="tcp-subtle mb-3 text-xs">
              Remote MCP packs exported as per-server TOML templates. Apply to prefill
              transport/name.
            </p>
            <div className="mb-3 grid gap-2 md:grid-cols-[1fr_auto]">
              <input
                className="tcp-input"
                placeholder="Search pack name, slug, or URL"
                value={catalogSearch}
                onInput={(event) => setCatalogSearch((event.target as HTMLInputElement).value)}
              />
              <button className="tcp-btn" onClick={() => void catalogQuery.refetch()}>
                <i data-lucide="refresh-cw"></i>
                Refresh
              </button>
            </div>
            <div className="grid max-h-[520px] gap-2 overflow-auto pr-1 md:grid-cols-2 2xl:grid-cols-3">
              {filteredCatalog.length ? (
                filteredCatalog.map((row) => {
                  const alreadyConfigured = configuredServerNames.has(
                    String(row.serverConfigName || row.slug || "").toLowerCase()
                  );
                  return (
                    <div key={row.slug} className="tcp-list-item grid h-full content-start gap-2">
                      <div className="flex flex-wrap items-start justify-between gap-2">
                        <div>
                          <div className="font-semibold">{row.name}</div>
                          <div className="tcp-subtle text-xs">
                            {row.slug}
                            {row.requiresSetup ? " · setup required" : ""}
                          </div>
                        </div>
                        <div className="flex flex-wrap gap-2">
                          <span className="tcp-badge-info">Tools: {row.toolCount}</span>
                          {row.authKind === "oauth" ? (
                            <span className="tcp-badge-info">OAuth</span>
                          ) : (
                            <span className={row.requiresAuth ? "tcp-badge-warn" : "tcp-badge-ok"}>
                              {row.requiresAuth ? "Auth" : "Authless"}
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="tcp-subtle line-clamp-2 break-all text-xs">
                        {row.transportUrl}
                      </div>
                      {row.description ? (
                        <div className="line-clamp-2 text-xs text-slate-200">{row.description}</div>
                      ) : null}
                      {row.authKind === "oauth" ? (
                        <div className="rounded-xl border border-sky-700/40 bg-sky-950/20 px-3 py-2 text-xs text-sky-100">
                          Add this pack, then finish the browser sign-in flow. Tandem will keep the
                          server pending until authorization completes.
                        </div>
                      ) : null}
                      <div className="mt-auto flex flex-wrap gap-2">
                        <button
                          className="tcp-btn"
                          onClick={() => {
                            setName(normalizeName(row.serverConfigName || row.slug || row.name));
                            setTransport(row.transportUrl);
                            setAuthMode(row.authKind === "oauth" ? "oauth" : "none");
                            if (row.authKind === "oauth") setToken("");
                            toast(
                              "ok",
                              row.authKind === "oauth"
                                ? `Loaded pack ${row.name}. Add it to start browser sign-in.`
                                : `Loaded pack ${row.name}. Add + Connect when ready.`
                            );
                          }}
                        >
                          Apply
                        </button>
                        <button
                          className="tcp-btn"
                          disabled={alreadyConfigured || actionMutation.isPending}
                          onClick={() => {
                            setName(normalizeName(row.serverConfigName || row.slug || row.name));
                            setTransport(row.transportUrl);
                            void handleAdd(false);
                          }}
                        >
                          {alreadyConfigured ? "Added" : "Add"}
                        </button>
                        <button
                          className="tcp-btn-primary"
                          disabled={alreadyConfigured || actionMutation.isPending}
                          onClick={() => {
                            setName(normalizeName(row.serverConfigName || row.slug || row.name));
                            setTransport(row.transportUrl);
                            void handleAdd(true);
                          }}
                        >
                          {alreadyConfigured
                            ? "Added"
                            : row.authKind === "oauth"
                              ? "Add + Start OAuth"
                              : "Add + Connect"}
                        </button>
                        <button
                          className="tcp-btn"
                          onClick={async () => {
                            setTomlModal({
                              slug: row.slug,
                              title: row.name,
                              body: "Loading TOML...",
                              loading: true,
                            });
                            try {
                              const res = await fetch(
                                `/api/engine/mcp/catalog/${encodeURIComponent(row.slug)}/toml`,
                                {
                                  method: "GET",
                                  credentials: "include",
                                  headers: {
                                    Accept: "application/toml,text/plain;q=0.9,*/*;q=0.8",
                                  },
                                }
                              );
                              if (!res.ok) throw new Error(`HTTP ${res.status}`);
                              const body = await res.text();
                              setTomlModal({
                                slug: row.slug,
                                title: row.name,
                                body: body || "Empty TOML response.",
                                loading: false,
                              });
                            } catch (error) {
                              setTomlModal({
                                slug: row.slug,
                                title: row.name,
                                body: `Failed to load TOML for ${row.slug}: ${error instanceof Error ? error.message : String(error)}`,
                                loading: false,
                              });
                            }
                          }}
                        >
                          View TOML
                        </button>
                        {row.documentationUrl ? (
                          <a
                            className="tcp-btn"
                            href={row.documentationUrl}
                            target="_blank"
                            rel="noreferrer"
                          >
                            Docs
                          </a>
                        ) : null}
                      </div>
                    </div>
                  );
                })
              ) : (
                <p className="tcp-subtle">No catalog entries match your search.</p>
              )}
            </div>
          </PageCard>

          <PageCard
            title={`Servers (${servers.length})`}
            subtitle="Configured MCP servers and controls"
          >
            <div className="mb-3 flex items-center justify-end">
              <button className="tcp-btn" onClick={() => void invalidateMcp()}>
                <i data-lucide="refresh-cw"></i>
                Reload
              </button>
            </div>
            <div className="tcp-list">
              {servers.length ? (
                servers.map((server) => {
                  const headerKeys = Object.keys(server.headers || {}).filter(Boolean);
                  const toolCount = Array.isArray(server.toolCache) ? server.toolCache.length : 0;
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
                          <span className={server.connected ? "tcp-badge-ok" : "tcp-badge-warn"}>
                            {server.connected ? "Connected" : "Disconnected"}
                          </span>
                          <span className={server.enabled ? "tcp-badge-info" : "tcp-badge-warn"}>
                            {server.enabled ? "Enabled" : "Disabled"}
                          </span>
                          <span className="tcp-badge-info">Tools: {toolCount}</span>
                        </div>
                      </div>
                      {server.lastError ? (
                        <div className="rounded-xl border border-rose-700/60 bg-rose-950/20 px-2 py-1 text-xs text-rose-300">
                          {server.lastError}
                        </div>
                      ) : null}
                      {server.lastAuthChallenge ? (
                        <div className="rounded-xl border border-amber-700/60 bg-amber-950/20 px-3 py-2 text-xs text-amber-100">
                          <div className="font-medium">OAuth authorization pending</div>
                          <div className="tcp-subtle mt-1">
                            {String(server.lastAuthChallenge.message || "").trim() ||
                              "Open the authorization URL to finish connecting this MCP server."}
                          </div>
                          <div className="tcp-subtle mt-1">
                            Tandem will keep checking for completion automatically while this page
                            is open.
                          </div>
                          {String(
                            server.lastAuthChallenge.authorization_url ||
                              server.lastAuthChallenge.authorizationUrl ||
                              server.authorizationUrl ||
                              ""
                          ).trim() ? (
                            <div className="mt-2 flex flex-wrap gap-2">
                              <a
                                className="tcp-btn inline-flex h-8 px-3 text-xs"
                                href={String(
                                  server.lastAuthChallenge.authorization_url ||
                                    server.lastAuthChallenge.authorizationUrl ||
                                    server.authorizationUrl ||
                                    ""
                                ).trim()}
                                target="_blank"
                                rel="noreferrer"
                              >
                                Open auth URL
                              </a>
                              <button
                                type="button"
                                className="tcp-btn inline-flex h-8 px-3 text-xs"
                                disabled={actionMutation.isPending}
                                onClick={() =>
                                  actionMutation.mutate({ action: "authenticate", server })
                                }
                              >
                                Mark sign-in complete
                              </button>
                            </div>
                          ) : null}
                        </div>
                      ) : null}
                      <div className="tcp-subtle text-xs">
                        {headerKeys.length
                          ? `Auth headers: ${headerKeys.join(", ")}`
                          : "No stored auth headers."}
                      </div>
                      <McpToolAllowlistEditor
                        title="Tool access"
                        subtitle="Leave all discovered tools selected to expose the full MCP server, or uncheck tools to hide them from agents and workflows."
                        discoveredTools={Array.isArray(server.toolCache) ? server.toolCache : []}
                        value={server.allowedTools}
                        disabled={mcpToolPolicyMutation.isPending}
                        onChange={(next) =>
                          mcpToolPolicyMutation.mutate({
                            serverName: server.name,
                            allowedTools: next,
                          })
                        }
                      />
                      <div className="flex flex-wrap gap-2">
                        <button className="tcp-btn" onClick={() => loadServerIntoForm(server)}>
                          Edit
                        </button>
                        <button
                          className="tcp-btn"
                          disabled={actionMutation.isPending}
                          onClick={() =>
                            actionMutation.mutate({
                              action: server.connected ? "disconnect" : "connect",
                              server,
                            })
                          }
                        >
                          {server.connected ? "Disconnect" : "Connect"}
                        </button>
                        <button
                          className="tcp-btn"
                          disabled={actionMutation.isPending}
                          onClick={() => actionMutation.mutate({ action: "refresh", server })}
                        >
                          Refresh
                        </button>
                        <button
                          className="tcp-btn"
                          disabled={actionMutation.isPending}
                          onClick={() =>
                            actionMutation.mutate({ action: "toggle-enabled", server })
                          }
                        >
                          {server.enabled ? "Disable" : "Enable"}
                        </button>
                        <button
                          className="tcp-btn-danger"
                          disabled={actionMutation.isPending}
                          onClick={() => actionMutation.mutate({ action: "delete", server })}
                        >
                          Delete
                        </button>
                      </div>
                    </div>
                  );
                })
              ) : (
                <p className="tcp-subtle">No MCP servers configured.</p>
              )}
            </div>
          </PageCard>

          <PageCard
            title={`Discovered MCP Tools (${toolIds.length})`}
            subtitle="Tools available across MCP servers"
          >
            <pre className="tcp-code max-h-[320px] overflow-auto">
              {toolIds.slice(0, 350).join("\n") ||
                "No tools discovered yet. Connect a server first."}
            </pre>
          </PageCard>

          <PageCard
            title="Capability Readiness Check"
            subtitle="Validate required capabilities before running templates"
          >
            <div className="grid gap-2 md:grid-cols-[1fr_auto]">
              <input
                className="tcp-input"
                value={requiredCapabilities}
                onInput={(event) =>
                  setRequiredCapabilities((event.target as HTMLInputElement).value)
                }
                placeholder="github.list_issues,github.create_pull_request"
              />
              <button className="tcp-btn" onClick={() => readinessMutation.mutate()}>
                <i data-lucide="shield-check"></i>
                Check
              </button>
            </div>
            <pre className="tcp-code mt-3 max-h-[260px] overflow-auto">{readinessResult}</pre>
          </PageCard>
        </div>

        {tomlModal ? (
          <div className="tcp-confirm-overlay" onClick={() => setTomlModal(null)}>
            <div
              className="tcp-doc-dialog w-[min(64rem,96vw)]"
              onClick={(event) => event.stopPropagation()}
            >
              <div className="tcp-doc-header">
                <h3 className="tcp-doc-title">TOML · {tomlModal.title}</h3>
                <div className="tcp-doc-actions">
                  <button
                    type="button"
                    className="tcp-btn"
                    disabled={tomlModal.loading}
                    onClick={async () => {
                      try {
                        await navigator.clipboard.writeText(tomlModal.body || "");
                        toast("ok", "TOML copied.");
                      } catch {
                        toast("err", "Failed to copy TOML.");
                      }
                    }}
                  >
                    Copy
                  </button>
                  <button type="button" className="tcp-btn" onClick={() => setTomlModal(null)}>
                    Close
                  </button>
                </div>
              </div>
              <pre className="tcp-doc-pre">{tomlModal.body}</pre>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
