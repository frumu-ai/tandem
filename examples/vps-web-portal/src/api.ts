export type JsonObject = Record<string, unknown>;
import { isLikelyToolCapableModel, toolCapablePolicyReason } from "./config/toolCapableModels";
const PORTAL_WORKSPACE_ROOT_KEY = "tandem_portal_workspace_root";

const readWorkspaceRootSetting = (): string | null => {
  if (typeof window === "undefined") return null;
  const raw = window.localStorage.getItem(PORTAL_WORKSPACE_ROOT_KEY);
  if (!raw) return null;
  const normalized = raw.trim();
  return normalized.length > 0 ? normalized : null;
};

const writeWorkspaceRootSetting = (value: string | null): void => {
  if (typeof window === "undefined") return;
  if (!value || value.trim().length === 0) {
    window.localStorage.removeItem(PORTAL_WORKSPACE_ROOT_KEY);
    return;
  }
  window.localStorage.setItem(PORTAL_WORKSPACE_ROOT_KEY, value.trim());
};

export const getPortalWorkspaceRoot = (): string | null => readWorkspaceRootSetting();

export const setPortalWorkspaceRoot = (value: string | null): void => {
  writeWorkspaceRootSetting(value);
};

const asString = (value: unknown): string | null =>
  typeof value === "string" && value.trim().length > 0 ? value : null;

const parseRunId = (payload: JsonObject): string => {
  const direct =
    asString(payload.id) ||
    asString(payload.runID) ||
    asString(payload.runId) ||
    asString(payload.run_id);
  if (direct) return direct;

  const nested = (payload.run || null) as JsonObject | null;
  if (nested) {
    const nestedId =
      asString(nested.id) ||
      asString(nested.runID) ||
      asString(nested.runId) ||
      asString(nested.run_id);
    if (nestedId) return nestedId;
  }

  throw new Error("Run ID missing in engine response");
};

const DEFAULT_PORTAL_PERMISSION_RULES: JsonObject[] = [
  { permission: "ls", pattern: "*", action: "allow" },
  { permission: "list", pattern: "*", action: "allow" },
  { permission: "glob", pattern: "*", action: "allow" },
  { permission: "search", pattern: "*", action: "allow" },
  { permission: "grep", pattern: "*", action: "allow" },
  { permission: "codesearch", pattern: "*", action: "allow" },
  { permission: "read", pattern: "*", action: "allow" },
  { permission: "todowrite", pattern: "*", action: "allow" },
  { permission: "todo_write", pattern: "*", action: "allow" },
  { permission: "websearch", pattern: "*", action: "allow" },
  { permission: "webfetch", pattern: "*", action: "allow" },
  { permission: "webfetch_html", pattern: "*", action: "allow" },
  // Keep shell access explicit in demos to avoid hidden permission deadlocks.
  { permission: "bash", pattern: "*", action: "allow" },
];

const asEpochMs = (value: unknown): number => {
  if (typeof value !== "number" || !Number.isFinite(value)) return Date.now();
  // Engine may return seconds in some payloads.
  return value < 1_000_000_000_000 ? Math.trunc(value * 1000) : Math.trunc(value);
};

export class EngineAPI {
  private baseUrl: string;
  private portalBaseUrl: string;
  private token: string | null;
  private requestTimeoutMs: number;
  private cachedModelSpec: EngineModelSpec | null = null;
  private cachedModelSpecAtMs = 0;

  constructor(token: string | null = null) {
    this.baseUrl = "/engine";
    this.portalBaseUrl = "/portal";
    this.token = token;
    this.requestTimeoutMs = 20000;
  }

  setToken(token: string) {
    this.token = token;
  }

  getToken(): string | null {
    return this.token;
  }

  get isConfigured() {
    return !!this.token;
  }

  private get headers() {
    return {
      "Content-Type": "application/json",
      ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
    };
  }

  private pickFirstModelId(models?: Record<string, ProviderModelEntry>): string | null {
    if (!models) return null;
    const keys = Object.keys(models);
    return keys.length > 0 ? keys[0] : null;
  }

  private clearModelSpecCache() {
    this.cachedModelSpec = null;
    this.cachedModelSpecAtMs = 0;
  }

  async ensureRunnableModel(): Promise<EngineModelSpec> {
    const spec = await this.resolveEngineModelSpec();
    if (!spec) {
      throw new Error(
        "No default provider/model configured. Open Provider Setup and choose a tool-capable model."
      );
    }
    if (!isLikelyToolCapableModel(spec.modelID)) {
      throw new Error(toolCapablePolicyReason(spec.modelID));
    }
    return spec;
  }

  private async resolveEngineModelSpec(): Promise<EngineModelSpec | null> {
    const now = Date.now();
    if (this.cachedModelSpec && now - this.cachedModelSpecAtMs < 30000) {
      return this.cachedModelSpec;
    }

    try {
      const [cfg, catalog] = await Promise.all([
        this.getProvidersConfig(),
        this.getProviderCatalog(),
      ]);
      const connected = new Set((catalog.connected || []).filter(Boolean));
      const entries = new Map((catalog.all || []).map((entry) => [entry.id, entry]));

      const defaultProviderId = asString(cfg.default) || asString(catalog.default);
      if (defaultProviderId && connected.has(defaultProviderId)) {
        const defaultModelId = asString(cfg.providers?.[defaultProviderId]?.default_model);
        if (defaultModelId) {
          const spec = {
            providerID: defaultProviderId,
            modelID: defaultModelId,
          };
          this.cachedModelSpec = spec;
          this.cachedModelSpecAtMs = now;
          return spec;
        }
      }

      for (const providerId of connected) {
        const modelId =
          asString(cfg.providers?.[providerId]?.default_model) ||
          this.pickFirstModelId(entries.get(providerId)?.models);
        if (modelId) {
          const spec = {
            providerID: providerId,
            modelID: modelId,
          };
          this.cachedModelSpec = spec;
          this.cachedModelSpecAtMs = now;
          return spec;
        }
      }
    } catch {
      // Keep request flow working even if model discovery fails.
    }

    return null;
  }

  private async request<T>(
    path: string,
    init: RequestInit = {},
    options: { portal?: boolean } = {}
  ): Promise<T> {
    const base = options.portal ? this.portalBaseUrl : this.baseUrl;
    const controller = new AbortController();
    const timeoutHandle = window.setTimeout(() => {
      controller.abort();
    }, this.requestTimeoutMs);

    let res: Response;
    try {
      res = await fetch(`${base}${path}`, {
        ...init,
        headers: {
          ...this.headers,
          ...(init.headers || {}),
        },
        signal: controller.signal,
      });
    } catch (error) {
      if (error instanceof DOMException && error.name === "AbortError") {
        throw new Error(`Request timed out after ${this.requestTimeoutMs}ms: ${path}`);
      }
      throw error;
    } finally {
      window.clearTimeout(timeoutHandle);
    }

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(`Request failed (${res.status} ${res.statusText}): ${body}`);
    }

    if (res.status === 204) {
      return undefined as T;
    }

    return (await res.json()) as T;
  }

  getGlobalEventStreamUrl(): string {
    return `${this.baseUrl}/global/event?token=${encodeURIComponent(this.token || "")}`;
  }

  getEventStreamUrl(sessionId: string, runId?: string): string {
    void runId;
    const params = new URLSearchParams();
    params.set("sessionID", sessionId);
    // Run-filtered SSE can miss events on some engines where updates are session-scoped.
    // Keep session-level stream as default for reliability across examples.
    params.set("token", this.token || "");
    return `${this.baseUrl}/event?${params.toString()}`;
  }

  async createSession(title = "Web Portal Session"): Promise<string> {
    const modelSpec = await this.resolveEngineModelSpec();
    const configuredWorkspace = readWorkspaceRootSetting();
    const payload: JsonObject = {
      title,
      directory: configuredWorkspace || ".",
      permission: DEFAULT_PORTAL_PERMISSION_RULES,
    };
    if (configuredWorkspace) {
      payload.workspace_root = configuredWorkspace;
    }
    if (modelSpec) {
      payload.model = modelSpec;
      payload.provider = modelSpec.providerID;
    }
    const data = await this.request<{ id: string }>(`/session`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
    return data.id;
  }

  async listSessions(query?: {
    q?: string;
    page?: number;
    pageSize?: number;
    archived?: boolean;
    scope?: "workspace" | "global";
    workspace?: string;
  }): Promise<SessionListResponse> {
    const params = new URLSearchParams();
    if (query?.q) params.set("q", query.q);
    if (query?.page !== undefined) params.set("page", query.page.toString());
    if (query?.pageSize !== undefined) params.set("page_size", query.pageSize.toString());
    if (query?.archived !== undefined) params.set("archived", query.archived.toString());
    if (query?.scope) params.set("scope", query.scope);
    if (query?.workspace) params.set("workspace", query.workspace);

    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.request<unknown>(`/session${qs}`);

    if (Array.isArray(raw)) {
      const sessions = raw
        .filter((item): item is SessionRecord => !!item && typeof item === "object")
        .map((item) => {
          const obj = item as JsonObject;
          const created =
            asEpochMs((obj as { created_at_ms?: unknown }).created_at_ms) ||
            asEpochMs((obj.time as JsonObject | undefined)?.created) ||
            Date.now();
          return {
            ...(obj as SessionRecord),
            created_at_ms: created,
          };
        });
      return { sessions, count: sessions.length };
    }

    const wrapped = (raw || {}) as JsonObject;
    const sessionsRaw = Array.isArray(wrapped.sessions) ? wrapped.sessions : [];
    const sessions = sessionsRaw.map((item) => {
      const obj = item as JsonObject;
      const created =
        asEpochMs((obj as { created_at_ms?: unknown }).created_at_ms) ||
        asEpochMs((obj.time as JsonObject | undefined)?.created) ||
        Date.now();
      return {
        ...(obj as SessionRecord),
        created_at_ms: created,
      };
    });
    const count =
      typeof wrapped.count === "number" && Number.isFinite(wrapped.count)
        ? wrapped.count
        : sessions.length;
    return { sessions, count };
  }

  async deleteSession(sessionId: string): Promise<void> {
    await this.request<void>(`/session/${encodeURIComponent(sessionId)}`, {
      method: "DELETE",
    });
  }

  async sendMessage(sessionId: string, text: string): Promise<void> {
    const modelSpec = await this.resolveEngineModelSpec();
    const payload: JsonObject = { parts: [{ type: "text", text }] };
    if (modelSpec) payload.model = modelSpec;
    await this.request<void>(`/session/${encodeURIComponent(sessionId)}/message`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async startAsyncRun(
    sessionId: string,
    messageText?: string
  ): Promise<{ runId: string; attachPath: string }> {
    const modelSpec = await this.ensureRunnableModel();
    const payload: JsonObject = messageText ? { parts: [{ type: "text", text: messageText }] } : {};
    if (modelSpec) payload.model = modelSpec;

    const path = `/session/${encodeURIComponent(sessionId)}/prompt_async?return=run`;
    const controller = new AbortController();
    const timeoutHandle = window.setTimeout(() => {
      controller.abort();
    }, this.requestTimeoutMs);

    let res: Response;
    try {
      res = await fetch(`${this.baseUrl}${path}`, {
        method: "POST",
        headers: this.headers,
        body: JSON.stringify(payload),
        signal: controller.signal,
      });
    } catch (error) {
      if (error instanceof DOMException && error.name === "AbortError") {
        throw new Error(`Request timed out after ${this.requestTimeoutMs}ms: ${path}`);
      }
      throw error;
    } finally {
      window.clearTimeout(timeoutHandle);
    }

    if (res.status === 409) {
      const conflict = ((await res.json().catch(() => ({}))) || {}) as JsonObject;
      const code = asString(conflict.code);
      const activeRun = (conflict.activeRun || {}) as JsonObject;
      const conflictRunId =
        asString(activeRun.runID) || asString(activeRun.runId) || asString(activeRun.run_id);
      const conflictAttach = asString(conflict.attachEventStream);
      if (code === "SESSION_RUN_CONFLICT" && conflictRunId) {
        return {
          runId: conflictRunId,
          attachPath:
            conflictAttach ||
            `${this.baseUrl}/event?sessionID=${encodeURIComponent(sessionId)}&runID=${encodeURIComponent(conflictRunId)}&token=${encodeURIComponent(this.token || "")}`,
        };
      }
    }

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(`Request failed (${res.status} ${res.statusText}): ${body}`);
    }

    const data = ((await res.json().catch(() => ({}))) || {}) as JsonObject;
    const runId = parseRunId(data);
    return {
      runId,
      attachPath: `${this.baseUrl}/event?sessionID=${encodeURIComponent(sessionId)}&runID=${encodeURIComponent(runId)}&token=${encodeURIComponent(this.token || "")}`,
    };
  }

  async getSystemHealth(): Promise<SystemHealth> {
    return this.request<SystemHealth>(`/global/health`);
  }

  async getSessionMessages(sessionId: string): Promise<EngineMessage[]> {
    return this.request<EngineMessage[]>(`/session/${encodeURIComponent(sessionId)}/message`);
  }

  async getSession(sessionId: string): Promise<SessionRecord> {
    return this.request<SessionRecord>(`/session/${encodeURIComponent(sessionId)}`);
  }

  async runSessionCommand(
    sessionId: string,
    command: string
  ): Promise<{
    ok?: boolean;
    output?: string;
    stdout?: string;
    stderr?: string;
    [key: string]: unknown;
  }> {
    return this.request<{ ok?: boolean; output?: string; stdout?: string; stderr?: string }>(
      `/session/${encodeURIComponent(sessionId)}/command`,
      {
        method: "POST",
        body: JSON.stringify({ command }),
      }
    );
  }

  async listPermissions(): Promise<PermissionSnapshotResponse> {
    return this.request<PermissionSnapshotResponse>(`/permission`);
  }

  async replyPermission(
    requestId: string,
    reply: "allow" | "allow_always" | "deny"
  ): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/permission/${encodeURIComponent(requestId)}/reply`, {
      method: "POST",
      body: JSON.stringify({ reply }),
    });
  }

  async getActiveRun(sessionId: string): Promise<SessionRunStateResponse> {
    return this.request<SessionRunStateResponse>(`/session/${encodeURIComponent(sessionId)}/run`);
  }

  async getProviderCatalog(): Promise<ProviderCatalog> {
    return this.request<ProviderCatalog>(`/provider`);
  }

  async getProvidersConfig(): Promise<ProvidersConfigResponse> {
    return this.request<ProvidersConfigResponse>(`/config/providers`);
  }

  async getProviderKeyPreview(providerId: string): Promise<ProviderKeyPreviewResponse> {
    return this.request<ProviderKeyPreviewResponse>(
      `/provider/key-preview?providerId=${encodeURIComponent(providerId)}`,
      {},
      { portal: true }
    );
  }

  async setProviderAuth(providerId: string, apiKey: string): Promise<void> {
    await this.request<void>(`/auth/${encodeURIComponent(providerId)}`, {
      method: "PUT",
      body: JSON.stringify({ apiKey }),
    });
  }

  async setProviderDefaults(providerId: string, modelId: string): Promise<void> {
    await this.request<void>(`/config`, {
      method: "PATCH",
      body: JSON.stringify({
        default_provider: providerId,
        providers: {
          [providerId]: {
            default_model: modelId,
          },
        },
      }),
    });
    this.clearModelSpecCache();
  }

  async getChannelsConfig(): Promise<ChannelsConfigResponse> {
    return this.request<ChannelsConfigResponse>(`/channels/config`);
  }

  async getChannelsStatus(): Promise<ChannelsStatusResponse> {
    return this.request<ChannelsStatusResponse>(`/channels/status`);
  }

  async putChannel(
    channel: "telegram" | "discord" | "slack",
    payload: JsonObject
  ): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/channels/${channel}`, {
      method: "PUT",
      body: JSON.stringify(payload),
    });
  }

  async deleteChannel(channel: "telegram" | "discord" | "slack"): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/channels/${channel}`, {
      method: "DELETE",
    });
  }

  async listMcpServers(): Promise<Record<string, unknown>> {
    return this.request<Record<string, unknown>>(`/mcp`);
  }

  async addMcpServer(payload: {
    name: string;
    transport: string;
    headers?: Record<string, string>;
    enabled?: boolean;
  }): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/mcp`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async connectMcpServer(name: string): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}/connect`, {
      method: "POST",
    });
  }

  async disconnectMcpServer(name: string): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}/disconnect`, {
      method: "POST",
    });
  }

  async refreshMcpServer(name: string): Promise<{ ok: boolean; count?: number }> {
    return this.request<{ ok: boolean; count?: number }>(
      `/mcp/${encodeURIComponent(name)}/refresh`,
      {
        method: "POST",
      }
    );
  }

  async patchMcpServer(name: string, enabled: boolean): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}`, {
      method: "PATCH",
      body: JSON.stringify({ enabled }),
    });
  }

  async listMcpTools(): Promise<unknown[]> {
    return this.request<unknown[]>(`/mcp/tools`);
  }

  async listToolIds(): Promise<string[]> {
    return this.request<string[]>(`/tool/ids`);
  }

  async createMission(payload: MissionCreateInput): Promise<MissionCreateResponse> {
    return this.request<MissionCreateResponse>(`/mission`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async listMissions(): Promise<MissionListResponse> {
    return this.request<MissionListResponse>(`/mission`);
  }

  async getMission(missionId: string): Promise<MissionGetResponse> {
    return this.request<MissionGetResponse>(`/mission/${encodeURIComponent(missionId)}`);
  }

  async applyMissionEvent(missionId: string, event: JsonObject): Promise<MissionEventResponse> {
    return this.request<MissionEventResponse>(`/mission/${encodeURIComponent(missionId)}/event`, {
      method: "POST",
      body: JSON.stringify({ event }),
    });
  }

  async listAgentTeamTemplates(): Promise<AgentTeamTemplatesResponse> {
    return this.request<AgentTeamTemplatesResponse>(`/agent-team/templates`);
  }

  async listAgentTeamInstances(query?: {
    missionID?: string;
    parentInstanceID?: string;
    status?: string;
  }): Promise<AgentTeamInstancesResponse> {
    const params = new URLSearchParams();
    if (query?.missionID) params.set("missionID", query.missionID);
    if (query?.parentInstanceID) params.set("parentInstanceID", query.parentInstanceID);
    if (query?.status) params.set("status", query.status);
    const suffix = params.toString() ? `?${params.toString()}` : "";
    return this.request<AgentTeamInstancesResponse>(`/agent-team/instances${suffix}`);
  }

  async listAgentTeamMissions(): Promise<AgentTeamMissionsResponse> {
    return this.request<AgentTeamMissionsResponse>(`/agent-team/missions`);
  }

  async listAgentTeamApprovals(): Promise<AgentTeamApprovalsResponse> {
    return this.request<AgentTeamApprovalsResponse>(`/agent-team/approvals`);
  }

  async spawnAgentTeam(payload: AgentTeamSpawnInput): Promise<AgentTeamSpawnResponse> {
    return this.request<AgentTeamSpawnResponse>(`/agent-team/spawn`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async approveAgentTeamSpawn(approvalId: string, reason: string): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(
      `/agent-team/approvals/spawn/${encodeURIComponent(approvalId)}/approve`,
      {
        method: "POST",
        body: JSON.stringify({ reason }),
      }
    );
  }

  async denyAgentTeamSpawn(approvalId: string, reason: string): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(
      `/agent-team/approvals/spawn/${encodeURIComponent(approvalId)}/deny`,
      {
        method: "POST",
        body: JSON.stringify({ reason }),
      }
    );
  }

  async cancelAgentTeamInstance(instanceId: string, reason: string): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(
      `/agent-team/instance/${encodeURIComponent(instanceId)}/cancel`,
      {
        method: "POST",
        body: JSON.stringify({ reason }),
      }
    );
  }

  async cancelAgentTeamMission(missionId: string, reason: string): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(
      `/agent-team/mission/${encodeURIComponent(missionId)}/cancel`,
      {
        method: "POST",
        body: JSON.stringify({ reason }),
      }
    );
  }

  async listRoutines(): Promise<DefinitionListResponse> {
    return this.request<DefinitionListResponse>(`/routines`);
  }

  async listAutomations(): Promise<DefinitionListResponse> {
    return this.request<DefinitionListResponse>(`/automations`);
  }

  async createRoutine(payload: JsonObject): Promise<DefinitionCreateResponse> {
    return this.request<DefinitionCreateResponse>(`/routines`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async createAutomation(payload: JsonObject): Promise<DefinitionCreateResponse> {
    return this.request<DefinitionCreateResponse>(`/automations`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async runNowDefinition(
    apiFamily: "routines" | "automations",
    id: string
  ): Promise<RunNowResponse> {
    return this.request<RunNowResponse>(`/${apiFamily}/${encodeURIComponent(id)}/run_now`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  }

  async listRuns(apiFamily: "routines" | "automations", limit = 25): Promise<RunsListResponse> {
    return this.request<RunsListResponse>(`/${apiFamily}/runs?limit=${limit}`);
  }

  async getRun(apiFamily: "routines" | "automations", runId: string): Promise<RunRecordResponse> {
    return this.request<RunRecordResponse>(`/${apiFamily}/runs/${encodeURIComponent(runId)}`);
  }

  async approveRun(
    apiFamily: "routines" | "automations",
    runId: string,
    reason: string
  ): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(
      `/${apiFamily}/runs/${encodeURIComponent(runId)}/approve`,
      {
        method: "POST",
        body: JSON.stringify({ reason }),
      }
    );
  }

  async denyRun(
    apiFamily: "routines" | "automations",
    runId: string,
    reason: string
  ): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/${apiFamily}/runs/${encodeURIComponent(runId)}/deny`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    });
  }

  async pauseRun(
    apiFamily: "routines" | "automations",
    runId: string,
    reason: string
  ): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/${apiFamily}/runs/${encodeURIComponent(runId)}/pause`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    });
  }

  async resumeRun(
    apiFamily: "routines" | "automations",
    runId: string,
    reason: string
  ): Promise<{ ok: boolean }> {
    return this.request<{ ok: boolean }>(`/${apiFamily}/runs/${encodeURIComponent(runId)}/resume`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    });
  }

  async listRunArtifacts(
    apiFamily: "routines" | "automations",
    runId: string
  ): Promise<RunArtifactsResponse> {
    return this.request<RunArtifactsResponse>(
      `/${apiFamily}/runs/${encodeURIComponent(runId)}/artifacts`
    );
  }

  async getSystemCapabilities(): Promise<SystemCapabilitiesResponse> {
    return this.request<SystemCapabilitiesResponse>(`/system/capabilities`, {}, { portal: true });
  }

  async getEngineServiceStatus(): Promise<SystemEngineStatusResponse> {
    return this.request<SystemEngineStatusResponse>(`/system/engine/status`, {}, { portal: true });
  }

  async controlEngine(action: "start" | "stop" | "restart"): Promise<SystemEngineActionResponse> {
    return this.request<SystemEngineActionResponse>(
      `/system/engine/${action}`,
      {
        method: "POST",
        body: JSON.stringify({}),
      },
      { portal: true }
    );
  }

  async previewArtifact(uri: string): Promise<ArtifactPreviewResponse> {
    return this.request<ArtifactPreviewResponse>(
      `/artifacts/content?uri=${encodeURIComponent(uri)}`,
      {},
      { portal: true }
    );
  }
}

// Global singleton
export const api = new EngineAPI();

export interface EngineModelSpec {
  providerID: string;
  modelID: string;
}

export interface SystemHealth {
  ready?: boolean;
  phase?: string;
  [key: string]: unknown;
}

export interface ProviderModelEntry {
  name?: string;
}

export interface ProviderEntry {
  id: string;
  name?: string;
  models?: Record<string, ProviderModelEntry>;
}

export interface ProviderCatalog {
  all: ProviderEntry[];
  connected?: string[];
  default?: string | null;
}

export interface ProviderConfigEntry {
  default_model?: string;
}

export interface ProvidersConfigResponse {
  default?: string | null;
  providers: Record<string, ProviderConfigEntry>;
}

export interface ProviderKeyPreviewResponse {
  ok: boolean;
  present: boolean;
  envVar: string | null;
  preview: string;
}

export interface EngineMessage {
  info?: {
    role?: string;
  };
  parts?: Array<{
    type?: string;
    text?: string;
  }>;
}

export interface ChannelConfigEntry {
  has_token?: boolean;
  allowed_users?: string[];
  mention_only?: boolean;
  guild_id?: string;
  channel_id?: string;
}

export interface ChannelsConfigResponse {
  telegram: ChannelConfigEntry;
  discord: ChannelConfigEntry;
  slack: ChannelConfigEntry;
}

export interface ChannelStatusEntry {
  enabled: boolean;
  connected: boolean;
  last_error?: string | null;
  active_sessions: number;
  meta?: JsonObject;
}

export interface ChannelsStatusResponse {
  telegram: ChannelStatusEntry;
  discord: ChannelStatusEntry;
  slack: ChannelStatusEntry;
}

export interface MissionCreateInput {
  title: string;
  goal: string;
  work_items: Array<{
    title: string;
    detail?: string;
    assigned_agent?: string;
  }>;
}

export interface MissionCreateResponse {
  mission?: JsonObject;
}

export interface MissionListResponse {
  missions: JsonObject[];
  count: number;
}

export interface MissionGetResponse {
  mission: JsonObject;
}

export interface MissionEventResponse {
  mission?: JsonObject;
  commands?: unknown[];
  orchestratorSpawns?: unknown;
  orchestratorCancellations?: unknown;
}

export interface AgentTeamSpawnInput {
  missionID?: string;
  parentInstanceID?: string;
  templateID?: string;
  role: string;
  source?: string;
  justification: string;
  budget_override?: JsonObject;
}

export interface AgentTeamSpawnResponse {
  ok?: boolean;
  missionID?: string;
  instanceID?: string;
  sessionID?: string;
  runID?: string | null;
  status?: string;
  skillHash?: string;
  code?: string;
  error?: string;
}

export interface AgentTeamTemplatesResponse {
  templates: JsonObject[];
  count: number;
}

export interface AgentTeamInstancesResponse {
  instances: JsonObject[];
  count: number;
}

export interface AgentTeamMissionsResponse {
  missions: JsonObject[];
  count: number;
}

export interface AgentTeamApprovalsResponse {
  spawnApprovals: JsonObject[];
  toolApprovals: JsonObject[];
  count: number;
}

export interface DefinitionListResponse {
  routines?: JsonObject[];
  automations?: JsonObject[];
  count: number;
}

export interface DefinitionCreateResponse {
  routine?: JsonObject;
  automation?: JsonObject;
}

export interface RunNowResponse {
  ok?: boolean;
  runID?: string;
  runId?: string;
  run_id?: string;
  run?: JsonObject;
  status?: string;
}

export interface RunsListResponse {
  runs: JsonObject[];
  count: number;
}

export interface RunRecordResponse {
  run?: JsonObject;
  status?: string;
  [key: string]: unknown;
}

export interface RunArtifactsResponse {
  runID?: string;
  automationRunID?: string;
  artifacts: ArtifactRecord[];
  count: number;
}

export interface ArtifactRecord {
  artifact_id?: string;
  uri: string;
  kind: string;
  label?: string;
  metadata?: JsonObject;
  created_at_ms?: number;
}

export interface SystemCapabilitiesResponse {
  processControl: {
    enabled: boolean;
    mode: string;
    serviceName: string;
    scriptPath?: string;
    reason?: string;
  };
  artifactPreview: {
    enabled: boolean;
    roots: string[];
    maxBytes: number;
  };
}

export interface SystemEngineStatusResponse {
  ok: boolean;
  serviceName: string;
  activeState: string;
  subState: string;
  loadedState: string;
  unitFileState: string;
  timestamp: string;
}

export interface SystemEngineActionResponse {
  ok: boolean;
  action: "start" | "stop" | "restart";
  status?: SystemEngineStatusResponse;
  message?: string;
}

export interface ArtifactPreviewResponse {
  ok: boolean;
  uri: string;
  path: string;
  kind: "text" | "json" | "markdown" | "binary";
  truncated: boolean;
  size: number;
  content?: string;
}

export interface SessionRecord {
  id: string;
  title: string;
  created_at_ms: number;
  directory?: string;
  workspaceRoot?: string;
  workspace_root?: string;
  workspace?: string;
  [key: string]: unknown;
}

export interface SessionListResponse {
  sessions: SessionRecord[];
  count: number;
}

export interface SessionRunStateResponse {
  active?: {
    runID?: string;
    runId?: string;
    run_id?: string;
    attachEventStream?: string;
    [key: string]: unknown;
  } | null;
}

export interface PermissionRequestRecord {
  id: string;
  permission?: string;
  pattern?: string;
  tool?: string;
  status?: string;
  sessionID?: string;
  [key: string]: unknown;
}

export interface PermissionRuleRecord {
  id: string;
  permission: string;
  pattern: string;
  action: string;
  [key: string]: unknown;
}

export interface PermissionSnapshotResponse {
  requests?: PermissionRequestRecord[];
  rules?: PermissionRuleRecord[];
}
