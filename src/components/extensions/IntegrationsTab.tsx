import { useEffect, useMemo, useState } from "react";
import { Link2, Trash2, Wifi, Terminal } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import {
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

export function IntegrationsTab({ workspacePath }: IntegrationsTabProps) {
  const hasWorkspace = !!workspacePath;
  const [scope, setScope] = useState<OpenCodeConfigScope>(hasWorkspace ? "project" : "global");

  const [servers, setServers] = useState<OpencodeMcpServerEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  useEffect(() => {
    refresh().catch(console.error);
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
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1">
              <CardTitle>Configured MCP servers</CardTitle>
              <CardDescription>
                These settings are written to OpenCode config. Remote HTTP servers are tested by
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
