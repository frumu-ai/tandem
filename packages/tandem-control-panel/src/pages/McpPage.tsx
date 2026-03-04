import { useCallback, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { AppPageProps } from "./pageTypes";
import { PageCard } from "./ui";

type McpServer = {
  name: string;
  transport: string;
  connected: boolean;
  enabled: boolean;
  lastError: string;
  headers: Record<string, string>;
  toolCache: any[];
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

function normalizeServerRow(input: any, fallbackName = ""): McpServer | null {
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
        };
      })
      .filter((row): row is CatalogServer => !!row && !!row.slug && !!row.transportUrl)
      .sort((a, b) => a.name.localeCompare(b.name)),
  };
}

function authPreview(authMode: string, token: string, customHeader: string, transport: string) {
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

export function McpPage({ client, api, toast }: AppPageProps) {
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

  const invalidateMcp = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["mcp"] });
  }, [queryClient]);

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
          const result = await client.mcp.connect(payload.name);
          if (!result?.ok) {
            const snapshot = normalizeServers(await client.mcp.list().catch(() => ({})));
            const failed = snapshot.find((row) => row.name === payload.name);
            const detail = failed?.lastError ? ` ${failed.lastError}` : "";
            throw new Error(`Added \"${payload.name}\" but connect failed.${detail}`);
          }
        }
        return;
      }

      if (!server) throw new Error("No server selected.");

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
      if (vars.action === "add") {
        toast(
          "ok",
          vars.value.connectAfterAdd
            ? `MCP \"${vars.value.name}\" added and connected.`
            : `MCP \"${vars.value.name}\" added.`
        );
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
          workflow_id: "control-panel-readiness",
          required_capabilities: required,
        });
      }

      return api("/api/engine/capabilities/readiness", {
        method: "POST",
        body: JSON.stringify({
          workflow_id: "control-panel-readiness",
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

    setAuthMode("none");
    setToken("");
    setCustomHeader("");

    if (apiKey) {
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
      };
      if (Object.keys(headers).length) payload.headers = headers;

      await actionMutation.mutateAsync({ action: "add", value: payload });
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  return (
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
              }}
              placeholder="https://example.com/mcp"
            />
          </div>
          <div>
            <label className="mb-1 block text-sm text-slate-300">Auth Mode</label>
            <select
              className="tcp-select"
              value={authMode}
              onChange={(event) => setAuthMode((event.target as HTMLSelectElement).value)}
            >
              <option value="none">No Auth Header</option>
              <option value="auto">Auto (x-api-key for known providers, else Bearer)</option>
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
          <div>
            <label className="mb-1 block text-sm text-slate-300">Token (optional)</label>
            <input
              className="tcp-input"
              type="password"
              value={token}
              onInput={(event) => setToken((event.target as HTMLInputElement).value)}
              placeholder="token"
            />
          </div>
          <p className="tcp-subtle text-xs">{authPreviewText}</p>
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
            Remote MCP packs exported as per-server TOML templates. Apply to prefill transport/name.
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
                        <span className={row.requiresAuth ? "tcp-badge-warn" : "tcp-badge-ok"}>
                          {row.requiresAuth ? "Auth" : "Authless"}
                        </span>
                      </div>
                    </div>
                    <div className="tcp-subtle line-clamp-2 break-all text-xs">
                      {row.transportUrl}
                    </div>
                    {row.description ? (
                      <div className="line-clamp-2 text-xs text-slate-200">{row.description}</div>
                    ) : null}
                    <div className="mt-auto flex flex-wrap gap-2">
                      <button
                        className="tcp-btn"
                        onClick={() => {
                          setName(normalizeName(row.serverConfigName || row.slug || row.name));
                          setTransport(row.transportUrl);
                          toast("ok", `Loaded pack ${row.name}. Add + Connect when ready.`);
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
                        {alreadyConfigured ? "Added" : "Add + Connect"}
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
                                headers: { Accept: "application/toml,text/plain;q=0.9,*/*;q=0.8" },
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
                    <div className="tcp-subtle text-xs">
                      {headerKeys.length
                        ? `Auth headers: ${headerKeys.join(", ")}`
                        : "No stored auth headers."}
                    </div>
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
                        onClick={() => actionMutation.mutate({ action: "toggle-enabled", server })}
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
            {toolIds.slice(0, 350).join("\n") || "No tools discovered yet. Connect a server first."}
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
              onInput={(event) => setRequiredCapabilities((event.target as HTMLInputElement).value)}
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
  );
}
