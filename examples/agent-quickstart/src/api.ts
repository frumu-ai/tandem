import { TandemClient } from "@frumu/tandem-client";

export type JsonObject = Record<string, unknown>;

const PORTAL_WORKSPACE_ROOT_KEY = "tandem_aq_workspace_root";
export const PORTAL_AUTH_EXPIRED_EVENT = "tandem_portal_auth_expired";
let currentToken = "";

export const getWorkspaceRoot = (): string | null => {
  const raw = window.localStorage.getItem(PORTAL_WORKSPACE_ROOT_KEY);
  return raw?.trim() || null;
};

export const setWorkspaceRoot = (v: string | null) => {
  if (!v?.trim()) window.localStorage.removeItem(PORTAL_WORKSPACE_ROOT_KEY);
  else window.localStorage.setItem(PORTAL_WORKSPACE_ROOT_KEY, v.trim());
};

export const DEFAULT_PERMISSION_RULES: JsonObject[] = [
  { permission: "ls", pattern: "*", action: "allow" },
  { permission: "list", pattern: "*", action: "allow" },
  { permission: "glob", pattern: "*", action: "allow" },
  { permission: "search", pattern: "*", action: "allow" },
  { permission: "grep", pattern: "*", action: "allow" },
  { permission: "read", pattern: "*", action: "allow" },
  { permission: "memory_store", pattern: "*", action: "allow" },
  { permission: "memory_search", pattern: "*", action: "allow" },
  { permission: "memory_list", pattern: "*", action: "allow" },
  { permission: "websearch", pattern: "*", action: "allow" },
  { permission: "webfetch", pattern: "*", action: "allow" },
  { permission: "webfetch_html", pattern: "*", action: "allow" },
  { permission: "bash", pattern: "*", action: "allow" },
  { permission: "todowrite", pattern: "*", action: "allow" },
  { permission: "todo_write", pattern: "*", action: "allow" },
];

/**
 * Live SDK instance (ES module live-binding).
 * It is configured to route calls through the Vite/Express proxy at `/engine`.
 */
const createClient = (token: string) =>
  new TandemClient({
    baseUrl: "/engine",
    token,
  });

export let client = createClient("");

export const setClientToken = (token: string) => {
  currentToken = token;
  client = createClient(token);
};

export const clearClientToken = () => {
  currentToken = "";
  client = createClient("");
};

export const verifyToken = async (token: string): Promise<boolean> => {
  const probe = createClient(token);
  try {
    await probe.health();
    return true;
  } catch {
    return false;
  }
};

const engineRequest = async <T>(path: string, init: RequestInit = {}): Promise<T> => {
  const headers = new Headers(init.headers || {});
  if (currentToken) headers.set("Authorization", `Bearer ${currentToken}`);
  if (init.body && !headers.has("Content-Type")) headers.set("Content-Type", "application/json");

  const response = await fetch(`/engine${path}`, {
    ...init,
    headers,
  });

  if (response.status === 401) {
    window.dispatchEvent(new Event(PORTAL_AUTH_EXPIRED_EVENT));
    throw new Error("Unauthorized");
  }

  if (!response.ok) {
    const raw = await response.text();
    throw new Error(raw || `Request failed: ${response.status}`);
  }

  return (await response.json()) as T;
};

export interface McpServerRecord {
  name: string;
  transport: string;
  enabled: boolean;
  connected: boolean;
  last_error?: string;
  headers?: Record<string, string>;
}

export const listMcpServers = async (): Promise<Record<string, McpServerRecord>> =>
  engineRequest<Record<string, McpServerRecord>>("/mcp");

export const addMcpServer = async (payload: {
  name: string;
  transport: string;
  headers?: Record<string, string>;
  enabled?: boolean;
}) => engineRequest<{ ok: boolean }>("/mcp", { method: "POST", body: JSON.stringify(payload) });

export const connectMcpServer = async (name: string) =>
  engineRequest<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}/connect`, { method: "POST" });

export const disconnectMcpServer = async (name: string) =>
  engineRequest<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}/disconnect`, { method: "POST" });

export const refreshMcpServer = async (name: string) =>
  engineRequest<{ ok: boolean; count?: number; error?: string }>(
    `/mcp/${encodeURIComponent(name)}/refresh`,
    { method: "POST" }
  );

export const setMcpServerEnabled = async (name: string, enabled: boolean) =>
  engineRequest<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}`, {
    method: "PATCH",
    body: JSON.stringify({ enabled }),
  });

export const listMcpTools = async (): Promise<unknown[]> => engineRequest<unknown[]>("/mcp/tools");

export const deleteMcpServer = async (name: string) =>
  engineRequest<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}`, { method: "DELETE" });

export const asEpochMs = (v: unknown): number => {
  if (typeof v !== "number" || !Number.isFinite(v)) return Date.now();
  return v < 1_000_000_000_000 ? Math.trunc(v * 1000) : Math.trunc(v);
};
