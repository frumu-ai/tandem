import { streamSse } from "./stream.js";
import type {
  TandemClientOptions,
  JsonObject,
  SystemHealth,
  SessionRecord,
  SessionListResponse,
  ListSessionsOptions,
  CreateSessionOptions,
  UpdateSessionOptions,
  SessionRunStateResponse,
  PromptAsyncResult,
  PromptPartInput,
  PromptModelOptions,
  PromptRoutingOptions,
  SessionDiff,
  SessionTodo,
  EngineMessage,
  PermissionSnapshotResponse,
  PermissionReply,
  QuestionsListResponse,
  QuestionRecord,
  ProviderCatalog,
  ProvidersConfigResponse,
  IdentityConfig,
  IdentityConfigResponse,
  ChannelName,
  ChannelsConfigResponse,
  ChannelsStatusResponse,
  ChannelVerifyResponse,
  ChannelToolPreferences,
  ChannelToolPreferencesInput,
  AddMcpServerOptions,
  MemoryPutOptions,
  MemoryPutResponse,
  MemorySearchOptions,
  MemorySearchResponse,
  MemoryListResponse,
  MemoryPromoteOptions,
  MemoryPromoteResponse,
  MemoryDemoteOptions,
  MemoryDemoteResponse,
  MemoryAuditResponse,
  SkillLocation,
  SkillRecord,
  SkillsListResponse,
  SkillImportOptions,
  SkillImportResponse,
  SkillTemplatesResponse,
  SkillCatalogRecord,
  SkillValidationResponse,
  SkillRouterMatchResponse,
  SkillsBenchmarkEvalResponse,
  SkillsTriggerEvalResponse,
  SkillCompileResponse,
  SkillGenerateResponse,
  SkillGenerateInstallResponse,
  ResourceRecord,
  ResourceListResponse,
  ResourceWriteOptions,
  ResourceWriteResponse,
  BrowserStatusResponse,
  BrowserInstallResponse,
  BrowserSmokeTestResponse,
  WorkflowRecord,
  WorkflowListResponse,
  WorkflowRunRecord,
  WorkflowRunListResponse,
  WorkflowHookRecord,
  WorkflowHookListResponse,
  BugMonitorConfigResponse,
  BugMonitorStatusResponse,
  BugMonitorIncidentRecord,
  BugMonitorIncidentListResponse,
  BugMonitorDraftRecord,
  BugMonitorDraftListResponse,
  BugMonitorPostListResponse,
  CoderGithubProjectInboxResponse,
  CoderGithubProjectIntakeResponse,
  CoderRunRecord,
  CoderRunsListResponse,
  CoderRunGetResponse,
  CoderProjectBindingGetResponse,
  CoderProjectBindingPutResponse,
  CoderArtifactRecord,
  CoderArtifactsResponse,
  CoderMemoryHitRecord,
  CoderMemoryHitsResponse,
  CoderMemoryCandidateRecord,
  CoderMemoryCandidatesResponse,
  PackInstallRecord,
  PacksListResponse,
  PackInspectionResponse,
  PackInstallOptions,
  PackUninstallOptions,
  PackExportOptions,
  PackDetectOptions,
  CapabilityBindingsFile,
  CapabilityReadinessInput,
  CapabilityReadinessOutput,
  CapabilityResolveInput,
  RoutineRecord,
  DefinitionListResponse,
  DefinitionCreateResponse,
  CreateRoutineOptions,
  PatchRoutineOptions,
  CreateAutomationOptions,
  PatchAutomationOptions,
  RunNowResponse,
  RunsListResponse,
  RunRecord,
  RunArtifactsResponse,
  RoutineHistoryResponse,
  AgentTeamSpawnInput,
  AgentTeamSpawnResponse,
  AgentTeamTemplateCreateInput,
  AgentTeamTemplatePatchInput,
  AgentTeamTemplatesResponse,
  AgentTeamInstancesResponse,
  AgentTeamMissionsResponse,
  AgentTeamApprovalsResponse,
  AgentStandupComposeInput,
  AgentStandupComposeResponse,
  AutomationV2Spec,
  AutomationV2RunRecord,
  WorkflowPlan,
  WorkflowPlanConversation,
  WorkflowPlanPreviewResponse,
  WorkflowPlanApplyResponse,
  WorkflowPlanChatResponse,
  WorkflowPlanGetResponse,
  WorkflowPlanImportPreviewResponse,
  WorkflowPlanImportResponse,
  WorkflowPlanPackBuilderExportRequest,
  WorkflowPlanPackBuilderExportResult,
  WorkflowPlanDraftRecord,
  WorkflowPlannerSessionCreateResponse,
  WorkflowPlannerSessionDuplicateResponse,
  WorkflowPlannerSessionListResponse,
  WorkflowPlannerSessionMessageResponse,
  WorkflowPlannerSessionPatchResponse,
  WorkflowPlannerSessionRecord,
  WorkflowPlannerSessionResetResponse,
  WorkflowPlannerSessionResponse,
  WorkflowPlannerSessionStartResponse,
  MissionCreateInput,
  MissionCreateResponse,
  MissionListResponse,
  MissionGetResponse,
  MissionEventResponse,
  ToolSchema,
  ToolExecuteResult,
  EngineEvent,
  RoutineFamily,
  MemoryItem,
  SkillTemplate,
  JsonValue,
} from "./public/index.js";
import {
  SystemHealthSchema,
  SessionRecordSchema,
  SessionListResponseSchema,
  SessionRunStateResponseSchema,
  RunNowResponseSchema,
  RunRecordSchema,
  ResourceWriteResponseSchema,
  ResourceRecordSchema,
  ResourceListResponseSchema,
  MemoryItemSchema,
  MemoryListResponseSchema,
  MemorySearchResponseSchema,
  ContextResolveResponseSchema,
  ContextTreeResponseSchema,
  ContextDistillResponseSchema,
  EngineEventSchema,
  parseResponse,
  idNormalizer,
} from "./normalize/index.js";

// ─── Internal helpers ─────────────────────────────────────────────────────────

const asString = (v: unknown): string | null =>
  typeof v === "string" && v.trim().length > 0 ? v : null;

const parseRunId = (payload: JsonObject): string => {
  try {
    const id = idNormalizer.parse(payload);
    if (id) return id;
  } catch {
    const nested = (payload.run || null) as JsonObject | null;
    if (nested) {
      try {
        const id2 = idNormalizer.parse(nested);
        if (id2) return id2;
      } catch {}
    }
  }
  throw new Error("Run ID missing in engine response");
};

type RequestInitWithTimeout = RequestInit & {
  timeoutMs?: number;
};

// ─── TandemClient ─────────────────────────────────────────────────────────────

/**
 * HTTP client for the Tandem autonomous agent engine.
 *
 * Provides full coverage of the Tandem engine HTTP + SSE API.
 *
 * @example
 * ```typescript
 * import { TandemClient } from "@frumu/tandem-client";
 *
 * const client = new TandemClient({
 *   baseUrl: "http://localhost:39731",
 *   token: "your-token",
 * });
 *
 * const sessionId = await client.sessions.create({ title: "My agent" });
 * const { runId } = await client.sessions.promptAsync(sessionId, "Summarize README.md");
 *
 * for await (const event of client.stream(sessionId, runId)) {
 *   if (event.type === "session.response") {
 *     process.stdout.write(String(event.properties.delta ?? ""));
 *   }
 *   if (
 *     event.type === "run.complete" ||
 *     event.type === "run.completed" ||
 *     event.type === "run.failed" ||
 *     event.type === "session.run.finished"
 *   ) break;
 * }
 * ```
 */
export class TandemClient {
  private baseUrl: string;
  private token: string;
  private timeoutMs: number;

  /** Session management */
  readonly sessions: Sessions;
  /** Permission approval flow */
  readonly permissions: Permissions;
  /** AI-generated question approval */
  readonly questions: Questions;
  /** Provider catalog and configuration */
  readonly providers: Providers;
  /** Bot identity and personality configuration */
  readonly identity: Identity;
  /** Messaging platform channel integrations */
  readonly channels: Channels;
  /** MCP (Model Context Protocol) server management */
  readonly mcp: Mcp;
  /** Scheduled routines */
  readonly routines: Routines;
  /** Mission-scoped automations */
  readonly automations: Automations;
  /** Persistent automation flows (V2) */
  readonly automationsV2: AutomationsV2;
  /** Workflow optimization campaigns */
  readonly optimizations: Optimizations;
  /** Engine-owned workflow planning */
  readonly workflowPlans: WorkflowPlans;
  /** Planner session management */
  readonly workflowPlannerSessions: WorkflowPlannerSessions;
  /** Semantic memory / vector store */
  readonly memory: Memory;
  /** Agent skill packs */
  readonly skills: Skills;
  /** Tandem pack lifecycle management */
  readonly packs: Packs;
  /** Capability bindings + resolver APIs */
  readonly capabilities: Capabilities;
  /** Key-value resource store */
  readonly resources: Resources;
  /** Browser lifecycle and diagnostics */
  readonly browser: Browser;
  /** Workflow registry, runs, and hooks */
  readonly workflows: Workflows;
  /** Bug monitor incident and draft operations */
  readonly bugMonitor: BugMonitor;
  /** Coder workflow runs and artifacts */
  readonly coder: Coder;
  /** Agent team orchestration */
  readonly agentTeams: AgentTeams;
  /** Multi-agent mission management */
  readonly missions: Missions;

  constructor(options: TandemClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.token = options.token;
    this.timeoutMs = options.timeoutMs ?? 20_000;

    const req = this._request.bind(this);
    this.sessions = new Sessions(this.baseUrl, this.token, this.timeoutMs, req);
    this.permissions = new Permissions(req);
    this.questions = new Questions(req);
    this.providers = new Providers(req);
    this.identity = new Identity(req);
    this.channels = new Channels(req);
    this.mcp = new Mcp(req, this._requestText.bind(this));
    const getToken = () => this.token;
    this.routines = new Routines(this.baseUrl, getToken, req);
    this.automations = new Automations(this.baseUrl, getToken, req);
    this.automationsV2 = new AutomationsV2(this.baseUrl, getToken, req);
    this.optimizations = new Optimizations(req);
    this.workflowPlans = new WorkflowPlans(req);
    this.workflowPlannerSessions = new WorkflowPlannerSessions(req);
    this.memory = new Memory(req);
    this.skills = new Skills(req);
    this.packs = new Packs(req);
    this.capabilities = new Capabilities(req);
    this.resources = new Resources(this.baseUrl, getToken, req);
    this.browser = new Browser(req);
    this.workflows = new Workflows(this.baseUrl, getToken, req);
    this.bugMonitor = new BugMonitor(req);
    this.coder = new Coder(req);
    this.agentTeams = new AgentTeams(req);
    this.missions = new Missions(req);
  }

  /**
   * Update the bearer token used for future HTTP and SSE requests.
   */
  setToken(token: string): void {
    this.token = token;
    this.sessions.setToken(token);
  }

  // ─── Health ───────────────────────────────────────────────────────────────

  /** Check engine health. Returns `{ ready: true }` when the engine is ready. */
  async health(): Promise<SystemHealth> {
    const raw = await this._request<unknown>("/global/health");
    return parseResponse(SystemHealthSchema, raw, "/global/health", 200);
  }

  // ─── Tools ────────────────────────────────────────────────────────────────

  /** List all tool IDs registered in the engine. */
  async listToolIds(): Promise<string[]> {
    return this._request<string[]>("/tool/ids");
  }

  /** List all tools with their schemas. */
  async listTools(): Promise<ToolSchema[]> {
    const raw = await this._request<unknown>("/tool");
    return Array.isArray(raw) ? (raw as ToolSchema[]) : [];
  }

  /**
   * Execute a built-in tool directly (without a session).
   *
   * @example
   * ```typescript
   * const result = await client.executeTool("workspace_list_files", { path: "." });
   * console.log(result.output);
   * ```
   */
  async executeTool(tool: string, args?: JsonObject): Promise<ToolExecuteResult> {
    return this._request<ToolExecuteResult>("/tool/execute", {
      method: "POST",
      body: JSON.stringify({ tool, args: args ?? {} }),
    });
  }

  // ─── SSE streaming ────────────────────────────────────────────────────────

  /**
   * Stream events from an active run as an async generator.
   *
   * @example
   * ```typescript
   * for await (const event of client.stream(sessionId, runId)) {
   *   if (event.type === "session.response") {
   *     process.stdout.write(String(event.properties.delta ?? ""));
   *   }
   *   if (
   *     event.type === "run.complete" ||
   *     event.type === "run.completed" ||
   *     event.type === "run.failed" ||
   *     event.type === "session.run.finished"
   *   ) break;
   * }
   * ```
   */
  stream(
    sessionId: string,
    runId?: string,
    options?: { signal?: AbortSignal }
  ): AsyncGenerator<EngineEvent> {
    const params = new URLSearchParams({ sessionID: sessionId });
    if (runId) params.set("runID", runId);
    const url = `${this.baseUrl}/event?${params.toString()}`;
    return streamSse(url, this.token, options);
  }

  /**
   * Stream the global event feed (all sessions).
   */
  globalStream(options?: { signal?: AbortSignal }): AsyncGenerator<EngineEvent> {
    const url = `${this.baseUrl}/global/event`;
    return streamSse(url, this.token, options);
  }

  /**
   * Pull stored events for a specific run (paginated, not SSE).
   */
  async runEvents(
    runId: string,
    options?: { sinceSeq?: number; tail?: number }
  ): Promise<EngineEvent[]> {
    const params = new URLSearchParams();
    if (options?.sinceSeq !== undefined) params.set("since_seq", String(options.sinceSeq));
    if (options?.tail !== undefined) params.set("tail", String(options.tail));
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this._request<unknown>(`/run/${encodeURIComponent(runId)}/events${qs}`);
    return Array.isArray(raw) ? (raw as EngineEvent[]) : [];
  }

  // ─── Internal HTTP ────────────────────────────────────────────────────────

  async _request<T>(path: string, init: RequestInitWithTimeout = {}): Promise<T> {
    const timeoutMs = init.timeoutMs ?? this.timeoutMs;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);

    let res: Response;
    try {
      res = await fetch(`${this.baseUrl}${path}`, {
        ...init,
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${this.token}`,
          ...(init.headers ?? {}),
        },
        signal: controller.signal,
      });
    } catch (err) {
      if (err instanceof Error && err.name === "AbortError") {
        throw new Error(`Request timed out after ${timeoutMs}ms: ${path}`);
      }
      throw err;
    } finally {
      clearTimeout(timer);
    }

    if (res.status === 204) return undefined as T;
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(`Request failed (${res.status} ${res.statusText}): ${body}`);
    }
    return res.json() as Promise<T>;
  }

  async _requestText(path: string, init: RequestInitWithTimeout = {}): Promise<string> {
    const timeoutMs = init.timeoutMs ?? this.timeoutMs;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);

    let res: Response;
    try {
      res = await fetch(`${this.baseUrl}${path}`, {
        ...init,
        headers: {
          Authorization: `Bearer ${this.token}`,
          ...(init.headers ?? {}),
        },
        signal: controller.signal,
      });
    } catch (err) {
      if (err instanceof Error && err.name === "AbortError") {
        throw new Error(`Request timed out after ${timeoutMs}ms: ${path}`);
      }
      throw err;
    } finally {
      clearTimeout(timer);
    }

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(`Request failed (${res.status} ${res.statusText}): ${body}`);
    }
    return res.text();
  }
}

// ─── Sessions namespace ───────────────────────────────────────────────────────

class Browser {
  constructor(private req: TandemClient["_request"]) {}

  async status(): Promise<BrowserStatusResponse> {
    return this.req<BrowserStatusResponse>("/browser/status");
  }

  async install(): Promise<BrowserInstallResponse> {
    return this.req<BrowserInstallResponse>("/browser/install", { method: "POST" });
  }

  async smokeTest(options?: { url?: string }): Promise<BrowserSmokeTestResponse> {
    return this.req<BrowserSmokeTestResponse>("/browser/smoke-test", {
      method: "POST",
      body: JSON.stringify(options ?? {}),
    });
  }
}

class Workflows {
  constructor(
    private baseUrl: string,
    private getToken: () => string,
    private req: TandemClient["_request"]
  ) {}

  async list(): Promise<WorkflowListResponse> {
    const raw = await this.req<unknown>("/workflows");
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    const workflows = Array.isArray(asObj.workflows) ? (asObj.workflows as WorkflowRecord[]) : [];
    return { workflows, count: typeof asObj.count === "number" ? asObj.count : workflows.length };
  }

  async get(id: string): Promise<WorkflowRecord> {
    const raw = await this.req<{ workflow?: WorkflowRecord }>(
      `/workflows/${encodeURIComponent(id)}`
    );
    return raw.workflow ?? ({} as WorkflowRecord);
  }

  async validate(payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>("/workflows/validate", {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  async simulate(payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>("/workflows/simulate", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  events(options?: {
    workflowId?: string;
    workflow_id?: string;
    runId?: string;
    run_id?: string;
    signal?: AbortSignal;
  }): AsyncGenerator<EngineEvent> {
    const params = new URLSearchParams();
    const workflowId = options?.workflow_id ?? options?.workflowId;
    const runId = options?.run_id ?? options?.runId;
    if (workflowId) params.set("workflow_id", workflowId);
    if (runId) params.set("run_id", runId);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return streamSse(`${this.baseUrl}/workflows/events${qs}`, this.getToken(), {
      signal: options?.signal,
    });
  }

  async listRuns(options?: {
    workflowId?: string;
    workflow_id?: string;
    limit?: number;
  }): Promise<WorkflowRunListResponse> {
    const params = new URLSearchParams();
    const workflowId = options?.workflow_id ?? options?.workflowId;
    if (workflowId) params.set("workflow_id", workflowId);
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(`/workflows/runs${qs}`);
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    const runs = Array.isArray(asObj.runs) ? (asObj.runs as WorkflowRunRecord[]) : [];
    return { runs, count: typeof asObj.count === "number" ? asObj.count : runs.length };
  }

  async getRun(id: string): Promise<WorkflowRunRecord> {
    const raw = await this.req<{ run?: WorkflowRunRecord }>(
      `/workflows/runs/${encodeURIComponent(id)}`
    );
    return raw.run ?? ({} as WorkflowRunRecord);
  }

  async run(id: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/workflows/${encodeURIComponent(id)}/run`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  async listHooks(options?: {
    workflowId?: string;
    workflow_id?: string;
  }): Promise<WorkflowHookListResponse> {
    const params = new URLSearchParams();
    const workflowId = options?.workflow_id ?? options?.workflowId;
    if (workflowId) params.set("workflow_id", workflowId);
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(`/workflow-hooks${qs}`);
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    const hooks = Array.isArray(asObj.hooks) ? (asObj.hooks as WorkflowHookRecord[]) : [];
    return { hooks, count: typeof asObj.count === "number" ? asObj.count : hooks.length };
  }

  async patchHook(id: string, patch: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/workflow-hooks/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(patch),
    });
  }
}

class BugMonitor {
  constructor(private req: TandemClient["_request"]) {}

  async getConfig(): Promise<BugMonitorConfigResponse> {
    return this.req<BugMonitorConfigResponse>("/config/bug-monitor");
  }

  async patchConfig(config: JsonObject): Promise<BugMonitorConfigResponse> {
    return this.req<BugMonitorConfigResponse>("/config/bug-monitor", {
      method: "PATCH",
      body: JSON.stringify(config),
    });
  }

  async getStatus(): Promise<BugMonitorStatusResponse> {
    return this.req<BugMonitorStatusResponse>("/bug-monitor/status");
  }

  async recomputeStatus(): Promise<BugMonitorStatusResponse> {
    return this.req<BugMonitorStatusResponse>("/bug-monitor/status/recompute", {
      method: "POST",
    });
  }

  async pause(): Promise<JsonObject> {
    return this.req<JsonObject>("/bug-monitor/pause", { method: "POST" });
  }

  async resume(): Promise<JsonObject> {
    return this.req<JsonObject>("/bug-monitor/resume", { method: "POST" });
  }

  async debug(): Promise<JsonObject> {
    return this.req<JsonObject>("/bug-monitor/debug");
  }

  async listIncidents(options?: { limit?: number }): Promise<BugMonitorIncidentListResponse> {
    const qs = options?.limit !== undefined ? `?limit=${options.limit}` : "";
    return this.req<BugMonitorIncidentListResponse>(`/bug-monitor/incidents${qs}`);
  }

  async getIncident(id: string): Promise<BugMonitorIncidentRecord> {
    const raw = await this.req<{ incident?: BugMonitorIncidentRecord }>(
      `/bug-monitor/incidents/${encodeURIComponent(id)}`
    );
    return raw.incident ?? ({} as BugMonitorIncidentRecord);
  }

  async replayIncident(id: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/bug-monitor/incidents/${encodeURIComponent(id)}/replay`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  async listDrafts(options?: { limit?: number }): Promise<BugMonitorDraftListResponse> {
    const qs = options?.limit !== undefined ? `?limit=${options.limit}` : "";
    return this.req<BugMonitorDraftListResponse>(`/bug-monitor/drafts${qs}`);
  }

  async listPosts(options?: { limit?: number }): Promise<BugMonitorPostListResponse> {
    const qs = options?.limit !== undefined ? `?limit=${options.limit}` : "";
    return this.req<BugMonitorPostListResponse>(`/bug-monitor/posts${qs}`);
  }

  async getDraft(id: string): Promise<BugMonitorDraftRecord> {
    const raw = await this.req<{ draft?: BugMonitorDraftRecord }>(
      `/bug-monitor/drafts/${encodeURIComponent(id)}`
    );
    return raw.draft ?? ({} as BugMonitorDraftRecord);
  }

  async approveDraft(id: string, reason?: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/bug-monitor/drafts/${encodeURIComponent(id)}/approve`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    });
  }

  async denyDraft(id: string, reason?: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/bug-monitor/drafts/${encodeURIComponent(id)}/deny`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    });
  }

  async report(payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>("/bug-monitor/report", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async createTriageRun(id: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/bug-monitor/drafts/${encodeURIComponent(id)}/triage-run`, {
      method: "POST",
    });
  }

  async createTriageSummary(id: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/bug-monitor/drafts/${encodeURIComponent(id)}/triage-summary`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async createIssueDraft(id: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/bug-monitor/drafts/${encodeURIComponent(id)}/issue-draft`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  async publishDraft(id: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/bug-monitor/drafts/${encodeURIComponent(id)}/publish`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  async recheckMatch(id: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/bug-monitor/drafts/${encodeURIComponent(id)}/recheck-match`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }
}

class Sessions {
  constructor(
    private baseUrl: string,
    private token: string,
    private timeoutMs: number,
    private req: TandemClient["_request"]
  ) {}

  setToken(token: string): void {
    this.token = token;
  }

  /** Create a new session. Returns the session ID. */
  async create(options: CreateSessionOptions = {}): Promise<string> {
    const payload: JsonObject = {
      title: options.title ?? "Tandem SDK Session",
      directory: options.directory ?? ".",
    };
    if (options.permissions) payload.permission = options.permissions as unknown as JsonValue;
    if (options.model && options.provider) {
      payload.model = { providerID: options.provider, modelID: options.model };
      payload.provider = options.provider;
    }
    const data = await this.req<{ id: string }>("/session", {
      method: "POST",
      body: JSON.stringify(payload),
    });
    return data.id;
  }

  /** List sessions with optional filtering. */
  async list(options: ListSessionsOptions = {}): Promise<SessionListResponse> {
    const params = new URLSearchParams();
    if (options.q) params.set("q", options.q);
    if (options.page !== undefined) params.set("page", String(options.page));
    if (options.pageSize !== undefined) params.set("page_size", String(options.pageSize));
    if (options.archived !== undefined) params.set("archived", String(options.archived));
    if (options.scope) params.set("scope", options.scope);
    if (options.workspace) params.set("workspace", options.workspace);
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(`/session${qs}`);
    return parseResponse(SessionListResponseSchema, raw, "/session", 200);
  }

  /** Get a session by ID. */
  async get(sessionId: string): Promise<SessionRecord> {
    const raw = await this.req<unknown>(`/session/${encodeURIComponent(sessionId)}`);
    return parseResponse(SessionRecordSchema, raw, `/session/${sessionId}`, 200);
  }

  /** Update session metadata (title, archive status). */
  async update(sessionId: string, options: UpdateSessionOptions): Promise<SessionRecord> {
    const raw = await this.req<unknown>(`/session/${encodeURIComponent(sessionId)}`, {
      method: "PATCH",
      body: JSON.stringify(options),
    });
    return parseResponse(SessionRecordSchema, raw, `/session/${sessionId}`, 200);
  }

  /** Archive a session (shorthand for `update(id, { archived: true })`). */
  async archive(sessionId: string): Promise<SessionRecord> {
    return this.update(sessionId, { archived: true });
  }

  /** Delete a session permanently. */
  async delete(sessionId: string): Promise<void> {
    await this.req<void>(`/session/${encodeURIComponent(sessionId)}`, { method: "DELETE" });
  }

  /** Get all messages in a session. */
  async messages(sessionId: string): Promise<EngineMessage[]> {
    return this.req<EngineMessage[]>(`/session/${encodeURIComponent(sessionId)}/message`);
  }

  /** Get pending TODOs associated with a session. */
  async todos(sessionId: string): Promise<SessionTodo[]> {
    const raw = await this.req<unknown>(`/session/${encodeURIComponent(sessionId)}/todo`);
    if (Array.isArray(raw)) return raw as SessionTodo[];
    const wrapped = raw as { todos?: SessionTodo[] };
    return wrapped.todos ?? [];
  }

  /** Get the currently active run for a session (if any). */
  async activeRun(sessionId: string): Promise<SessionRunStateResponse> {
    const raw = await this.req<unknown>(`/session/${encodeURIComponent(sessionId)}/run`);
    return parseResponse(SessionRunStateResponseSchema, raw, `/session/${sessionId}/run`, 200);
  }

  /**
   * Start an async run and return the run ID.
   * Use `client.stream(sessionId, runId)` to receive events.
   *
   * Handles 409 SESSION_RUN_CONFLICT by returning the existing run ID.
   */
  async promptAsync(
    sessionId: string,
    prompt: string,
    model?: PromptModelOptions,
    routing?: PromptRoutingOptions
  ): Promise<PromptAsyncResult> {
    return this.promptAsyncParts(sessionId, [{ type: "text", text: prompt }], model, routing);
  }

  /**
   * Start an async run with explicit prompt parts (text and/or file parts).
   *
   * Handles 409 SESSION_RUN_CONFLICT by returning the existing run ID.
   */
  async promptAsyncParts(
    sessionId: string,
    parts: PromptPartInput[],
    model?: PromptModelOptions,
    routing?: PromptRoutingOptions
  ): Promise<PromptAsyncResult> {
    const payload: JsonObject = { parts: parts as unknown as JsonObject[] };
    if (model?.provider && model?.model) {
      payload.model = {
        providerID: model.provider,
        modelID: model.model,
      };
    }
    if (routing?.toolMode) payload.toolMode = routing.toolMode;
    if (routing?.toolAllowlist?.length) payload.toolAllowlist = routing.toolAllowlist;
    if (routing?.contextMode) payload.contextMode = routing.contextMode;
    const path = `/session/${encodeURIComponent(sessionId)}/prompt_async?return=run`;

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    let res: Response;
    try {
      res = await fetch(`${this.baseUrl}${path}`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${this.token}`,
        },
        body: JSON.stringify(payload),
        signal: controller.signal,
      });
    } finally {
      clearTimeout(timer);
    }

    if (res.status === 409) {
      const conflict = (await res.json().catch(() => ({}))) as JsonObject;
      const active = conflict.activeRun as JsonObject | undefined;
      const conflictId =
        asString(active?.runID) || asString(active?.runId) || asString(active?.run_id);
      if (conflictId) return { runId: conflictId };
    }
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(`promptAsyncParts failed (${res.status}): ${body}`);
    }
    const data = (await res.json()) as JsonObject;
    return { runId: parseRunId(data) };
  }

  /**
   * Run a prompt synchronously and return the text reply (blocking).
   * For long tasks prefer `promptAsync` + `stream()`.
   */
  async promptSync(sessionId: string, prompt: string): Promise<string> {
    const payload: JsonObject = { parts: [{ type: "text", text: prompt }] };
    const data = await this.req<{ reply?: string; text?: string; output?: string } & JsonObject>(
      `/session/${encodeURIComponent(sessionId)}/prompt_sync`,
      { method: "POST", body: JSON.stringify(payload) }
    );
    return asString(data.reply) || asString(data.text) || asString(data.output) || "";
  }

  /**
   * Abort the active run for a session.
   */
  async abort(sessionId: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/session/${encodeURIComponent(sessionId)}/abort`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  }

  /**
   * Cancel the session's active run (alias of abort on some engine versions).
   */
  async cancel(sessionId: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/session/${encodeURIComponent(sessionId)}/cancel`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  }

  /**
   * Cancel a specific run within a session.
   */
  async cancelRun(sessionId: string, runId: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(
      `/session/${encodeURIComponent(sessionId)}/run/${encodeURIComponent(runId)}/cancel`,
      { method: "POST", body: JSON.stringify({}) }
    );
  }

  /**
   * Fork a session into a child session (divergent conversation branch).
   */
  async fork(sessionId: string): Promise<SessionRecord> {
    const raw = await this.req<unknown>(`/session/${encodeURIComponent(sessionId)}/fork`, {
      method: "POST",
      body: JSON.stringify({}),
    });
    return parseResponse(SessionRecordSchema, raw, `/session/${sessionId}/fork`, 200);
  }

  /**
   * Get the workspace diff produced by the session's last run.
   */
  async diff(sessionId: string): Promise<SessionDiff> {
    return this.req<SessionDiff>(`/session/${encodeURIComponent(sessionId)}/diff`);
  }

  /**
   * Revert uncommitted workspace changes made by the session.
   */
  async revert(sessionId: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/session/${encodeURIComponent(sessionId)}/revert`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  }

  /**
   * Undo a previous revert (restore session changes).
   */
  async unrevert(sessionId: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/session/${encodeURIComponent(sessionId)}/unrevert`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  }

  /**
   * Get child sessions forked from this session.
   */
  async children(sessionId: string): Promise<SessionRecord[]> {
    const raw = await this.req<unknown>(`/session/${encodeURIComponent(sessionId)}/children`);
    const parsed = parseResponse<SessionListResponse>(
      SessionListResponseSchema,
      raw,
      `/session/${sessionId}/children`,
      200
    );
    return parsed.sessions;
  }

  /**
   * Trigger an engine-side summarization of the session's conversation history.
   */
  async summarize(sessionId: string): Promise<{ ok: boolean; summary?: string }> {
    return this.req<{ ok: boolean; summary?: string }>(
      `/session/${encodeURIComponent(sessionId)}/summarize`,
      { method: "POST", body: JSON.stringify({}) }
    );
  }

  /**
   * Attach a session to a different workspace directory.
   */
  async attach(sessionId: string, targetWorkspace: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/session/${encodeURIComponent(sessionId)}/attach`, {
      method: "POST",
      body: JSON.stringify({ target_workspace: targetWorkspace }),
    });
  }
}

// ─── Permissions namespace ────────────────────────────────────────────────────

class Permissions {
  constructor(private req: TandemClient["_request"]) {}

  /** List all pending permission requests and existing rules. */
  async list(): Promise<PermissionSnapshotResponse> {
    return this.req<PermissionSnapshotResponse>("/permission");
  }

  /** Reply to a permission request. Use "always" to auto-approve future requests. */
  async reply(requestId: string, reply: PermissionReply): Promise<{ ok: boolean }> {
    const res = await this.req<{ ok?: boolean; error?: string }>(
      `/permission/${encodeURIComponent(requestId)}/reply`,
      { method: "POST", body: JSON.stringify({ reply }) }
    );
    if (!res.ok) throw new Error(`Permission reply rejected: ${res.error ?? requestId}`);
    return { ok: true };
  }
}

// ─── Questions namespace ──────────────────────────────────────────────────────

class Questions {
  constructor(private req: TandemClient["_request"]) {}

  /** List pending AI-generated questions awaiting user confirmation. */
  async list(): Promise<QuestionsListResponse> {
    const raw = await this.req<unknown>("/question");
    if (Array.isArray(raw)) return { questions: raw as QuestionRecord[] };
    return raw as QuestionsListResponse;
  }

  /** Answer a pending question. */
  async reply(questionId: string, answer: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/question/${encodeURIComponent(questionId)}/reply`, {
      method: "POST",
      body: JSON.stringify({ answer }),
    });
  }

  /** Reject/dismiss a pending question. */
  async reject(questionId: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/question/${encodeURIComponent(questionId)}/reject`, {
      method: "POST",
      body: JSON.stringify({}),
    });
  }
}

// ─── Providers namespace ──────────────────────────────────────────────────────

class Providers {
  constructor(private req: TandemClient["_request"]) {}

  /** List all available providers and their models. */
  async catalog(): Promise<ProviderCatalog> {
    return this.req<ProviderCatalog>("/provider");
  }

  /** Get the current provider/model configuration. */
  async config(): Promise<ProvidersConfigResponse> {
    return this.req<ProvidersConfigResponse>("/config/providers");
  }

  /** Set the default provider and model. */
  async setDefaults(providerId: string, modelId: string): Promise<void> {
    await this.req<void>("/config", {
      method: "PATCH",
      body: JSON.stringify({
        default_provider: providerId,
        providers: { [providerId]: { default_model: modelId } },
      }),
    });
  }

  /** Store an API key for a provider. */
  async setApiKey(providerId: string, apiKey: string): Promise<void> {
    await this.req<void>(`/auth/${encodeURIComponent(providerId)}`, {
      method: "PUT",
      body: JSON.stringify({ apiKey }),
    });
  }

  /** Start a provider-owned OAuth flow. */
  async oauthAuthorize(providerId: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/provider/${encodeURIComponent(providerId)}/oauth/authorize`, {
      method: "POST",
    });
  }

  /** Poll provider OAuth session state. */
  async oauthStatus(providerId: string, sessionId?: string): Promise<JsonObject> {
    const suffix = sessionId ? `?session_id=${encodeURIComponent(sessionId)}` : "";
    return this.req<JsonObject>(
      `/provider/${encodeURIComponent(providerId)}/oauth/status${suffix}`
    );
  }

  /** Import a local CLI-managed OAuth session for a provider. */
  async oauthUseLocalSession(providerId: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/provider/${encodeURIComponent(providerId)}/oauth/session/local`, {
      method: "POST",
    });
  }

  /** Disconnect a provider-owned OAuth session. */
  async oauthDisconnect(providerId: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/provider/${encodeURIComponent(providerId)}/oauth/session`, {
      method: "DELETE",
    });
  }

  /** Get authentication status for a provider. */
  async authStatus(): Promise<JsonObject> {
    return this.req<JsonObject>("/provider/auth");
  }
}

class Identity {
  constructor(private req: TandemClient["_request"]) {}

  /** Get current bot identity + personality config and available presets. */
  async get(): Promise<IdentityConfigResponse> {
    return this.req<IdentityConfigResponse>("/config/identity");
  }

  /** Patch bot identity/personality configuration. */
  async patch(identity: IdentityConfig): Promise<IdentityConfigResponse> {
    return this.req<IdentityConfigResponse>("/config/identity", {
      method: "PATCH",
      body: JSON.stringify(identity),
    });
  }
}

// ─── Channels namespace ───────────────────────────────────────────────────────

class Channels {
  constructor(private req: TandemClient["_request"]) {}

  /** Get channel configuration (Telegram / Discord / Slack). */
  async config(): Promise<ChannelsConfigResponse> {
    return this.req<ChannelsConfigResponse>("/channels/config");
  }

  /** Get live channel connection status. */
  async status(): Promise<ChannelsStatusResponse> {
    return this.req<ChannelsStatusResponse>("/channels/status");
  }

  /** Configure a channel (bot token, allowed users, etc.). */
  async put(channel: ChannelName, payload: JsonObject): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/channels/${channel}`, {
      method: "PUT",
      body: JSON.stringify(payload),
    });
  }

  /** Remove a channel configuration. */
  async delete(channel: ChannelName): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/channels/${channel}`, { method: "DELETE" });
  }

  /** Verify channel connectivity and prerequisites (token, gateway, intents, etc.). */
  async verify(channel: ChannelName, payload: JsonObject = {}): Promise<ChannelVerifyResponse> {
    return this.req<ChannelVerifyResponse>(`/channels/${channel}/verify`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Read per-channel tool preferences for channel-created sessions. */
  async toolPreferences(channel: ChannelName): Promise<ChannelToolPreferences> {
    return this.req<ChannelToolPreferences>(`/channels/${channel}/tool-preferences`);
  }

  /** Update per-channel tool preferences for channel-created sessions. */
  async setToolPreferences(
    channel: ChannelName,
    payload: ChannelToolPreferencesInput
  ): Promise<ChannelToolPreferences> {
    return this.req<ChannelToolPreferences>(`/channels/${channel}/tool-preferences`, {
      method: "PUT",
      body: JSON.stringify(payload),
    });
  }
}

// ─── MCP namespace ────────────────────────────────────────────────────────────

class Mcp {
  constructor(
    private req: TandemClient["_request"],
    private reqText?: TandemClient["_requestText"]
  ) {}

  /** List registered MCP servers. */
  async list(): Promise<Record<string, unknown>> {
    return this.req<Record<string, unknown>>("/mcp");
  }

  /** List all discovered MCP tools. */
  async listTools(): Promise<unknown[]> {
    return this.req<unknown[]>("/mcp/tools");
  }

  /** List all discovered MCP resources. */
  async listResources(): Promise<unknown[]> {
    const raw = await this.req<unknown>("/mcp/resources");
    return Array.isArray(raw) ? raw : [];
  }

  /**
   * Register a new MCP server.
   *
   * @example
   * ```typescript
   * await client.mcp.add({ name: "arcade", transport: "https://mcp.arcade.ai/mcp" });
   * await client.mcp.connect("arcade");
   * ```
   */
  async add(options: AddMcpServerOptions): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>("/mcp", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Connect to an MCP server and discover its tools. */
  async connect(name: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}/connect`, {
      method: "POST",
    });
  }

  /** Disconnect from an MCP server. */
  async disconnect(name: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}/disconnect`, {
      method: "POST",
    });
  }

  /** Re-discover tools from a connected MCP server. */
  async refresh(name: string): Promise<{ ok: boolean; count?: number }> {
    return this.req<{ ok: boolean; count?: number }>(`/mcp/${encodeURIComponent(name)}/refresh`, {
      method: "POST",
    });
  }

  /** Enable or disable an MCP server. */
  async setEnabled(name: string, enabled: boolean): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/mcp/${encodeURIComponent(name)}`, {
      method: "PATCH",
      body: JSON.stringify({ enabled }),
    });
  }

  async delete(name: string): Promise<{ ok?: boolean; deleted?: boolean }> {
    return this.req<{ ok?: boolean; deleted?: boolean }>(`/mcp/${encodeURIComponent(name)}`, {
      method: "DELETE",
    });
  }

  async auth(name: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/mcp/${encodeURIComponent(name)}/auth`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  async deleteAuth(name: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/mcp/${encodeURIComponent(name)}/auth`, {
      method: "DELETE",
    });
  }

  async authCallback(name: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/mcp/${encodeURIComponent(name)}/auth/callback`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async authenticate(name: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/mcp/${encodeURIComponent(name)}/auth/authenticate`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  async catalogToml(slug: string): Promise<string> {
    if (!this.reqText) throw new Error("Text request helper unavailable");
    return this.reqText(`/mcp/catalog/${encodeURIComponent(slug)}/toml`);
  }
}

// ─── Memory namespace ─────────────────────────────────────────────────────────

class Memory {
  constructor(private req: TandemClient["_request"]) {}

  /**
   * Store a memory item.
   *
   * @example
   * ```typescript
   * await client.memory.put({
   *   text: "The team uses Rust for backend services.",
   *   tags: ["team", "architecture"],
   * });
   * ```
   */
  async put(options: MemoryPutOptions): Promise<MemoryPutResponse> {
    return this.req<MemoryPutResponse>("/memory/put", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /**
   * Semantic search over stored memories.
   *
   * @example
   * ```typescript
   * const { results } = await client.memory.search({
   *   query: "backend technology choices",
   *   limit: 5,
   * });
   * ```
   */
  async search(options: MemorySearchOptions): Promise<MemorySearchResponse> {
    const raw = await this.req<unknown>("/memory/search", {
      method: "POST",
      body: JSON.stringify(options),
    });
    return parseResponse(MemorySearchResponseSchema, raw, "/memory/search", 200);
  }

  /** List stored memory items with optional text filter. */
  async list(options?: {
    q?: string;
    limit?: number;
    offset?: number;
    userId?: string;
  }): Promise<MemoryListResponse> {
    const params = new URLSearchParams();
    if (options?.q) params.set("q", options.q);
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    if (options?.offset !== undefined) params.set("offset", String(options.offset));
    if (options?.userId) params.set("user_id", options.userId);
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(`/memory${qs}`);
    return parseResponse(MemoryListResponseSchema, raw, "/memory", 200);
  }

  /** Delete a memory item by ID. */
  async delete(memoryId: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/memory/${encodeURIComponent(memoryId)}`, {
      method: "DELETE",
    });
  }

  /** Promote a transient memory item to persistent storage. */
  async promote(options: MemoryPromoteOptions): Promise<MemoryPromoteResponse> {
    return this.req<MemoryPromoteResponse>("/memory/promote", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Demote a memory item back to private/demoted state. */
  async demote(options: MemoryDemoteOptions): Promise<MemoryDemoteResponse> {
    const payload = {
      id: options.id,
      run_id: options.runId,
    };
    return this.req<MemoryDemoteResponse>("/memory/demote", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Retrieve the memory audit log for a run. */
  async audit(options?: { run_id?: string; limit?: number }): Promise<MemoryAuditResponse> {
    const params = new URLSearchParams();
    if (options?.run_id) params.set("run_id", options.run_id);
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(`/memory/audit${qs}`);
    if (Array.isArray(raw)) return { entries: raw as JsonObject[], count: raw.length };
    return raw as MemoryAuditResponse;
  }

  /** Resolve a context URI to a memory node. */
  async contextResolveUri(uri: string): Promise<{ node?: JsonObject }> {
    const raw = await this.req<unknown>("/memory/context/resolve", {
      method: "POST",
      body: JSON.stringify({ uri }),
    });
    return parseResponse(ContextResolveResponseSchema, raw, "/memory/context/resolve", 200);
  }

  /** Get a tree of memory nodes starting from a URI. */
  async contextTree(uri: string, maxDepth?: number): Promise<{ tree: JsonObject[] }> {
    const params = new URLSearchParams();
    params.set("uri", uri);
    if (maxDepth !== undefined) params.set("max_depth", String(maxDepth));
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(`/memory/context/tree${qs}`);
    return parseResponse(ContextTreeResponseSchema, raw, "/memory/context/tree", 200);
  }

  /** Generate L0/L1 layers for a memory node. */
  async contextGenerateLayers(nodeId: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>("/memory/context/layers/generate", {
      method: "POST",
      body: JSON.stringify({ node_id: nodeId }),
    });
  }

  /** Distill a session's conversation into memories. */
  async contextDistill(
    sessionId: string,
    conversation: string[]
  ): Promise<{
    ok: boolean;
    distillation_id?: string;
    session_id?: string;
    facts_extracted?: number;
  }> {
    const raw = await this.req<unknown>("/memory/context/distill", {
      method: "POST",
      body: JSON.stringify({ session_id: sessionId, conversation }),
    });
    return parseResponse(ContextDistillResponseSchema, raw, "/memory/context/distill", 200);
  }
}

// ─── Skills namespace ─────────────────────────────────────────────────────────

class Skills {
  constructor(private req: TandemClient["_request"]) {}

  /** List installed agent skills. */
  async list(location?: SkillLocation): Promise<SkillsListResponse> {
    const qs = location ? `?location=${encodeURIComponent(location)}` : "";
    const raw = await this.req<unknown>(`/skills${qs}`);
    if (Array.isArray(raw)) return { skills: raw as SkillRecord[], count: raw.length };
    return raw as SkillsListResponse;
  }

  /** Get details of a specific skill by name. */
  async get(name: string): Promise<SkillRecord> {
    return this.req<SkillRecord>(`/skills/${encodeURIComponent(name)}`);
  }

  /** Import a skill from YAML content or a file path. */
  async import(options: SkillImportOptions): Promise<SkillImportResponse> {
    const payload = {
      content: options.content,
      file_or_path: (options as { file_or_path?: string }).file_or_path ?? options.fileOrPath,
      location: options.location,
      namespace: options.namespace,
      conflict_policy:
        (options as { conflict_policy?: "skip" | "overwrite" | "rename" }).conflict_policy ??
        options.conflictPolicy,
    };
    return this.req<SkillImportResponse>("/skills/import", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Preview a skill import (dry run). */
  async preview(options: SkillImportOptions): Promise<SkillImportResponse> {
    const payload = {
      content: options.content,
      file_or_path: (options as { file_or_path?: string }).file_or_path ?? options.fileOrPath,
      location: options.location,
      namespace: options.namespace,
      conflict_policy:
        (options as { conflict_policy?: "skip" | "overwrite" | "rename" }).conflict_policy ??
        options.conflictPolicy,
    };
    return this.req<SkillImportResponse>("/skills/import/preview", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** List available skill templates shipped with the engine. */
  async templates(): Promise<SkillTemplatesResponse> {
    const raw = await this.req<unknown>("/skills/templates");
    if (Array.isArray(raw)) return { templates: raw as SkillTemplate[], count: raw.length };
    return raw as SkillTemplatesResponse;
  }

  /** List enriched skill catalog records. */
  async catalog(): Promise<{ skills: SkillCatalogRecord[]; count: number }> {
    const raw = await this.req<unknown>("/skills/catalog");
    const rows = Array.isArray(raw) ? (raw as SkillCatalogRecord[]) : [];
    return { skills: rows, count: rows.length };
  }

  /** Validate SKILL.md content or a local path/zip. */
  async validate(options: {
    content?: string;
    fileOrPath?: string;
    file_or_path?: string;
  }): Promise<SkillValidationResponse> {
    const payload = {
      content: options.content,
      file_or_path: options.file_or_path ?? options.fileOrPath,
    };
    return this.req<SkillValidationResponse>("/skills/validate", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Match a user goal to the best skill candidate. */
  async match(options: {
    goal: string;
    maxMatches?: number;
    threshold?: number;
    max_matches?: number;
  }): Promise<SkillRouterMatchResponse> {
    const payload = {
      goal: options.goal,
      max_matches: options.max_matches ?? options.maxMatches,
      threshold: options.threshold,
    };
    return this.req<SkillRouterMatchResponse>("/skills/router/match", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Evaluate routing against benchmark cases. */
  async evalBenchmark(options: {
    cases: Array<{ prompt: string; expectedSkill?: string; expected_skill?: string }>;
    threshold?: number;
  }): Promise<SkillsBenchmarkEvalResponse> {
    const payload = {
      threshold: options.threshold,
      cases: options.cases.map((row) => ({
        prompt: row.prompt,
        expected_skill: row.expected_skill ?? row.expectedSkill,
      })),
    };
    return this.req<SkillsBenchmarkEvalResponse>("/skills/evals/benchmark", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Evaluate trigger quality for a single target skill. */
  async evalTriggers(options: {
    skillName?: string;
    skill_name?: string;
    prompts: string[];
    threshold?: number;
  }): Promise<SkillsTriggerEvalResponse> {
    const payload = {
      skill_name: options.skill_name ?? options.skillName,
      prompts: options.prompts,
      threshold: options.threshold,
    };
    return this.req<SkillsTriggerEvalResponse>("/skills/evals/triggers", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Compile a selected or routed skill into an execution summary. */
  async compile(options: {
    skillName?: string;
    skill_name?: string;
    goal?: string;
    threshold?: number;
    maxMatches?: number;
    max_matches?: number;
    schedule?: JsonObject;
  }): Promise<SkillCompileResponse> {
    const payload = {
      skill_name: options.skill_name ?? options.skillName,
      goal: options.goal,
      threshold: options.threshold,
      max_matches: options.max_matches ?? options.maxMatches,
      schedule: options.schedule,
    };
    return this.req<SkillCompileResponse>("/skills/compile", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Generate scaffold skill artifacts from a natural-language prompt. */
  async generate(options: { prompt: string; threshold?: number }): Promise<SkillGenerateResponse> {
    return this.req<SkillGenerateResponse>("/skills/generate", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Install generated or custom artifacts into local skills. */
  async generateInstall(options: {
    prompt?: string;
    threshold?: number;
    location?: SkillLocation;
    conflictPolicy?: "skip" | "overwrite" | "rename";
    artifacts?: {
      "SKILL.md"?: string;
      "workflow.yaml"?: string;
      "automation.example.yaml"?: string;
    };
  }): Promise<SkillGenerateInstallResponse> {
    const payload = {
      prompt: options.prompt,
      threshold: options.threshold,
      location: options.location,
      conflict_policy: options.conflictPolicy,
      artifacts: options.artifacts
        ? {
            "SKILL.md": options.artifacts["SKILL.md"],
            "workflow.yaml": options.artifacts["workflow.yaml"],
            "automation.example.yaml": options.artifacts["automation.example.yaml"],
          }
        : undefined,
    };
    return this.req<SkillGenerateInstallResponse>("/skills/generate/install", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }
}

// ─── Packs namespace ──────────────────────────────────────────────────────────

class Packs {
  constructor(private req: TandemClient["_request"]) {}

  /** List installed tandem packs. */
  async list(): Promise<PacksListResponse> {
    return this.req<PacksListResponse>("/packs");
  }

  /** Inspect an installed pack by `pack_id` or `name`. */
  async inspect(selector: string): Promise<PackInspectionResponse> {
    return this.req<PackInspectionResponse>(`/packs/${encodeURIComponent(selector)}`);
  }

  /** Install a pack from local path or URL. */
  async install(options: PackInstallOptions): Promise<{ installed: PackInstallRecord }> {
    return this.req<{ installed: PackInstallRecord }>("/packs/install", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Install a pack from a downloaded attachment path. */
  async installFromAttachment(options: {
    attachment_id: string;
    path: string;
    connector?: string;
    channel_id?: string;
    sender_id?: string;
  }): Promise<{ installed: PackInstallRecord }> {
    return this.req<{ installed: PackInstallRecord }>("/packs/install_from_attachment", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Uninstall a pack by `pack_id` or `name` (+ optional version). */
  async uninstall(options: PackUninstallOptions): Promise<{ removed: PackInstallRecord }> {
    return this.req<{ removed: PackInstallRecord }>("/packs/uninstall", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Export an installed pack to zip. */
  async export(options: PackExportOptions): Promise<{
    exported: { path: string; sha256: string; bytes: number };
  }> {
    return this.req<{
      exported: { path: string; sha256: string; bytes: number };
    }>("/packs/export", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Detect `tandempack.yaml` marker in zip path. */
  async detect(options: PackDetectOptions): Promise<{ is_pack: boolean; marker: string }> {
    return this.req<{ is_pack: boolean; marker: string }>("/packs/detect", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Check available updates for a pack (stub endpoint in v0.4.0). */
  async updates(selector: string): Promise<{
    pack_id?: string;
    name?: string;
    current_version?: string;
    updates: JsonObject[];
  }> {
    const raw = await this.req<unknown>(`/packs/${encodeURIComponent(selector)}/updates`);
    const asObj = (raw || {}) as JsonObject;
    const updates = Array.isArray(asObj.updates) ? (asObj.updates as JsonObject[]) : [];
    return {
      pack_id: asString(asObj.pack_id) ?? undefined,
      name: asString(asObj.name) ?? undefined,
      current_version: asString(asObj.current_version) ?? undefined,
      updates,
    };
  }

  /** Apply a pack update (stub endpoint in v0.4.0). */
  async update(
    selector: string,
    options?: { target_version?: string }
  ): Promise<{
    updated: boolean;
    pack_id?: string;
    name?: string;
    current_version?: string;
    target_version?: string;
    reason?: string;
  }> {
    const raw = await this.req<JsonObject>(`/packs/${encodeURIComponent(selector)}/update`, {
      method: "POST",
      body: JSON.stringify(options ?? {}),
    });
    return {
      updated: Boolean(raw.updated),
      pack_id: asString(raw.pack_id) ?? undefined,
      name: asString(raw.name) ?? undefined,
      current_version: asString(raw.current_version) ?? undefined,
      target_version: asString(raw.target_version) ?? undefined,
      reason: asString(raw.reason) ?? undefined,
    };
  }
}

// ─── Capabilities namespace ───────────────────────────────────────────────────

class Capabilities {
  constructor(private req: TandemClient["_request"]) {}

  /** Get current capability bindings file. */
  async getBindings(): Promise<CapabilityBindingsFile> {
    const raw = await this.req<{ bindings: CapabilityBindingsFile }>("/capabilities/bindings");
    return raw.bindings;
  }

  /** Replace capability bindings file. */
  async setBindings(bindings: CapabilityBindingsFile): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>("/capabilities/bindings", {
      method: "PUT",
      body: JSON.stringify(bindings),
    });
  }

  /** Discover provider tools available for resolution. */
  async discovery(): Promise<{
    tools: Array<{ provider: string; tool_name: string; schema?: JsonObject }>;
  }> {
    return this.req<{
      tools: Array<{ provider: string; tool_name: string; schema?: JsonObject }>;
    }>("/capabilities/discovery");
  }

  /**
   * Resolve capability IDs to provider tools.
   * Returns resolver payload on success; throws on HTTP errors (including 409 missing capability).
   */
  async resolve(input: CapabilityResolveInput): Promise<{ resolution: JsonObject }> {
    return this.req<{ resolution: JsonObject }>("/capabilities/resolve", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }

  /** Evaluate runtime readiness for required capabilities and return blocking issues. */
  async readiness(
    input: CapabilityReadinessInput
  ): Promise<{ readiness: CapabilityReadinessOutput }> {
    return this.req<{ readiness: CapabilityReadinessOutput }>("/capabilities/readiness", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }
}

// ─── Resources namespace ──────────────────────────────────────────────────────

class Resources {
  constructor(
    private baseUrl: string,
    private getToken: () => string,
    private req: TandemClient["_request"]
  ) {}

  /** List stored resource records. */
  async list(options?: { prefix?: string; limit?: number }): Promise<ResourceListResponse> {
    const params = new URLSearchParams();
    if (options?.prefix) params.set("prefix", options.prefix);
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(`/resource${qs}`);
    return parseResponse(ResourceListResponseSchema, raw, "/resource", 200);
  }

  /**
   * Write a resource key-value entry.
   *
   * @example
   * ```typescript
   * await client.resources.write({
   *   key: "agent-config/alert-threshold",
   *   value: { threshold: 0.95 },
   * });
   * ```
   */
  async write(options: ResourceWriteOptions): Promise<ResourceWriteResponse> {
    return this.req<ResourceWriteResponse>("/resource", {
      method: "PUT",
      body: JSON.stringify(options),
    });
  }

  async get(key: string): Promise<ResourceRecord> {
    const raw = await this.req<unknown>(`/resource/${encodeURIComponent(key)}`);
    return parseResponse(ResourceRecordSchema, raw, `/resource/${key}`, 200);
  }

  async putKey(
    key: string,
    value: JsonValue,
    options?: JsonObject
  ): Promise<ResourceWriteResponse> {
    const raw = await this.req<unknown>(`/resource/${encodeURIComponent(key)}`, {
      method: "PUT",
      body: JSON.stringify({ value, ...(options ?? {}) }),
    });
    return parseResponse(ResourceWriteResponseSchema, raw, `/resource/${key}`, 200);
  }

  async patchKey(key: string, patch: JsonObject): Promise<ResourceWriteResponse> {
    const raw = await this.req<unknown>(`/resource/${encodeURIComponent(key)}`, {
      method: "PATCH",
      body: JSON.stringify(patch),
    });
    return parseResponse(ResourceWriteResponseSchema, raw, `/resource/${key}`, 200);
  }

  /** Delete a resource entry. */
  async delete(key: string, options?: { if_match_rev?: number }): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>("/resource", {
      method: "DELETE",
      body: JSON.stringify({ key, ...options }),
    });
  }

  async deleteKey(key: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/resource/${encodeURIComponent(key)}`, {
      method: "DELETE",
    });
  }

  events(options?: { sinceSeq?: number; tail?: number }): AsyncGenerator<EngineEvent> {
    const params = new URLSearchParams();
    if (options?.sinceSeq !== undefined) params.set("since_seq", String(options.sinceSeq));
    if (options?.tail !== undefined) params.set("tail", String(options.tail));
    const qs = params.toString() ? `?${params.toString()}` : "";
    return streamSse(`${this.baseUrl}/resource/events${qs}`, this.getToken());
  }
}

// ─── Coder namespace ─────────────────────────────────────────────────────────

class Coder {
  constructor(private req: TandemClient["_request"]) {}

  /** Create a coder workflow run. */
  async createRun(payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>("/coder/runs", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** List coder runs with optional filters. */
  async listRuns(options?: {
    limit?: number;
    workflowMode?: string;
    workflow_mode?: string;
    repoSlug?: string;
    repo_slug?: string;
  }): Promise<CoderRunsListResponse> {
    const params = new URLSearchParams();
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const workflowMode = options?.workflow_mode ?? options?.workflowMode;
    const repoSlug = options?.repo_slug ?? options?.repoSlug;
    if (workflowMode) params.set("workflow_mode", workflowMode);
    if (repoSlug) params.set("repo_slug", repoSlug);
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(`/coder/runs${qs}`);
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    const runs = Array.isArray(asObj.runs) ? (asObj.runs as CoderRunRecord[]) : [];
    return {
      runs,
      count: typeof asObj.count === "number" ? asObj.count : runs.length,
    };
  }

  /** Get a single coder run plus linked context run details. */
  async getRun(runId: string): Promise<CoderRunGetResponse> {
    const raw = await this.req<unknown>(`/coder/runs/${encodeURIComponent(runId)}`);
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    return {
      ...asObj,
      coderRun: (asObj.coder_run as CoderRunRecord | undefined) ?? undefined,
      coder_run: (asObj.coder_run as CoderRunRecord | undefined) ?? undefined,
      run: (asObj.run as JsonObject | undefined) ?? undefined,
    };
  }

  /** Load project-scoped coder binding metadata. */
  async getProjectBinding(projectId: string): Promise<CoderProjectBindingGetResponse> {
    const raw = await this.req<unknown>(
      `/coder/projects/${encodeURIComponent(projectId)}/bindings`
    );
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    return {
      ...asObj,
      binding: (asObj.binding as unknown as CoderProjectBindingGetResponse["binding"]) ?? null,
    };
  }

  /** Save project-scoped coder binding metadata. */
  async putProjectBinding(
    projectId: string,
    payload: JsonObject
  ): Promise<CoderProjectBindingPutResponse> {
    const raw = await this.req<unknown>(
      `/coder/projects/${encodeURIComponent(projectId)}/bindings`,
      {
        method: "PUT",
        body: JSON.stringify(payload),
      }
    );
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    return {
      ...asObj,
      binding: asObj.binding as unknown as CoderProjectBindingPutResponse["binding"],
    };
  }

  /** List GitHub Project inbox items for a coder project binding. */
  async getProjectGithubInbox(projectId: string): Promise<CoderGithubProjectInboxResponse> {
    const raw = await this.req<unknown>(
      `/coder/projects/${encodeURIComponent(projectId)}/github-project/inbox`
    );
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    return {
      ...asObj,
      items: Array.isArray(asObj.items)
        ? (asObj.items as unknown as CoderGithubProjectInboxResponse["items"])
        : [],
      binding: asObj.binding as unknown as CoderGithubProjectInboxResponse["binding"],
      project_id: String(asObj.project_id || asObj.projectId || ""),
      schema_drift: Boolean(asObj.schema_drift ?? asObj.schemaDrift),
      live_schema_fingerprint: String(
        asObj.live_schema_fingerprint || asObj.liveSchemaFingerprint || ""
      ),
    };
  }

  /** Intake a GitHub Project item into a Tandem coder run lineage. */
  async intakeProjectItem(
    projectId: string,
    payload: JsonObject
  ): Promise<CoderGithubProjectIntakeResponse> {
    const raw = await this.req<unknown>(
      `/coder/projects/${encodeURIComponent(projectId)}/github-project/intake`,
      {
        method: "POST",
        body: JSON.stringify(payload),
      }
    );
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    return {
      ...asObj,
      coder_run: (asObj.coder_run as CoderRunRecord | undefined) ?? undefined,
      coderRun: (asObj.coder_run as CoderRunRecord | undefined) ?? undefined,
      run: (asObj.run as JsonObject | undefined) ?? undefined,
    };
  }

  /** Execute the next runnable task in a coder workflow. */
  async executeNext(runId: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/execute-next`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  /** Continue executing runnable tasks until the run stops or completes. */
  async executeAll(runId: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/execute-all`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  /** Spawn a follow-on coder workflow from an existing run. */
  async createFollowOnRun(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/follow-on-run`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Approve a coder run that is waiting on human review. */
  async approveRun(runId: string, reason?: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/approve`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    });
  }

  /** Cancel a coder run. */
  async cancelRun(runId: string, reason?: string): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/cancel`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    });
  }

  /** List artifacts emitted by a coder run. */
  async listArtifacts(runId: string): Promise<CoderArtifactsResponse> {
    const raw = await this.req<unknown>(`/coder/runs/${encodeURIComponent(runId)}/artifacts`);
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    const artifacts = Array.isArray(asObj.artifacts)
      ? (asObj.artifacts as CoderArtifactRecord[])
      : [];
    return {
      ...asObj,
      artifacts,
      count: typeof asObj.count === "number" ? asObj.count : artifacts.length,
    };
  }

  /** Inspect ranked memory hits for a coder run. */
  async getMemoryHits(
    runId: string,
    options?: { query?: string; limit?: number }
  ): Promise<CoderMemoryHitsResponse> {
    const params = new URLSearchParams();
    if (options?.query) params.set("q", options.query);
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const qs = params.toString() ? `?${params.toString()}` : "";
    const raw = await this.req<unknown>(
      `/coder/runs/${encodeURIComponent(runId)}/memory-hits${qs}`
    );
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    const hits = Array.isArray(asObj.hits) ? (asObj.hits as CoderMemoryHitRecord[]) : [];
    return {
      ...asObj,
      hits,
      count: typeof asObj.count === "number" ? asObj.count : hits.length,
    };
  }

  /** Create a triage inspection report artifact. */
  async createTriageInspectionReport(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(
      `/coder/runs/${encodeURIComponent(runId)}/triage-inspection-report`,
      {
        method: "POST",
        body: JSON.stringify(payload),
      }
    );
  }

  /** Create a triage reproduction report artifact. */
  async createTriageReproductionReport(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(
      `/coder/runs/${encodeURIComponent(runId)}/triage-reproduction-report`,
      {
        method: "POST",
        body: JSON.stringify(payload),
      }
    );
  }

  /** Create a triage summary artifact. */
  async createTriageSummary(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/triage-summary`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Create PR review evidence for a coder run. */
  async createPrReviewEvidence(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/pr-review-evidence`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Create a PR review summary artifact. */
  async createPrReviewSummary(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/pr-review-summary`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Create an issue-fix validation report artifact. */
  async createIssueFixValidationReport(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(
      `/coder/runs/${encodeURIComponent(runId)}/issue-fix-validation-report`,
      {
        method: "POST",
        body: JSON.stringify(payload),
      }
    );
  }

  /** Create an issue-fix summary artifact. */
  async createIssueFixSummary(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/issue-fix-summary`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Draft a pull request for an issue-fix coder run. */
  async createPrDraft(runId: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/pr-draft`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  /** Submit a pull request for an issue-fix coder run. */
  async submitPr(runId: string, payload?: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/pr-submit`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });
  }

  /** Create a merge readiness report artifact. */
  async createMergeReadinessReport(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/merge-readiness-report`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Create a merge recommendation summary artifact. */
  async createMergeRecommendationSummary(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(
      `/coder/runs/${encodeURIComponent(runId)}/merge-recommendation-summary`,
      {
        method: "POST",
        body: JSON.stringify(payload),
      }
    );
  }

  /** List pending or emitted memory candidates for a coder run. */
  async listMemoryCandidates(runId: string): Promise<CoderMemoryCandidatesResponse> {
    const raw = await this.req<unknown>(
      `/coder/runs/${encodeURIComponent(runId)}/memory-candidates`
    );
    const asObj = ((raw as JsonObject | null) ?? {}) as JsonObject;
    const candidates = Array.isArray(asObj.candidates)
      ? (asObj.candidates as CoderMemoryCandidateRecord[])
      : [];
    return {
      ...asObj,
      candidates,
      count: typeof asObj.count === "number" ? asObj.count : candidates.length,
    };
  }

  /** Persist a memory candidate generated by a coder workflow. */
  async createMemoryCandidate(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/coder/runs/${encodeURIComponent(runId)}/memory-candidates`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Promote a reviewed memory candidate into governed memory. */
  async promoteMemoryCandidate(
    runId: string,
    candidateId: string,
    payload?: JsonObject
  ): Promise<JsonObject> {
    return this.req<JsonObject>(
      `/coder/runs/${encodeURIComponent(runId)}/memory-candidates/${encodeURIComponent(candidateId)}/promote`,
      {
        method: "POST",
        body: JSON.stringify(payload ?? {}),
      }
    );
  }
}

// ─── Routines namespace ───────────────────────────────────────────────────────

class Routines {
  constructor(
    private baseUrl: string,
    private getToken: () => string,
    private req: TandemClient["_request"]
  ) {}

  /** List all scheduled routines. */
  async list(): Promise<DefinitionListResponse> {
    return this.req<DefinitionListResponse>("/routines");
  }

  /**
   * Create a scheduled routine.
   *
   * @example
   * ```typescript
   * await client.routines.create({
   *   name: "Daily digest",
   *   schedule: "0 8 * * *",
   *   entrypoint: "Summarize activity from the last 24 hours",
   * });
   * ```
   */
  async create(options: CreateRoutineOptions): Promise<DefinitionCreateResponse> {
    // Map `prompt` shorthand → `entrypoint` for engine compat
    const payload = { ...options };
    if ("prompt" in payload && !("entrypoint" in payload)) {
      (payload as Record<string, unknown>).entrypoint = payload.prompt;
    }
    return this.req<DefinitionCreateResponse>("/routines", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  /** Update a routine (partial patch). */
  async update(id: string, patch: PatchRoutineOptions): Promise<RoutineRecord> {
    return this.req<RoutineRecord>(`/routines/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(patch),
    });
  }

  /** Delete a routine by ID. */
  async delete(id: string): Promise<void> {
    await this.req<void>(`/routines/${encodeURIComponent(id)}`, { method: "DELETE" });
  }

  /** Trigger a routine immediately (run now). */
  async runNow(id: string): Promise<RunNowResponse> {
    const raw = await this.req<unknown>(`/routines/${encodeURIComponent(id)}/run_now`, {
      method: "POST",
      body: JSON.stringify({}),
    });
    return parseResponse(RunNowResponseSchema, raw, `/routines/${id}/run_now`, 200);
  }

  /** List recent runs across all routines. */
  async listRuns(options?: { routine_id?: string; limit?: number }): Promise<RunsListResponse> {
    const params = new URLSearchParams();
    if (options?.routine_id) params.set("routine_id", options.routine_id);
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const qs = params.toString() ? `?${params.toString()}` : "";
    return this.req<RunsListResponse>(`/routines/runs${qs}`);
  }

  /** List runs for a specific routine. */
  async getRunsForRoutine(id: string, limit = 25): Promise<RunsListResponse> {
    return this.req<RunsListResponse>(`/routines/${encodeURIComponent(id)}/runs?limit=${limit}`);
  }

  /** Get a specific run record. */
  async getRun(runId: string): Promise<RunRecord> {
    const raw = await this.req<unknown>(`/routines/runs/${encodeURIComponent(runId)}`);
    return parseResponse(RunRecordSchema, raw, `/routines/runs/${runId}`, 200);
  }

  /** List artifacts produced by a run. */
  async listArtifacts(runId: string): Promise<RunArtifactsResponse> {
    return this.req<RunArtifactsResponse>(`/routines/runs/${encodeURIComponent(runId)}/artifacts`);
  }

  /** Approve a run that requires human approval. */
  async approveRun(runId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/routines/runs/${encodeURIComponent(runId)}/approve`, {
      method: "POST",
      body: JSON.stringify({ reason: reason ?? "" }),
    });
  }

  /** Deny a run that requires human approval. */
  async denyRun(runId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/routines/runs/${encodeURIComponent(runId)}/deny`, {
      method: "POST",
      body: JSON.stringify({ reason: reason ?? "" }),
    });
  }

  /** Pause an active run. */
  async pauseRun(runId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/routines/runs/${encodeURIComponent(runId)}/pause`, {
      method: "POST",
      body: JSON.stringify({ reason: reason ?? "" }),
    });
  }

  /** Resume a paused run. */
  async resumeRun(runId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/routines/runs/${encodeURIComponent(runId)}/resume`, {
      method: "POST",
      body: JSON.stringify({ reason: reason ?? "" }),
    });
  }

  /** Get execution history for a routine. */
  async history(id: string, limit?: number): Promise<RoutineHistoryResponse> {
    const qs = limit !== undefined ? `?limit=${limit}` : "";
    const raw = await this.req<unknown>(`/routines/${encodeURIComponent(id)}/history${qs}`);
    if (Array.isArray(raw)) return { history: raw as JsonObject[], count: raw.length };
    return raw as RoutineHistoryResponse;
  }

  events(options?: { routineId?: string; routine_id?: string; signal?: AbortSignal }) {
    const params = new URLSearchParams();
    const routineId = options?.routine_id ?? options?.routineId;
    if (routineId) params.set("routine_id", routineId);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return streamSse(`${this.baseUrl}/routines/events${qs}`, this.getToken(), {
      signal: options?.signal,
    });
  }

  addArtifact(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/routines/runs/${encodeURIComponent(runId)}/artifacts`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }
}

class WorkflowPlans {
  constructor(private req: TandemClient["_request"]) {}

  private readonly chatTimeoutMs = 90_000;

  async preview(options: {
    prompt: string;
    schedule?: JsonObject;
    planSource?: string;
    plan_source?: string;
    allowedMcpServers?: string[];
    allowed_mcp_servers?: string[];
    workspaceRoot?: string;
    workspace_root?: string;
    operatorPreferences?: JsonObject;
    operator_preferences?: JsonObject;
  }): Promise<WorkflowPlanPreviewResponse> {
    return this.req<WorkflowPlanPreviewResponse>("/workflow-plans/preview", {
      method: "POST",
      body: JSON.stringify({
        prompt: options.prompt,
        schedule: options.schedule,
        plan_source: options.plan_source ?? options.planSource,
        allowed_mcp_servers: options.allowed_mcp_servers ?? options.allowedMcpServers,
        workspace_root: options.workspace_root ?? options.workspaceRoot,
        operator_preferences: options.operator_preferences ?? options.operatorPreferences,
      }),
    });
  }

  async apply(options: {
    planId?: string;
    plan_id?: string;
    plan?: WorkflowPlan;
    creatorId?: string;
    creator_id?: string;
    packBuilderExport?: WorkflowPlanPackBuilderExportRequest;
    pack_builder_export?: WorkflowPlanPackBuilderExportRequest;
    overlapDecision?: string;
    overlap_decision?: string;
  }): Promise<WorkflowPlanApplyResponse> {
    return this.req<WorkflowPlanApplyResponse>("/workflow-plans/apply", {
      method: "POST",
      body: JSON.stringify({
        plan_id: options.plan_id ?? options.planId,
        plan: options.plan,
        creator_id: options.creator_id ?? options.creatorId,
        pack_builder_export: options.pack_builder_export ?? options.packBuilderExport,
        overlap_decision: options.overlap_decision ?? options.overlapDecision,
      }),
    });
  }

  async chatStart(options: {
    prompt: string;
    schedule?: JsonObject;
    planSource?: string;
    plan_source?: string;
    allowedMcpServers?: string[];
    allowed_mcp_servers?: string[];
    workspaceRoot?: string;
    workspace_root?: string;
    operatorPreferences?: JsonObject;
    operator_preferences?: JsonObject;
  }): Promise<WorkflowPlanChatResponse> {
    return this.req<WorkflowPlanChatResponse>("/workflow-plans/chat/start", {
      method: "POST",
      timeoutMs: this.chatTimeoutMs,
      body: JSON.stringify({
        prompt: options.prompt,
        schedule: options.schedule,
        plan_source: options.plan_source ?? options.planSource,
        allowed_mcp_servers: options.allowed_mcp_servers ?? options.allowedMcpServers,
        workspace_root: options.workspace_root ?? options.workspaceRoot,
        operator_preferences: options.operator_preferences ?? options.operatorPreferences,
      }),
    });
  }

  async get(planId: string): Promise<WorkflowPlanGetResponse> {
    return this.req<WorkflowPlanGetResponse>(`/workflow-plans/${encodeURIComponent(planId)}`);
  }

  async chatMessage(options: {
    planId?: string;
    plan_id?: string;
    message: string;
  }): Promise<WorkflowPlanChatResponse> {
    return this.req<WorkflowPlanChatResponse>("/workflow-plans/chat/message", {
      method: "POST",
      timeoutMs: this.chatTimeoutMs,
      body: JSON.stringify({
        plan_id: options.plan_id ?? options.planId,
        message: options.message,
      }),
    });
  }

  async chatReset(options: {
    planId?: string;
    plan_id?: string;
  }): Promise<WorkflowPlanChatResponse> {
    return this.req<WorkflowPlanChatResponse>("/workflow-plans/chat/reset", {
      method: "POST",
      body: JSON.stringify({
        plan_id: options.plan_id ?? options.planId,
      }),
    });
  }

  async importPreview(options: {
    bundle: JsonObject;
    creatorId?: string;
    creator_id?: string;
    projectSlug?: string;
    project_slug?: string;
    title?: string;
  }): Promise<WorkflowPlanImportPreviewResponse> {
    return this.req<WorkflowPlanImportPreviewResponse>("/workflow-plans/import/preview", {
      method: "POST",
      body: JSON.stringify({
        bundle: options.bundle,
        creator_id: options.creator_id ?? options.creatorId,
        project_slug: options.project_slug ?? options.projectSlug,
        title: options.title,
      }),
    });
  }

  async importPlan(options: {
    bundle: JsonObject;
    creatorId?: string;
    creator_id?: string;
    projectSlug?: string;
    project_slug?: string;
    title?: string;
  }): Promise<WorkflowPlanImportResponse> {
    return this.req<WorkflowPlanImportResponse>("/workflow-plans/import", {
      method: "POST",
      body: JSON.stringify({
        bundle: options.bundle,
        creator_id: options.creator_id ?? options.creatorId,
        project_slug: options.project_slug ?? options.projectSlug,
        title: options.title,
      }),
    });
  }
}

class WorkflowPlannerSessions {
  constructor(private req: TandemClient["_request"]) {}

  private readonly chatTimeoutMs = 90_000;

  async list(options?: {
    projectSlug?: string;
    project_slug?: string;
  }): Promise<WorkflowPlannerSessionListResponse> {
    const params = new URLSearchParams();
    const projectSlug = options?.project_slug ?? options?.projectSlug;
    if (projectSlug) params.set("project_slug", projectSlug);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return this.req<WorkflowPlannerSessionListResponse>(`/workflow-plans/sessions${qs}`);
  }

  async create(options: {
    projectSlug: string;
    project_slug?: string;
    title?: string;
    workspaceRoot?: string;
    workspace_root?: string;
    goal?: string;
    notes?: string;
    plannerProvider?: string;
    planner_provider?: string;
    plannerModel?: string;
    planner_model?: string;
    planSource?: string;
    plan_source?: string;
    allowedMcpServers?: string[];
    allowed_mcp_servers?: string[];
    operatorPreferences?: JsonObject;
    operator_preferences?: JsonObject;
  }): Promise<WorkflowPlannerSessionCreateResponse> {
    return this.req<WorkflowPlannerSessionCreateResponse>("/workflow-plans/sessions", {
      method: "POST",
      body: JSON.stringify({
        project_slug: options.project_slug ?? options.projectSlug,
        title: options.title,
        workspace_root: options.workspace_root ?? options.workspaceRoot,
        goal: options.goal,
        notes: options.notes,
        planner_provider: options.planner_provider ?? options.plannerProvider,
        planner_model: options.planner_model ?? options.plannerModel,
        plan_source: options.plan_source ?? options.planSource,
        allowed_mcp_servers: options.allowed_mcp_servers ?? options.allowedMcpServers,
        operator_preferences: options.operator_preferences ?? options.operatorPreferences,
      }),
    });
  }

  async get(sessionId: string): Promise<WorkflowPlannerSessionResponse> {
    return this.req<WorkflowPlannerSessionResponse>(
      `/workflow-plans/sessions/${encodeURIComponent(sessionId)}`
    );
  }

  async patch(
    sessionId: string,
    patch: {
      title?: string;
      workspaceRoot?: string;
      workspace_root?: string;
      goal?: string;
      notes?: string;
      plannerProvider?: string;
      planner_provider?: string;
      plannerModel?: string;
      planner_model?: string;
      planSource?: string;
      plan_source?: string;
      allowedMcpServers?: string[];
      allowed_mcp_servers?: string[];
      operatorPreferences?: JsonObject;
      operator_preferences?: JsonObject;
      currentPlanId?: string;
      current_plan_id?: string;
      draft?: WorkflowPlanDraftRecord;
      publishedAtMs?: number;
      published_at_ms?: number;
      publishedTasks?: JsonValue[];
      published_tasks?: JsonValue[];
    }
  ): Promise<WorkflowPlannerSessionPatchResponse> {
    return this.req<WorkflowPlannerSessionPatchResponse>(
      `/workflow-plans/sessions/${encodeURIComponent(sessionId)}`,
      {
        method: "PATCH",
        body: JSON.stringify({
          title: patch.title,
          workspace_root: patch.workspace_root ?? patch.workspaceRoot,
          goal: patch.goal,
          notes: patch.notes,
          planner_provider: patch.planner_provider ?? patch.plannerProvider,
          planner_model: patch.planner_model ?? patch.plannerModel,
          plan_source: patch.plan_source ?? patch.planSource,
          allowed_mcp_servers: patch.allowed_mcp_servers ?? patch.allowedMcpServers,
          operator_preferences: patch.operator_preferences ?? patch.operatorPreferences,
          current_plan_id: patch.current_plan_id ?? patch.currentPlanId,
          draft: patch.draft,
          published_at_ms: patch.published_at_ms ?? patch.publishedAtMs,
          published_tasks: patch.published_tasks ?? patch.publishedTasks,
        }),
      }
    );
  }

  async duplicate(
    sessionId: string,
    options?: { title?: string }
  ): Promise<WorkflowPlannerSessionDuplicateResponse> {
    return this.req<WorkflowPlannerSessionDuplicateResponse>(
      `/workflow-plans/sessions/${encodeURIComponent(sessionId)}/duplicate`,
      {
        method: "POST",
        body: JSON.stringify({ title: options?.title }),
      }
    );
  }

  async delete(
    sessionId: string
  ): Promise<{ ok: boolean; session?: WorkflowPlannerSessionRecord }> {
    return this.req<{ ok: boolean; session?: WorkflowPlannerSessionRecord }>(
      `/workflow-plans/sessions/${encodeURIComponent(sessionId)}`,
      { method: "DELETE" }
    );
  }

  async start(
    sessionId: string,
    options: {
      prompt: string;
      schedule?: JsonObject;
      planSource?: string;
      plan_source?: string;
      allowedMcpServers?: string[];
      allowed_mcp_servers?: string[];
      workspaceRoot?: string;
      workspace_root?: string;
      operatorPreferences?: JsonObject;
      operator_preferences?: JsonObject;
    }
  ): Promise<WorkflowPlannerSessionStartResponse> {
    return this.req<WorkflowPlannerSessionStartResponse>(
      `/workflow-plans/sessions/${encodeURIComponent(sessionId)}/start`,
      {
        method: "POST",
        timeoutMs: this.chatTimeoutMs,
        body: JSON.stringify({
          prompt: options.prompt,
          schedule: options.schedule,
          plan_source: options.plan_source ?? options.planSource,
          allowed_mcp_servers: options.allowed_mcp_servers ?? options.allowedMcpServers,
          workspace_root: options.workspace_root ?? options.workspaceRoot,
          operator_preferences: options.operator_preferences ?? options.operatorPreferences,
        }),
      }
    );
  }

  async message(
    sessionId: string,
    options: { message: string }
  ): Promise<WorkflowPlannerSessionMessageResponse> {
    return this.req<WorkflowPlannerSessionMessageResponse>(
      `/workflow-plans/sessions/${encodeURIComponent(sessionId)}/message`,
      {
        method: "POST",
        timeoutMs: this.chatTimeoutMs,
        body: JSON.stringify({ message: options.message }),
      }
    );
  }

  async reset(sessionId: string): Promise<WorkflowPlannerSessionResetResponse> {
    return this.req<WorkflowPlannerSessionResetResponse>(
      `/workflow-plans/sessions/${encodeURIComponent(sessionId)}/reset`,
      {
        method: "POST",
      }
    );
  }
}

// ─── Automations namespace ────────────────────────────────────────────────────

class Automations {
  constructor(
    private baseUrl: string,
    private getToken: () => string,
    private req: TandemClient["_request"]
  ) {}

  /** List all automations. */
  async list(): Promise<DefinitionListResponse> {
    return this.req<DefinitionListResponse>("/automations");
  }

  /**
   * Create a mission-scoped automation.
   *
   * @example
   * ```typescript
   * await client.automations.create({
   *   name: "Weekly security scan",
   *   schedule: "0 9 * * 1",  // every Monday 9am
   *   mission: {
   *     objective: "Run a security audit of the API surface",
   *     success_criteria: ["No critical vulnerabilities", "Report written to reports/security.md"],
   *   },
   *   policy: {
   *     tool: { external_integrations_allowed: false },
   *     approval: { requires_approval: true },
   *   },
   * });
   * ```
   */
  async create(options: CreateAutomationOptions): Promise<DefinitionCreateResponse> {
    return this.req<DefinitionCreateResponse>("/automations", {
      method: "POST",
      body: JSON.stringify(options),
    });
  }

  /** Update an automation (partial patch). */
  async update(id: string, patch: PatchAutomationOptions): Promise<JsonObject> {
    return this.req<JsonObject>(`/automations/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(patch),
    });
  }

  /** Delete an automation. */
  async delete(id: string): Promise<void> {
    await this.req<void>(`/automations/${encodeURIComponent(id)}`, { method: "DELETE" });
  }

  /** Trigger an automation immediately. */
  async runNow(id: string): Promise<RunNowResponse> {
    const raw = await this.req<unknown>(`/automations/${encodeURIComponent(id)}/run_now`, {
      method: "POST",
      body: JSON.stringify({}),
    });
    return parseResponse(RunNowResponseSchema, raw, `/automations/${id}/run_now`, 200);
  }

  /** List recent runs across all automations. */
  async listRuns(options?: { automation_id?: string; limit?: number }): Promise<RunsListResponse> {
    const params = new URLSearchParams();
    if (options?.automation_id) params.set("automation_id", options.automation_id);
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const qs = params.toString() ? `?${params.toString()}` : "";
    return this.req<RunsListResponse>(`/automations/runs${qs}`);
  }

  /** List runs for a specific automation. */
  async getRunsForAutomation(id: string, limit = 25): Promise<RunsListResponse> {
    return this.req<RunsListResponse>(`/automations/${encodeURIComponent(id)}/runs?limit=${limit}`);
  }

  /** Get a specific automation run record. */
  async getRun(runId: string): Promise<RunRecord> {
    const raw = await this.req<unknown>(`/automations/runs/${encodeURIComponent(runId)}`);
    return parseResponse(RunRecordSchema, raw, `/automations/runs/${runId}`, 200);
  }

  /** List artifacts from an automation run. */
  async listArtifacts(runId: string): Promise<RunArtifactsResponse> {
    return this.req<RunArtifactsResponse>(
      `/automations/runs/${encodeURIComponent(runId)}/artifacts`
    );
  }

  /** Approve an automation run pending human review. */
  async approveRun(runId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/automations/runs/${encodeURIComponent(runId)}/approve`, {
      method: "POST",
      body: JSON.stringify({ reason: reason ?? "" }),
    });
  }

  /** Deny an automation run pending human review. */
  async denyRun(runId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/automations/runs/${encodeURIComponent(runId)}/deny`, {
      method: "POST",
      body: JSON.stringify({ reason: reason ?? "" }),
    });
  }

  /** Pause an active automation run. */
  async pauseRun(runId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/automations/runs/${encodeURIComponent(runId)}/pause`, {
      method: "POST",
      body: JSON.stringify({ reason: reason ?? "" }),
    });
  }

  /** Resume a paused automation run. */
  async resumeRun(runId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(`/automations/runs/${encodeURIComponent(runId)}/resume`, {
      method: "POST",
      body: JSON.stringify({ reason: reason ?? "" }),
    });
  }

  /** Get execution history for an automation. */
  async history(id: string, limit?: number): Promise<RoutineHistoryResponse> {
    const qs = limit !== undefined ? `?limit=${limit}` : "";
    const raw = await this.req<unknown>(`/automations/${encodeURIComponent(id)}/history${qs}`);
    if (Array.isArray(raw)) return { history: raw as JsonObject[], count: raw.length };
    return raw as RoutineHistoryResponse;
  }

  events(options?: {
    automationId?: string;
    automation_id?: string;
    runId?: string;
    run_id?: string;
    signal?: AbortSignal;
  }) {
    const params = new URLSearchParams();
    const automationId = options?.automation_id ?? options?.automationId;
    const runId = options?.run_id ?? options?.runId;
    if (automationId) params.set("automation_id", automationId);
    if (runId) params.set("run_id", runId);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return streamSse(`${this.baseUrl}/automations/events${qs}`, this.getToken(), {
      signal: options?.signal,
    });
  }

  addArtifact(runId: string, payload: JsonObject): Promise<JsonObject> {
    return this.req<JsonObject>(`/automations/runs/${encodeURIComponent(runId)}/artifacts`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }
}

// ─── AutomationsV2 namespace ─────────────────────────────────────────────────

class AutomationsV2 {
  constructor(
    private baseUrl: string,
    private getToken: () => string,
    private req: TandemClient["_request"]
  ) {}

  async create(spec: AutomationV2Spec): Promise<{ automation: JsonObject }> {
    return this.req<{ automation: JsonObject }>("/automations/v2", {
      method: "POST",
      body: JSON.stringify(spec),
    });
  }

  async list(): Promise<{ automations: JsonObject[]; count: number }> {
    return this.req<{ automations: JsonObject[]; count: number }>("/automations/v2");
  }

  async get(id: string): Promise<{ automation: JsonObject }> {
    return this.req<{ automation: JsonObject }>(`/automations/v2/${encodeURIComponent(id)}`);
  }

  async update(id: string, patch: Partial<AutomationV2Spec>): Promise<{ automation: JsonObject }> {
    return this.req<{ automation: JsonObject }>(`/automations/v2/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(patch),
    });
  }

  async delete(id: string): Promise<{ ok?: boolean; deleted?: boolean }> {
    return this.req<{ ok?: boolean; deleted?: boolean }>(
      `/automations/v2/${encodeURIComponent(id)}`,
      { method: "DELETE" }
    );
  }

  async runNow(
    id: string,
    options?: { dryRun?: boolean }
  ): Promise<{ ok?: boolean; dry_run?: boolean; dryRun?: boolean; run?: AutomationV2RunRecord }> {
    return this.req<{
      ok?: boolean;
      dry_run?: boolean;
      dryRun?: boolean;
      run?: AutomationV2RunRecord;
    }>(`/automations/v2/${encodeURIComponent(id)}/run_now`, {
      method: "POST",
      body: JSON.stringify({
        dry_run: options?.dryRun === true,
      }),
    });
  }

  async pause(id: string, reason?: string): Promise<{ ok?: boolean; automation?: JsonObject }> {
    return this.req<{ ok?: boolean; automation?: JsonObject }>(
      `/automations/v2/${encodeURIComponent(id)}/pause`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async resume(id: string): Promise<{ ok?: boolean; automation?: JsonObject }> {
    return this.req<{ ok?: boolean; automation?: JsonObject }>(
      `/automations/v2/${encodeURIComponent(id)}/resume`,
      { method: "POST", body: JSON.stringify({}) }
    );
  }

  async listRuns(
    id: string,
    limit = 50
  ): Promise<{ runs: AutomationV2RunRecord[]; count: number }> {
    return this.req<{ runs: AutomationV2RunRecord[]; count: number }>(
      `/automations/v2/${encodeURIComponent(id)}/runs?limit=${limit}`
    );
  }

  async getRun(runId: string): Promise<{ run: AutomationV2RunRecord }> {
    return this.req<{ run: AutomationV2RunRecord }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}`
    );
  }

  async pauseRun(
    runId: string,
    reason?: string
  ): Promise<{ ok?: boolean; run?: AutomationV2RunRecord }> {
    return this.req<{ ok?: boolean; run?: AutomationV2RunRecord }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/pause`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async resumeRun(
    runId: string,
    reason?: string
  ): Promise<{ ok?: boolean; run?: AutomationV2RunRecord }> {
    return this.req<{ ok?: boolean; run?: AutomationV2RunRecord }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/resume`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async recoverRun(
    runId: string,
    reason?: string
  ): Promise<{ ok?: boolean; run?: AutomationV2RunRecord }> {
    return this.req<{ ok?: boolean; run?: AutomationV2RunRecord }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/recover`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async repairRun(
    runId: string,
    input: {
      node_id: string;
      prompt?: string;
      template_id?: string;
      model_policy?: JsonObject;
      reason?: string;
    }
  ): Promise<{ ok?: boolean; run?: AutomationV2RunRecord; automation?: JsonObject }> {
    return this.req<{ ok?: boolean; run?: AutomationV2RunRecord; automation?: JsonObject }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/repair`,
      { method: "POST", body: JSON.stringify(input) }
    );
  }

  async retryTask(
    runId: string,
    nodeId: string,
    reason?: string
  ): Promise<{
    ok?: boolean;
    run?: AutomationV2RunRecord;
    node_id?: string;
    reset_nodes?: string[];
    cleared_outputs?: string[];
  }> {
    return this.req<{
      ok?: boolean;
      run?: AutomationV2RunRecord;
      node_id?: string;
      reset_nodes?: string[];
      cleared_outputs?: string[];
    }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/tasks/${encodeURIComponent(nodeId)}/retry`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async previewTaskReset(
    runId: string,
    nodeId: string
  ): Promise<{
    ok?: boolean;
    preview?: {
      run_id?: string;
      node_id?: string;
      reset_nodes?: string[];
      cleared_outputs?: string[];
      preserves_upstream_outputs?: boolean;
    };
  }> {
    return this.req<{
      ok?: boolean;
      preview?: {
        run_id?: string;
        node_id?: string;
        reset_nodes?: string[];
        cleared_outputs?: string[];
        preserves_upstream_outputs?: boolean;
      };
    }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/tasks/${encodeURIComponent(nodeId)}/reset_preview`
    );
  }

  async continueTask(
    runId: string,
    nodeId: string,
    reason?: string
  ): Promise<{
    ok?: boolean;
    run?: AutomationV2RunRecord;
    node_id?: string;
    reset_nodes?: string[];
  }> {
    return this.req<{
      ok?: boolean;
      run?: AutomationV2RunRecord;
      node_id?: string;
      reset_nodes?: string[];
    }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/tasks/${encodeURIComponent(nodeId)}/continue`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async requeueTask(
    runId: string,
    nodeId: string,
    reason?: string
  ): Promise<{
    ok?: boolean;
    run?: AutomationV2RunRecord;
    node_id?: string;
    reset_nodes?: string[];
    cleared_outputs?: string[];
  }> {
    return this.req<{
      ok?: boolean;
      run?: AutomationV2RunRecord;
      node_id?: string;
      reset_nodes?: string[];
      cleared_outputs?: string[];
    }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/tasks/${encodeURIComponent(nodeId)}/requeue`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async claimBacklogTask(
    runId: string,
    taskId: string,
    input?: { reason?: string; agent_id?: string; lease_ms?: number }
  ): Promise<{
    ok?: boolean;
    task?: JsonObject;
    agent_id?: string;
    reason?: string;
    blackboard?: JsonObject;
  }> {
    return this.req<{
      ok?: boolean;
      task?: JsonObject;
      agent_id?: string;
      reason?: string;
      blackboard?: JsonObject;
    }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/backlog/tasks/${encodeURIComponent(taskId)}/claim`,
      { method: "POST", body: JSON.stringify(input || {}) }
    );
  }

  async requeueBacklogTask(
    runId: string,
    taskId: string,
    reason?: string
  ): Promise<{ ok?: boolean; task?: JsonObject; reason?: string; blackboard?: JsonObject }> {
    return this.req<{
      ok?: boolean;
      task?: JsonObject;
      reason?: string;
      blackboard?: JsonObject;
    }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/backlog/tasks/${encodeURIComponent(taskId)}/requeue`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async cancelRun(
    runId: string,
    reason?: string
  ): Promise<{ ok?: boolean; run?: AutomationV2RunRecord }> {
    return this.req<{ ok?: boolean; run?: AutomationV2RunRecord }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/cancel`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  async gateDecide(
    runId: string,
    input: { decision: "approve" | "deny"; reason?: string }
  ): Promise<{ ok?: boolean; run?: AutomationV2RunRecord }> {
    return this.req<{ ok?: boolean; run?: AutomationV2RunRecord }>(
      `/automations/v2/runs/${encodeURIComponent(runId)}/gate`,
      { method: "POST", body: JSON.stringify(input) }
    );
  }

  /** List handoff artifacts (inbox / approved / archived) for a workflow automation. */
  async listHandoffs(id: string): Promise<{
    automation_id: string;
    workspace_root: string;
    handoff_config: {
      inbox_dir: string;
      approved_dir: string;
      archived_dir: string;
      auto_approve: boolean;
    };
    inbox: JsonObject[];
    approved: JsonObject[];
    archived: JsonObject[];
    counts: { inbox: number; approved: number; archived: number; total: number };
  }> {
    return this.req<{
      automation_id: string;
      workspace_root: string;
      handoff_config: {
        inbox_dir: string;
        approved_dir: string;
        archived_dir: string;
        auto_approve: boolean;
      };
      inbox: JsonObject[];
      approved: JsonObject[];
      archived: JsonObject[];
      counts: { inbox: number; approved: number; archived: number; total: number };
    }>(`/automations/v2/${encodeURIComponent(id)}/handoffs`);
  }

  events(options?: {
    automationId?: string;
    automation_id?: string;
    runId?: string;
    run_id?: string;
    signal?: AbortSignal;
  }) {
    const params = new URLSearchParams();
    const automationId = options?.automation_id ?? options?.automationId;
    const runId = options?.run_id ?? options?.runId;
    if (automationId) params.set("automation_id", automationId);
    if (runId) params.set("run_id", runId);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return streamSse(`${this.baseUrl}/automations/v2/events${qs}`, this.getToken(), {
      signal: options?.signal,
    });
  }
}

// ─── Optimizations namespace ────────────────────────────────────────────────

class Optimizations {
  constructor(private req: TandemClient["_request"]) {}

  async list(): Promise<{ optimizations: JsonObject[]; count: number }> {
    return this.req<{ optimizations: JsonObject[]; count: number }>("/optimizations");
  }

  async create(input: {
    optimization_id?: string;
    name?: string;
    source_workflow_id: string;
    artifacts: {
      objective_ref: string;
      eval_ref: string;
      mutation_policy_ref: string;
      scope_ref: string;
      budget_ref: string;
      research_log_ref?: string | null;
      summary_ref?: string | null;
    };
    metadata?: JsonObject | null;
  }): Promise<{ optimization: JsonObject; experimentCount?: number }> {
    return this.req<{ optimization: JsonObject; experimentCount?: number }>("/optimizations", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }

  async get(id: string): Promise<{ optimization: JsonObject; experimentCount?: number }> {
    return this.req<{ optimization: JsonObject; experimentCount?: number }>(
      `/optimizations/${encodeURIComponent(id)}`
    );
  }

  async action(
    id: string,
    input: {
      action: string;
      experiment_id?: string;
      run_id?: string;
      reason?: string;
    }
  ): Promise<{ optimization: JsonObject; experimentCount?: number }> {
    return this.req<{ optimization: JsonObject; experimentCount?: number }>(
      `/optimizations/${encodeURIComponent(id)}/actions`,
      {
        method: "POST",
        body: JSON.stringify(input),
      }
    );
  }

  async listExperiments(
    id: string
  ): Promise<{ optimization: JsonObject; experiments: JsonObject[]; count: number }> {
    return this.req<{ optimization: JsonObject; experiments: JsonObject[]; count: number }>(
      `/optimizations/${encodeURIComponent(id)}/experiments`
    );
  }

  async getExperiment(
    id: string,
    experimentId: string
  ): Promise<{ optimization: JsonObject; experiment: JsonObject }> {
    return this.req<{ optimization: JsonObject; experiment: JsonObject }>(
      `/optimizations/${encodeURIComponent(id)}/experiments/${encodeURIComponent(experimentId)}`
    );
  }

  async applyWinner(
    id: string,
    experimentId: string
  ): Promise<{ optimization: JsonObject; experiment: JsonObject; automation: JsonObject }> {
    return this.req<{ optimization: JsonObject; experiment: JsonObject; automation: JsonObject }>(
      `/optimizations/${encodeURIComponent(id)}/experiments/${encodeURIComponent(experimentId)}`,
      { method: "POST", body: JSON.stringify({}) }
    );
  }
}

// ─── AgentTeams namespace ─────────────────────────────────────────────────────

class AgentTeams {
  constructor(private req: TandemClient["_request"]) {}

  /** List available agent team templates. */
  async listTemplates(options?: { workspaceRoot?: string }): Promise<AgentTeamTemplatesResponse> {
    const params = new URLSearchParams();
    const workspaceRoot = String(options?.workspaceRoot || "").trim();
    if (workspaceRoot) params.set("workspaceRoot", workspaceRoot);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return this.req<AgentTeamTemplatesResponse>(`/agent-team/templates${qs}`);
  }

  async createTemplate(
    input: AgentTeamTemplateCreateInput,
    options?: { workspaceRoot?: string }
  ): Promise<{ ok?: boolean; template?: JsonObject }> {
    const params = new URLSearchParams();
    const workspaceRoot = String(options?.workspaceRoot || "").trim();
    if (workspaceRoot) params.set("workspaceRoot", workspaceRoot);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return this.req<{ ok?: boolean; template?: JsonObject }>(`/agent-team/templates${qs}`, {
      method: "POST",
      body: JSON.stringify(input),
    });
  }

  async updateTemplate(
    id: string,
    patch: AgentTeamTemplatePatchInput,
    options?: { workspaceRoot?: string }
  ): Promise<{ ok?: boolean; template?: JsonObject }> {
    const params = new URLSearchParams();
    const workspaceRoot = String(options?.workspaceRoot || "").trim();
    if (workspaceRoot) params.set("workspaceRoot", workspaceRoot);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return this.req<{ ok?: boolean; template?: JsonObject }>(
      `/agent-team/templates/${encodeURIComponent(id)}${qs}`,
      { method: "PATCH", body: JSON.stringify(patch) }
    );
  }

  async deleteTemplate(
    id: string,
    options?: { workspaceRoot?: string }
  ): Promise<{ ok?: boolean; deleted?: boolean }> {
    const params = new URLSearchParams();
    const workspaceRoot = String(options?.workspaceRoot || "").trim();
    if (workspaceRoot) params.set("workspaceRoot", workspaceRoot);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return this.req<{ ok?: boolean; deleted?: boolean }>(
      `/agent-team/templates/${encodeURIComponent(id)}${qs}`,
      { method: "DELETE" }
    );
  }

  /** Compose an Agent Standup automation spec from selected templates. */
  async composeStandup(input: AgentStandupComposeInput): Promise<AgentStandupComposeResponse> {
    return this.req<AgentStandupComposeResponse>("/agent-standup/compose", {
      method: "POST",
      body: JSON.stringify({
        name: input.name,
        workspace_root: input.workspaceRoot,
        schedule: input.schedule,
        participant_template_ids: input.participantTemplateIds,
        report_path_template: input.reportPathTemplate,
        model_policy: input.modelPolicy,
      }),
    });
  }

  /** List agent team instances. */
  async listInstances(options?: {
    missionID?: string;
    parentInstanceID?: string;
    status?: string;
  }): Promise<AgentTeamInstancesResponse> {
    const params = new URLSearchParams();
    if (options?.missionID) params.set("missionID", options.missionID);
    if (options?.parentInstanceID) params.set("parentInstanceID", options.parentInstanceID);
    if (options?.status) params.set("status", options.status);
    const qs = params.toString() ? `?${params.toString()}` : "";
    return this.req<AgentTeamInstancesResponse>(`/agent-team/instances${qs}`);
  }

  /** List missions managed by the agent team orchestrator. */
  async listMissions(): Promise<AgentTeamMissionsResponse> {
    return this.req<AgentTeamMissionsResponse>("/agent-team/missions");
  }

  /** List pending spawn and tool approvals. */
  async listApprovals(): Promise<AgentTeamApprovalsResponse> {
    return this.req<AgentTeamApprovalsResponse>("/agent-team/approvals");
  }

  /**
   * Spawn a new agent team instance.
   *
   * @example
   * ```typescript
   * const result = await client.agentTeams.spawn({
   *   missionID: "mission-123",
   *   role: "builder",
   *   justification: "Implementing feature X as planned",
   * });
   * ```
   */
  async spawn(input: AgentTeamSpawnInput): Promise<AgentTeamSpawnResponse> {
    return this.req<AgentTeamSpawnResponse>("/agent-team/spawn", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }

  /** Approve a pending agent team spawn request. */
  async approveSpawn(approvalId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(
      `/agent-team/approvals/spawn/${encodeURIComponent(approvalId)}/approve`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }

  /** Deny a pending agent team spawn request. */
  async denySpawn(approvalId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.req<{ ok: boolean }>(
      `/agent-team/approvals/spawn/${encodeURIComponent(approvalId)}/deny`,
      { method: "POST", body: JSON.stringify({ reason: reason ?? "" }) }
    );
  }
}

// ─── Missions namespace ───────────────────────────────────────────────────────

class Missions {
  constructor(private req: TandemClient["_request"]) {}

  /** List all missions. */
  async list(): Promise<MissionListResponse> {
    return this.req<MissionListResponse>("/mission");
  }

  /**
   * Create a new mission with work items.
   *
   * @example
   * ```typescript
   * const { mission } = await client.missions.create({
   *   title: "Q1 Security Hardening",
   *   goal: "Audit and fix security issues in the API surface",
   *   work_items: [
   *     { title: "Audit auth middleware", assigned_agent: "security-auditor" },
   *     { title: "Review input validation", assigned_agent: "security-auditor" },
   *   ],
   * });
   * ```
   */
  async create(input: MissionCreateInput): Promise<MissionCreateResponse> {
    return this.req<MissionCreateResponse>("/mission", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }

  /** Get a mission by ID. */
  async get(missionId: string): Promise<MissionGetResponse> {
    return this.req<MissionGetResponse>(`/mission/${encodeURIComponent(missionId)}`);
  }

  /** Apply a state event to a mission. */
  async applyEvent(missionId: string, event: JsonObject): Promise<MissionEventResponse> {
    return this.req<MissionEventResponse>(`/mission/${encodeURIComponent(missionId)}/event`, {
      method: "POST",
      body: JSON.stringify({ event }),
    });
  }
}

// Re-export namespace types for instanceof checks / documentation
export type { RoutineFamily, RoutineRecord };
