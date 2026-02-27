import { useEffect, useMemo, useState } from "react";
import { Loader2, PlugZap, RefreshCw } from "lucide-react";
import {
  addMcpServer,
  connectMcpServer,
  deleteMcpServer,
  disconnectMcpServer,
  listMcpServers,
  listMcpTools,
  McpServerRecord,
  setMcpServerEnabled,
  refreshMcpServer,
  client,
} from "../api";

type Action = "connect" | "disconnect" | "refresh" | "enable" | "disable" | "delete";
type Toast = { id: number; kind: "success" | "error"; message: string };

const isArcadeTransport = (transport: string): boolean => {
  try {
    const url = new URL(transport.trim());
    return url.hostname === "api.arcade.dev";
  } catch {
    return false;
  }
};

const hasHeader = (headers: Record<string, string>, key: string): boolean =>
  Object.entries(headers).some(
    ([k, v]) => k.trim().toLowerCase() === key.toLowerCase() && v.trim().length > 0
  );

const parseMcpTransportInput = (
  raw: string
): {
  nameHint?: string;
  transport?: string;
  headersHint?: Record<string, string>;
  error?: string;
} => {
  const trimmed = raw.trim();
  if (!trimmed) return { error: "Transport is required." };
  if (!trimmed.startsWith("{")) return { transport: trimmed };

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return { error: "Transport JSON is invalid." };
  }
  if (!parsed || typeof parsed !== "object") {
    return { error: "Transport JSON must be an object." };
  }

  const root = parsed as Record<string, unknown>;
  const servers = root.mcpServers;
  if (!servers || typeof servers !== "object") {
    return { error: "No mcpServers object found in JSON." };
  }

  const entries = Object.entries(servers as Record<string, unknown>);
  if (entries.length === 0) {
    return { error: "mcpServers is empty." };
  }

  const [nameHint, serverCfg] = entries[0];
  if (!serverCfg || typeof serverCfg !== "object") {
    return { error: "Invalid MCP server config." };
  }
  const cfg = serverCfg as Record<string, unknown>;
  const headersHint =
    cfg.headers && typeof cfg.headers === "object"
      ? Object.fromEntries(
          Object.entries(cfg.headers as Record<string, unknown>).filter(
            (entry): entry is [string, string] =>
              typeof entry[0] === "string" && typeof entry[1] === "string"
          )
        )
      : undefined;

  if (typeof cfg.url === "string" && cfg.url.trim()) {
    return { nameHint, transport: cfg.url.trim(), headersHint };
  }

  if (typeof cfg.command === "string" && cfg.command.trim()) {
    const args = Array.isArray(cfg.args)
      ? cfg.args
          .filter((v): v is string => typeof v === "string")
          .map((v) => v.trim())
          .filter(Boolean)
      : [];
    const commandLine = [cfg.command.trim(), ...args].join(" ");
    return { nameHint, transport: `stdio:${commandLine}`, headersHint };
  }

  return { error: "Could not find url or command in MCP server config." };
};

export default function McpSetup() {
  const [name, setName] = useState("arcade");
  const [transport, setTransport] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [arcadeUserId, setArcadeUserId] = useState("");
  const [servers, setServers] = useState<Record<string, McpServerRecord>>({});
  const [mcpTools, setMcpTools] = useState<unknown[]>([]);
  const [toolIds, setToolIds] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const pushToast = (kind: Toast["kind"], message: string) => {
    const id = Date.now() + Math.floor(Math.random() * 1000);
    setToasts((prev) => [...prev, { id, kind, message }]);
    window.setTimeout(() => {
      setToasts((prev) => prev.filter((toast) => toast.id !== id));
    }, 4200);
  };

  const rows = useMemo(
    () =>
      Object.entries(servers)
        .map(([serverName, value]) => ({ serverName, value }))
        .sort((a, b) => a.serverName.localeCompare(b.serverName)),
    [servers]
  );

  const refreshAll = async () => {
    const [registry, tools, ids] = await Promise.all([
      listMcpServers(),
      listMcpTools(),
      client.listToolIds(),
    ]);
    setServers(registry || {});
    setMcpTools(Array.isArray(tools) ? tools : []);
    setToolIds(Array.isArray(ids) ? ids : []);
  };

  useEffect(() => {
    const load = async () => {
      setLoading(true);
      try {
        await refreshAll();
      } catch (e) {
        pushToast("error", e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    };
    void load();
  }, []);

  const withBusy = async (work: () => Promise<void>) => {
    setBusy(true);
    try {
      await work();
    } catch (e) {
      pushToast("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const addServer = async () => {
    const parsed = parseMcpTransportInput(transport);
    if (parsed.error) {
      pushToast("error", parsed.error);
      return;
    }
    const finalName = name.trim() || parsed.nameHint || "default";
    const finalTransport = parsed.transport || "";
    if (!finalTransport) {
      pushToast("error", "Transport is required.");
      return;
    }
    await withBusy(async () => {
      const headers: Record<string, string> = {
        ...(parsed.headersHint || {}),
        ...(apiKey.trim().length > 0 ? { Authorization: `Bearer ${apiKey.trim()}` } : {}),
        ...(arcadeUserId.trim().length > 0 ? { "Arcade-User-ID": arcadeUserId.trim() } : {}),
      };
      if (isArcadeTransport(finalTransport)) {
        if (!hasHeader(headers, "Authorization")) {
          pushToast("error", "Arcade MCP requires an Authorization Bearer API key.");
          return;
        }
        if (!hasHeader(headers, "Arcade-User-ID")) {
          pushToast(
            "error",
            "Arcade MCP requires a stable Arcade-User-ID (for reusable OAuth identity)."
          );
          return;
        }
      }
      await addMcpServer({
        name: finalName,
        transport: finalTransport,
        headers: Object.keys(headers).length > 0 ? headers : undefined,
        enabled: true,
      });
      await connectMcpServer(finalName);
      await refreshAll();
      pushToast("success", `MCP server '${finalName}' added and connected.`);
    });
  };

  const onAction = async (serverName: string, action: Action) => {
    await withBusy(async () => {
      if (action === "connect") await connectMcpServer(serverName);
      if (action === "disconnect") await disconnectMcpServer(serverName);
      if (action === "refresh") await refreshMcpServer(serverName);
      if (action === "enable") await setMcpServerEnabled(serverName, true);
      if (action === "disable") await setMcpServerEnabled(serverName, false);
      if (action === "delete") await deleteMcpServer(serverName);
      await refreshAll();
      const label = action === "delete" ? "deleted" : "updated";
      pushToast("success", `MCP server '${serverName}' ${label}.`);
    });
  };

  const formatTransportJson = () => {
    const raw = transport.trim();
    if (!raw || !raw.startsWith("{")) return;
    try {
      setTransport(JSON.stringify(JSON.parse(raw), null, 2));
    } catch {
      pushToast("error", "Transport JSON is invalid.");
    }
  };

  return (
    <div className="h-full overflow-y-auto bg-gray-950">
      <div className="fixed top-4 right-4 z-50 space-y-2 w-[22rem] pointer-events-none">
        {toasts.map((toast) => (
          <div
            key={toast.id}
            className={`pointer-events-auto rounded-xl border px-3 py-2 text-sm shadow-lg ${
              toast.kind === "success"
                ? "bg-emerald-900/95 border-emerald-700/60 text-emerald-100"
                : "bg-rose-900/95 border-rose-700/60 text-rose-100"
            }`}
          >
            {toast.message}
          </div>
        ))}
      </div>
      <div className="max-w-5xl mx-auto px-4 py-8 space-y-6">
        <div>
          <h1 className="text-2xl font-bold text-white flex items-center gap-2">
            <PlugZap className="text-cyan-400" size={22} />
            MCP Connections
          </h1>
          <p className="text-sm text-gray-400 mt-1">
            Add and connect MCP servers (e.g. Arcade) so their tools are available to the agent.
          </p>
        </div>

        <div className="grid lg:grid-cols-2 gap-4">
          <div className="rounded-2xl border border-gray-800 bg-gray-900/70 p-4 space-y-3">
            <h2 className="text-sm font-semibold text-white">Add MCP Server</h2>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2.5 text-sm text-gray-200"
              placeholder="arcade"
            />
            <textarea
              value={transport}
              onChange={(e) => setTransport(e.target.value)}
              className="w-full min-h-28 bg-gray-800 border border-gray-700 rounded-xl px-3 py-2.5 text-sm text-gray-200 font-mono"
              placeholder="https://.../mcp, stdio:command, or full mcpServers JSON"
            />
            <button
              type="button"
              onClick={formatTransportJson}
              className="w-full px-3 py-2 rounded-xl bg-gray-800 hover:bg-gray-700 text-gray-300 text-xs"
            >
              Pretty format JSON
            </button>
            <input
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              className="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2.5 text-sm text-gray-200"
              placeholder="Arcade API key (optional)"
            />
            <input
              value={arcadeUserId}
              onChange={(e) => setArcadeUserId(e.target.value)}
              className="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2.5 text-sm text-gray-200"
              placeholder="Arcade User ID (required for Arcade Headers mode)"
            />
            <button
              onClick={() => void addServer()}
              disabled={busy}
              className="w-full px-3 py-2.5 rounded-xl bg-cyan-600 hover:bg-cyan-500 disabled:opacity-60 text-white text-sm font-medium"
            >
              {busy ? "Saving..." : "Add + Connect"}
            </button>
          </div>

          <div className="rounded-2xl border border-gray-800 bg-gray-900/70 p-4 space-y-3">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-white">Registry</h2>
              <button
                onClick={() => void withBusy(refreshAll)}
                className="inline-flex items-center gap-1 px-2 py-1 rounded-lg bg-gray-800 text-xs text-gray-300 hover:text-white"
              >
                <RefreshCw size={12} />
                Refresh
              </button>
            </div>
            <div className="text-xs text-gray-400">
              MCP tools: <span className="text-gray-200">{mcpTools.length}</span> | total tool IDs:{" "}
              <span className="text-gray-200">{toolIds.length}</span>
            </div>
            <div className="max-h-56 overflow-auto rounded-lg border border-gray-800 bg-gray-950 p-2 text-xs text-gray-300">
              {toolIds
                .filter((id) => id.startsWith("mcp."))
                .slice(0, 200)
                .join("\n") || "No MCP-prefixed tools loaded yet."}
            </div>
          </div>
        </div>

        <div className="rounded-2xl border border-gray-800 bg-gray-900/70 p-4">
          <div className="flex items-center justify-between mb-3">
            <h2 className="text-sm font-semibold text-white">Configured Servers</h2>
            {loading && <Loader2 size={16} className="animate-spin text-gray-500" />}
          </div>

          <div className="space-y-3">
            {rows.length === 0 && !loading && (
              <div className="text-sm text-gray-500">No MCP servers configured.</div>
            )}
            {rows.map(({ serverName, value }) => (
              <div
                key={serverName}
                className="rounded-xl border border-gray-800 bg-gray-950 px-3 py-3 space-y-2"
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-sm text-white font-medium truncate">{serverName}</div>
                    <div className="text-xs text-gray-500 truncate">{value.transport}</div>
                  </div>
                  <div className="text-xs">
                    <span
                      className={`px-2 py-1 rounded-full border ${
                        value.connected
                          ? "text-emerald-300 border-emerald-700/50 bg-emerald-900/20"
                          : "text-gray-400 border-gray-700 bg-gray-800/50"
                      }`}
                    >
                      {value.connected ? "connected" : "disconnected"}
                    </span>
                  </div>
                </div>
                <div className="flex gap-2 flex-wrap">
                  <button
                    className="px-2.5 py-1 rounded-lg bg-emerald-700 text-xs text-white"
                    onClick={() => void onAction(serverName, "connect")}
                    disabled={busy}
                  >
                    connect
                  </button>
                  <button
                    className="px-2.5 py-1 rounded-lg bg-yellow-700 text-xs text-white"
                    onClick={() => void onAction(serverName, "refresh")}
                    disabled={busy}
                  >
                    refresh
                  </button>
                  <button
                    className="px-2.5 py-1 rounded-lg bg-rose-700 text-xs text-white"
                    onClick={() => void onAction(serverName, "disconnect")}
                    disabled={busy}
                  >
                    disconnect
                  </button>
                  {value.enabled ? (
                    <button
                      className="px-2.5 py-1 rounded-lg bg-gray-700 text-xs text-white"
                      onClick={() => void onAction(serverName, "disable")}
                      disabled={busy}
                    >
                      disable
                    </button>
                  ) : (
                    <button
                      className="px-2.5 py-1 rounded-lg bg-blue-700 text-xs text-white"
                      onClick={() => void onAction(serverName, "enable")}
                      disabled={busy}
                    >
                      enable
                    </button>
                  )}
                  <button
                    className="px-2.5 py-1 rounded-lg bg-red-900 text-xs text-red-100"
                    onClick={() => void onAction(serverName, "delete")}
                    disabled={busy}
                  >
                    delete
                  </button>
                </div>
                {value.last_error && (
                  <div className="text-xs text-rose-300 bg-rose-900/20 border border-rose-800/40 rounded-lg px-2 py-1">
                    {value.last_error}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
