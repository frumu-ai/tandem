import { useEffect, useMemo, useState } from "react";
import { Link2, Trash2, Wifi, Terminal } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import {
  mcpAddServer,
  mcpConnect,
  mcpCatalog,
  mcpDisconnect,
  mcpListServers,
  mcpListTools,
  mcpRefresh,
  mcpSetEnabled,
  capabilityReadiness,
  type CapabilityReadinessResult,
  type McpCatalogEntry,
  type McpCatalogResult,
  type McpRemoteTool,
  type McpServerRecord,
  opencodeAddMcpServer,
  opencodeListMcpServers,
  opencodeRemoveMcpServer,
  opencodeTestMcpConnection,
  type OpenCodeConfigScope,
  type OpencodeMcpServerEntry,
  type OpencodeMcpTestResult,
} from "@/lib/tauri";

interface IntegrationsTabProps {
  workspacePath: string | null;
}

function getServerType(config: Record<string, unknown>): string {
  const t = config?.type;
  return typeof t === "string" ? t : "unknown";
}

function getRemoteUrl(config: Record<string, unknown>): string | null {
  const url = config?.url;
  return typeof url === "string" ? url : null;
}

function getLocalCommand(config: Record<string, unknown>): string | null {
  const cmd = (config as { command?: unknown })?.command;
  if (Array.isArray(cmd) && cmd.every((x) => typeof x === "string")) return cmd.join(" ");
  return null;
}

type RemotePreset = {
  name: string;
  url: string;
  description: string;
};

const POPULAR_REMOTE_PRESETS: RemotePreset[] = [
  {
    name: "Context7",
    url: "https://mcp.context7.com/mcp",
    description: "Up-to-date library docs and code examples",
  },
  {
    name: "DeepWiki",
    url: "https://mcp.deepwiki.com/mcp",
    description: "AI-powered documentation for GitHub repositories",
  },
];

type CatalogRow = {
  slug: string;
  name: string;
  description: string;
  transportUrl: string;
  serverConfigName: string;
  documentationUrl: string;
  toolCount: number;
  requiresAuth: boolean;
  requiresSetup: boolean;
};

function normalizeCatalog(raw: McpCatalogResult | null | undefined): CatalogRow[] {
  const rows = Array.isArray(raw?.servers) ? raw?.servers : [];
  return rows
    .map((entry: McpCatalogEntry | null | undefined) => {
      const slug = String(entry?.slug || "").trim();
      const transportUrl = String(entry?.transport_url || "").trim();
      if (!slug || !transportUrl) return null;
      return {
        slug,
        name: String(entry?.name || slug).trim(),
        description: String(entry?.description || "").trim(),
        transportUrl,
        serverConfigName: String(entry?.server_config_name || slug).trim(),
        documentationUrl: String(entry?.documentation_url || "").trim(),
        toolCount: Number.isFinite(Number(entry?.tool_count)) ? Number(entry?.tool_count) : 0,
        requiresAuth: entry?.requires_auth !== false,
        requiresSetup: entry?.requires_setup === true,
      };
    })
    .filter((entry): entry is CatalogRow => !!entry)
    .sort((a, b) => a.name.localeCompare(b.name));
}

function parseHeaderLinesToMap(raw: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of String(raw || "").split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const idx = trimmed.indexOf(":");
    if (idx <= 0) continue;
    const key = trimmed.slice(0, idx).trim();
    const value = trimmed.slice(idx + 1).trim();
    if (!key || !value) continue;
    out[key] = value;
  }
  return out;
}

export function IntegrationsTab({ workspacePath }: IntegrationsTabProps) {
  const hasWorkspace = !!workspacePath;
  const [scope, setScope] = useState<OpenCodeConfigScope>(hasWorkspace ? "project" : "global");

  const [servers, setServers] = useState<OpencodeMcpServerEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [runtimeLoading, setRuntimeLoading] = useState(true);
  const [runtimeBusyServer, setRuntimeBusyServer] = useState<string | null>(null);
  const [runtimeServers, setRuntimeServers] = useState<McpServerRecord[]>([]);
  const [runtimeTools, setRuntimeTools] = useState<McpRemoteTool[]>([]);
  const [catalogRows, setCatalogRows] = useState<CatalogRow[]>([]);
  const [catalogGeneratedAt, setCatalogGeneratedAt] = useState<string>("");
  const [catalogSearch, setCatalogSearch] = useState<string>("");
  const [catalogLoading, setCatalogLoading] = useState<boolean>(true);
  const [readinessRequired, setReadinessRequired] = useState("");
  const [readinessResult, setReadinessResult] = useState<CapabilityReadinessResult | null>(null);
  const [readinessLoading, setReadinessLoading] = useState(false);

  const [testingName, setTestingName] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, OpencodeMcpTestResult>>({});

  // Add remote
  const [remoteName, setRemoteName] = useState("");
  const [remoteUrl, setRemoteUrl] = useState("");
  const [remoteHeaders, setRemoteHeaders] = useState("");

  // Add local
  const [localName, setLocalName] = useState("");
  const [localCommand, setLocalCommand] = useState("");
  const [localArgs, setLocalArgs] = useState("");

  useEffect(() => {
    if (!hasWorkspace && scope === "project") setScope("global");
  }, [hasWorkspace, scope]);

  const refresh = async (nextScope: OpenCodeConfigScope = scope) => {
    setLoading(true);
    try {
      setError(null);
      const list = await opencodeListMcpServers(nextScope);
      setServers(list);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load MCP servers");
      setServers([]);
    } finally {
      setLoading(false);
    }
  };

  const refreshRuntime = async () => {
    setRuntimeLoading(true);
    try {
      const [serversList, toolsList] = await Promise.all([mcpListServers(), mcpListTools()]);
      setRuntimeServers(serversList);
      setRuntimeTools(toolsList);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load MCP runtime");
      setRuntimeServers([]);
      setRuntimeTools([]);
    } finally {
      setRuntimeLoading(false);
    }
  };

  const refreshCatalog = async () => {
    setCatalogLoading(true);
    try {
      const payload = await mcpCatalog();
      const catalog =
        payload && typeof payload === "object" && "catalog" in payload ? payload.catalog : payload;
      setCatalogRows(normalizeCatalog(catalog));
      setCatalogGeneratedAt(String(catalog?.generated_at || "").trim());
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load MCP catalog");
      setCatalogRows([]);
      setCatalogGeneratedAt("");
    } finally {
      setCatalogLoading(false);
    }
  };

  useEffect(() => {
    refresh().catch(console.error);
    refreshRuntime().catch(console.error);
    refreshCatalog().catch(console.error);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope, workspacePath]);

  const addRemote = async () => {
    const name = remoteName.trim();
    const url = remoteUrl.trim();
    if (!name || !url) return;

    const headers = remoteHeaders
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);

    const config: Record<string, unknown> = {
      type: "remote",
      url,
      enabled: true,
      ...(headers.length > 0 ? { headers } : {}),
    };

    setSaving(true);
    try {
      setError(null);
      const updated = await opencodeAddMcpServer(scope, name, config);
      setServers(updated);
      setRemoteName("");
      setRemoteUrl("");
      setRemoteHeaders("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to add remote integration");
    } finally {
      setSaving(false);
    }
  };

  const addPreset = async (preset: RemotePreset) => {
    if (servers.some((s) => s.name === preset.name)) {
      setError(`Integration "${preset.name}" already exists.`);
      return;
    }

    const config: Record<string, unknown> = {
      type: "remote",
      url: preset.url,
      enabled: true,
    };

    setSaving(true);
    try {
      setError(null);
      const updated = await opencodeAddMcpServer(scope, preset.name, config);
      setServers(updated);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to add preset integration");
    } finally {
      setSaving(false);
    }
  };

  const addLocal = async () => {
    const name = localName.trim();
    const cmd = localCommand.trim();
    if (!name || !cmd) return;

    const args = localArgs
      .split(/\s+/)
      .map((s) => s.trim())
      .filter(Boolean);

    const config: Record<string, unknown> = {
      type: "local",
      command: [cmd, ...args],
      enabled: true,
    };

    setSaving(true);
    try {
      setError(null);
      const updated = await opencodeAddMcpServer(scope, name, config);
      setServers(updated);
      setLocalName("");
      setLocalCommand("");
      setLocalArgs("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to add local integration");
    } finally {
      setSaving(false);
    }
  };

  const remove = async (name: string) => {
    setSaving(true);
    try {
      setError(null);
      const updated = await opencodeRemoveMcpServer(scope, name);
      setServers(updated);
      setTestResults((prev) => {
        const next = { ...prev };
        delete next[name];
        return next;
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to remove integration");
    } finally {
      setSaving(false);
    }
  };

  const runtimeConnect = async (name: string) => {
    setRuntimeBusyServer(name);
    try {
      const result = await mcpConnect(name);
      if (!result.ok) {
        throw new Error(result.error ?? "Failed to connect MCP server");
      }
      await refreshRuntime();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to connect MCP server");
    } finally {
      setRuntimeBusyServer(null);
    }
  };

  const runtimeDisconnect = async (name: string) => {
    setRuntimeBusyServer(name);
    try {
      const result = await mcpDisconnect(name);
      if (!result.ok) {
        throw new Error(result.error ?? "Failed to disconnect MCP server");
      }
      await refreshRuntime();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to disconnect MCP server");
    } finally {
      setRuntimeBusyServer(null);
    }
  };

  const runtimeRefresh = async (name: string) => {
    setRuntimeBusyServer(name);
    try {
      const result = await mcpRefresh(name);
      if (!result.ok) {
        throw new Error(result.error ?? "Failed to refresh MCP tools");
      }
      await refreshRuntime();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to refresh MCP tools");
    } finally {
      setRuntimeBusyServer(null);
    }
  };

  const runtimeToggleEnabled = async (name: string, enabled: boolean) => {
    setRuntimeBusyServer(name);
    try {
      const result = await mcpSetEnabled(name, enabled);
      if (!result.ok) {
        throw new Error(result.error ?? "Failed to update MCP server");
      }
      await refreshRuntime();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update MCP server");
    } finally {
      setRuntimeBusyServer(null);
    }
  };

  const runReadinessCheck = async () => {
    const required = readinessRequired
      .split(",")
      .map((row) => row.trim())
      .filter(Boolean);
    if (!required.length) {
      setError("Enter at least one required capability ID.");
      return;
    }
    setReadinessLoading(true);
    try {
      setError(null);
      const payload = await capabilityReadiness({
        workflow_id: "desktop-mcp-readiness",
        required_capabilities: required,
      });
      setReadinessResult(payload?.readiness ?? null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to evaluate capability readiness");
      setReadinessResult(null);
    } finally {
      setReadinessLoading(false);
    }
  };

  const test = async (name: string) => {
    setTestingName(name);
    try {
      const result = await opencodeTestMcpConnection(scope, name);
      setTestResults((prev) => ({ ...prev, [name]: result }));
    } catch (e) {
      setTestResults((prev) => ({
        ...prev,
        [name]: {
          status: "failed",
          ok: false,
          error: e instanceof Error ? e.message : "Test failed",
        },
      }));
    } finally {
      setTestingName(null);
    }
  };

  const statusFor = useMemo(() => {
    return (name: string) => testResults[name] ?? null;
  }, [testResults]);

  const configuredNames = useMemo(() => {
    return new Set(
      servers.map((s) =>
        String(s.name || "")
          .trim()
          .toLowerCase()
      )
    );
  }, [servers]);

  const addCatalogServer = async (row: CatalogRow, connectAfterAdd: boolean) => {
    const name = (row.serverConfigName || row.slug || row.name).trim();
    const url = row.transportUrl.trim();
    if (!name || !url) {
      setError("Selected catalog entry is missing a name or transport URL.");
      return;
    }

    const headersMap = parseHeaderLinesToMap(remoteHeaders);
    const headerLines = Object.entries(headersMap).map(([k, v]) => `${k}: ${v}`);
    if (connectAfterAdd && row.requiresAuth && headerLines.length === 0) {
      setRemoteName(name);
      setRemoteUrl(url);
      setError("This MCP requires auth headers. Add headers first, then Add + Connect.");
      return;
    }

    setSaving(true);
    try {
      setError(null);
      await opencodeAddMcpServer(scope, name, {
        type: "remote",
        url,
        enabled: true,
        ...(headerLines.length > 0 ? { headers: headerLines } : {}),
      });
      await mcpAddServer({
        name,
        transport: url,
        enabled: true,
        ...(Object.keys(headersMap).length > 0 ? { headers: headersMap } : {}),
      });
      if (connectAfterAdd) {
        const connected = await mcpConnect(name);
        if (!connected.ok) {
          throw new Error(connected.error ?? "Added server but failed to connect.");
        }
      }
      await Promise.all([refresh(), refreshRuntime()]);
      setRemoteName(name);
      setRemoteUrl(url);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to add MCP server from catalog");
    } finally {
      setSaving(false);
    }
  };

  const visibleCatalogRows = useMemo(() => {
    const query = catalogSearch.trim().toLowerCase();
    if (!query) return catalogRows.slice(0, 50);
    return catalogRows
      .filter((row) => {
        return (
          row.name.toLowerCase().includes(query) ||
          row.slug.toLowerCase().includes(query) ||
          row.transportUrl.toLowerCase().includes(query)
        );
      })
      .slice(0, 50);
  }, [catalogRows, catalogSearch]);

  return (
    <div className="space-y-6">
      {error && (
        <div className="rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
          {error}
        </div>
      )}

      {/* Scope selector */}
      <div className="flex items-center justify-between gap-4">
        <div>
          <p className="text-sm font-medium text-text">Scope</p>
          <p className="text-xs text-text-subtle">Choose where MCP servers are configured</p>
        </div>
        <div className="flex rounded-lg border border-border bg-surface overflow-hidden">
          <button
            type="button"
            onClick={() => setScope("project")}
            disabled={!hasWorkspace}
            className={cn(
              "px-3 py-2 text-sm font-medium transition-colors",
              scope === "project"
                ? "bg-primary/20 text-primary"
                : "text-text-muted hover:bg-surface-elevated hover:text-text",
              !hasWorkspace &&
                "cursor-not-allowed opacity-50 hover:bg-transparent hover:text-text-muted"
            )}
          >
            Folder
          </button>
          <button
            type="button"
            onClick={() => setScope("global")}
            className={cn(
              "px-3 py-2 text-sm font-medium transition-colors",
              scope === "global"
                ? "bg-primary/20 text-primary"
                : "text-text-muted hover:bg-surface-elevated hover:text-text"
            )}
          >
            Global
          </button>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Runtime MCP status</CardTitle>
          <CardDescription>
            Runtime connection state and discovered connector tools used by agents and routines.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {runtimeLoading ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
              Loading runtime MCP status...
            </div>
          ) : runtimeServers.length === 0 ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
              No MCP servers registered in runtime yet.
            </div>
          ) : (
            <div className="space-y-2">
              {runtimeServers.map((server) => {
                const serverTools = runtimeTools.filter((tool) => tool.server_name === server.name);
                const busy = runtimeBusyServer === server.name;
                return (
                  <div
                    key={server.name}
                    className="rounded-lg border border-border bg-surface-elevated p-3"
                  >
                    <div className="flex items-center justify-between gap-3">
                      <div className="min-w-0">
                        <p className="truncate font-semibold text-text">{server.name}</p>
                        <p className="mt-1 truncate text-xs font-mono text-text-muted">
                          {server.transport}
                        </p>
                        <p className="mt-1 text-xs text-text-subtle">
                          {server.enabled ? "Enabled" : "Disabled"} ·{" "}
                          {server.connected ? "Connected" : "Disconnected"} · {serverTools.length}{" "}
                          tools
                        </p>
                        {server.last_error && (
                          <p className="mt-1 text-xs text-error/80">{server.last_error}</p>
                        )}
                      </div>
                      <div className="flex items-center gap-2">
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy}
                          onClick={() => runtimeToggleEnabled(server.name, !server.enabled)}
                        >
                          {server.enabled ? "Disable" : "Enable"}
                        </Button>
                        {server.connected ? (
                          <Button
                            size="sm"
                            variant="secondary"
                            disabled={busy}
                            onClick={() => runtimeDisconnect(server.name)}
                          >
                            Disconnect
                          </Button>
                        ) : (
                          <Button
                            size="sm"
                            variant="secondary"
                            disabled={busy || !server.enabled}
                            onClick={() => runtimeConnect(server.name)}
                          >
                            Connect
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy || !server.enabled}
                          onClick={() => runtimeRefresh(server.name)}
                        >
                          Refresh
                        </Button>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Capability Readiness</CardTitle>
          <CardDescription>
            Fail-closed preflight for required capability IDs before creating or running automation.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex gap-2">
            <Input
              value={readinessRequired}
              onChange={(e) => setReadinessRequired(e.target.value)}
              placeholder="github.list_issues, github.create_pull_request"
            />
            <Button onClick={runReadinessCheck} disabled={readinessLoading}>
              {readinessLoading ? "Checking..." : "Check"}
            </Button>
          </div>
          <pre className="rounded-lg border border-border bg-surface-elevated p-3 text-xs text-text-muted overflow-auto max-h-72">
            {readinessResult ? JSON.stringify(readinessResult, null, 2) : "No readiness check yet."}
          </pre>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1">
              <CardTitle>Configured MCP servers</CardTitle>
              <CardDescription>
                These settings are written to Tandem config. Remote HTTP servers are tested by
                sending a real MCP <span className="font-mono">initialize</span> request (Streamable
                HTTP / SSE). Restart the AI engine to apply changes.
              </CardDescription>
            </div>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => openUrl("https://opencode.ai/docs/mcp-servers")}
            >
              Docs
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          {loading ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
              Loading MCP servers...
            </div>
          ) : servers.length === 0 ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-6 text-center">
              <Link2 className="mx-auto mb-2 h-8 w-8 text-text-subtle" />
              <p className="text-sm text-text-muted">No MCP servers configured.</p>
              <p className="text-xs text-text-subtle">Add a remote or local MCP server below.</p>
            </div>
          ) : (
            <div className="space-y-2">
              {servers.map((s) => {
                const type = getServerType(s.config);
                const url = getRemoteUrl(s.config);
                const cmd = getLocalCommand(s.config);
                const status = statusFor(s.name);

                const badge = (() => {
                  const code =
                    typeof status?.http_status === "number" ? ` (${status.http_status})` : "";

                  if (!status) {
                    return (
                      <span className="rounded-full bg-surface px-2 py-0.5 text-xs text-text-subtle border border-border">
                        Unknown
                      </span>
                    );
                  }
                  if (status.ok) {
                    return (
                      <span className="rounded-full bg-success/10 px-2 py-0.5 text-xs text-success border border-success/20">
                        Connected{code}
                      </span>
                    );
                  }

                  switch (status.status) {
                    case "not_supported":
                      return (
                        <span className="rounded-full bg-surface px-2 py-0.5 text-xs text-text-subtle border border-border">
                          Not tested
                        </span>
                      );
                    case "auth_required":
                      return (
                        <span className="rounded-full bg-yellow-500/10 px-2 py-0.5 text-xs text-yellow-500 border border-yellow-500/20">
                          Auth required{code}
                        </span>
                      );
                    case "wrong_url":
                      return (
                        <span className="rounded-full bg-error/10 px-2 py-0.5 text-xs text-error border border-error/20">
                          Wrong URL{code}
                        </span>
                      );
                    case "wrong_method":
                      return (
                        <span className="rounded-full bg-error/10 px-2 py-0.5 text-xs text-error border border-error/20">
                          Incompatible{code}
                        </span>
                      );
                    case "gone":
                      return (
                        <span className="rounded-full bg-error/10 px-2 py-0.5 text-xs text-error border border-error/20">
                          Gone{code}
                        </span>
                      );
                    case "unreachable":
                      return (
                        <span className="rounded-full bg-error/10 px-2 py-0.5 text-xs text-error border border-error/20">
                          Unreachable
                        </span>
                      );
                    case "invalid_response":
                      return (
                        <span className="rounded-full bg-error/10 px-2 py-0.5 text-xs text-error border border-error/20">
                          Invalid response{code}
                        </span>
                      );
                    default:
                      return (
                        <span className="rounded-full bg-error/10 px-2 py-0.5 text-xs text-error border border-error/20">
                          Failed{code}
                        </span>
                      );
                  }
                })();

                return (
                  <div
                    key={s.name}
                    className="rounded-lg border border-border bg-surface-elevated p-3 space-y-2"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          <p className="truncate font-semibold text-text">{s.name}</p>
                          <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs text-primary border border-primary/20">
                            {type}
                          </span>
                          {badge}
                        </div>
                        {type === "remote" && url && (
                          <p className="mt-1 truncate text-xs font-mono text-text-muted">{url}</p>
                        )}
                        {type === "local" && cmd && (
                          <p className="mt-1 truncate text-xs font-mono text-text-muted">{cmd}</p>
                        )}
                        {status?.error && (
                          <p className="mt-1 text-xs text-error/80">{status.error}</p>
                        )}
                      </div>

                      <div className="flex items-center gap-2">
                        {type === "remote" && (
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() => test(s.name)}
                            disabled={testingName === s.name}
                            title="Test connection"
                          >
                            <Wifi className="mr-2 h-4 w-4" />
                            {testingName === s.name ? "Testing..." : "Test"}
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => remove(s.name)}
                          disabled={saving}
                          className="text-text-subtle hover:text-error hover:bg-error/10"
                          title="Remove"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Popular presets</CardTitle>
          <CardDescription>Quickly add a known-good remote MCP endpoint.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {POPULAR_REMOTE_PRESETS.map((p) => (
            <div
              key={p.name}
              className="flex items-start justify-between gap-3 rounded-lg border border-border bg-surface-elevated p-3"
            >
              <div className="min-w-0">
                <p className="font-semibold text-text">{p.name}</p>
                <p className="mt-0.5 truncate text-xs font-mono text-text-muted">{p.url}</p>
                <p className="mt-1 text-xs text-text-subtle">{p.description}</p>
              </div>
              <Button size="sm" variant="secondary" onClick={() => addPreset(p)} disabled={saving}>
                Add
              </Button>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Remote MCP catalog</CardTitle>
          <CardDescription>
            Search official/curated remote MCP packs and apply one to the Add Remote form.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex items-center gap-2">
            <Input
              value={catalogSearch}
              onChange={(e) => setCatalogSearch(e.target.value)}
              placeholder="Search by name, slug, or URL"
            />
            <Button size="sm" variant="ghost" onClick={refreshCatalog} disabled={catalogLoading}>
              {catalogLoading ? "Loading..." : "Refresh"}
            </Button>
          </div>
          <p className="text-xs text-text-subtle">
            {catalogGeneratedAt
              ? `Generated ${catalogGeneratedAt}`
              : "Catalog timestamp unavailable"}
          </p>
          {catalogLoading ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
              Loading catalog...
            </div>
          ) : visibleCatalogRows.length === 0 ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
              No catalog entries match your search.
            </div>
          ) : (
            <div className="max-h-96 space-y-2 overflow-auto pr-1">
              {visibleCatalogRows.map((row) => (
                <div
                  key={row.slug}
                  className="rounded-lg border border-border bg-surface-elevated p-3 space-y-2"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <p className="truncate font-semibold text-text">{row.name}</p>
                      <p className="mt-1 truncate text-xs text-text-subtle">{row.slug}</p>
                      <p className="mt-1 truncate font-mono text-xs text-text-muted">
                        {row.transportUrl}
                      </p>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs text-primary border border-primary/20">
                        Tools: {row.toolCount}
                      </span>
                      <span
                        className={cn(
                          "rounded-full px-2 py-0.5 text-xs border",
                          row.requiresAuth
                            ? "bg-yellow-500/10 text-yellow-500 border-yellow-500/20"
                            : "bg-success/10 text-success border-success/20"
                        )}
                      >
                        {row.requiresAuth ? "Auth" : "Authless"}
                      </span>
                    </div>
                  </div>
                  {row.description && <p className="text-xs text-text-muted">{row.description}</p>}
                  <div className="flex items-center gap-2">
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        setRemoteName(row.serverConfigName || row.slug);
                        setRemoteUrl(row.transportUrl);
                      }}
                    >
                      Apply
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={saving || configuredNames.has(row.serverConfigName.toLowerCase())}
                      onClick={() => {
                        addCatalogServer(row, false).catch(console.error);
                      }}
                    >
                      {configuredNames.has(row.serverConfigName.toLowerCase()) ? "Added" : "Add"}
                    </Button>
                    <Button
                      size="sm"
                      disabled={saving || configuredNames.has(row.serverConfigName.toLowerCase())}
                      onClick={() => {
                        addCatalogServer(row, true).catch(console.error);
                      }}
                    >
                      {configuredNames.has(row.serverConfigName.toLowerCase())
                        ? "Added"
                        : "Add + Connect"}
                    </Button>
                    {row.documentationUrl && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => openUrl(row.documentationUrl)}
                      >
                        Docs
                      </Button>
                    )}
                    {row.requiresSetup && (
                      <span className="text-xs text-yellow-500">Setup required</span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Add remote server</CardTitle>
          <CardDescription>Configure an HTTP MCP endpoint.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid gap-3 md:grid-cols-2">
            <Input
              value={remoteName}
              onChange={(e) => setRemoteName(e.target.value)}
              placeholder="Name"
            />
            <Input
              value={remoteUrl}
              onChange={(e) => setRemoteUrl(e.target.value)}
              placeholder="https://example.com/mcp"
            />
          </div>
          <textarea
            value={remoteHeaders}
            onChange={(e) => setRemoteHeaders(e.target.value)}
            placeholder="Optional headers (one per line)\nAuthorization: Bearer $TOKEN"
            rows={4}
            className="w-full rounded-lg border border-border bg-surface p-3 font-mono text-sm text-text placeholder:text-text-subtle focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
          />
          <div className="flex items-center justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setRemoteName("");
                setRemoteUrl("");
                setRemoteHeaders("");
              }}
              disabled={saving}
            >
              Clear
            </Button>
            <Button
              onClick={addRemote}
              disabled={saving || !remoteName.trim() || !remoteUrl.trim()}
            >
              {saving ? "Saving..." : "Add Remote"}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Add local server</CardTitle>
          <CardDescription>
            Configure a local stdio MCP server (Tandem does not spawn or handshake yet).
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid gap-3 md:grid-cols-2">
            <Input
              value={localName}
              onChange={(e) => setLocalName(e.target.value)}
              placeholder="Name"
            />
            <Input
              value={localCommand}
              onChange={(e) => setLocalCommand(e.target.value)}
              placeholder="Command (e.g. npx)"
            />
          </div>
          <Input
            value={localArgs}
            onChange={(e) => setLocalArgs(e.target.value)}
            placeholder="Args (space-separated, optional)"
          />
          <div className="flex items-center justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setLocalName("");
                setLocalCommand("");
                setLocalArgs("");
              }}
              disabled={saving}
            >
              Clear
            </Button>
            <Button
              onClick={addLocal}
              disabled={saving || !localName.trim() || !localCommand.trim()}
            >
              <Terminal className="mr-2 h-4 w-4" />
              {saving ? "Saving..." : "Add Local"}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
