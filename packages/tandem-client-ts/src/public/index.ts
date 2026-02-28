// ─── Core & Enums ─────────────────────────────────────────────────────────────

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };

export type RunStatus = "queued" | "running" | "succeeded" | "failed" | "canceled" | "unknown";
export type RoutineStatus = "enabled" | "disabled" | "paused" | "unknown";
export type ApprovalStatus = "pending" | "approved" | "rejected" | "unknown";
export type ChannelName = "telegram" | "discord" | "slack";

export interface TandemClientOptions {
  baseUrl: string;
  token: string;
  timeoutMs?: number;
}

// ─── Health ──────────────────────────────────────────────────────────────────

export interface SystemHealth {
  ready?: boolean;
  phase?: string;
  [key: string]: unknown;
}

// ─── Sessions ────────────────────────────────────────────────────────────────

export interface CreateSessionOptions {
  title?: string;
  directory?: string;
  permissions?: PermissionRule[];
  provider?: string;
  model?: string;
}

export interface UpdateSessionOptions {
  title?: string;
  archived?: boolean;
}

export interface SessionRecord {
  id: string;
  title: string;
  createdAtMs: number;
  directory?: string;
  workspaceRoot?: string;
  archived?: boolean;
  [key: string]: unknown;
}

export interface SessionListResponse {
  sessions: SessionRecord[];
  count: number;
}

export interface ListSessionsOptions {
  q?: string;
  page?: number;
  pageSize?: number;
  archived?: boolean;
  scope?: "workspace" | "global";
  workspace?: string;
}

export interface SessionRunStateResponse {
  active?: {
    runId?: string;
    attachEventStream?: string;
    [key: string]: unknown;
  } | null;
}

export interface PromptAsyncResult {
  runId: string;
}

export interface PromptModelOptions {
  provider: string;
  model: string;
}

export interface SessionDiff {
  diff?: string;
  files?: string[];
  [key: string]: unknown;
}

export interface SessionTodo {
  id?: string;
  content: string;
  status?: string;
  [key: string]: unknown;
}

// ─── Messages ────────────────────────────────────────────────────────────────

export interface MessagePart {
  type?: string;
  text?: string;
}

export interface EngineMessage {
  info?: { role?: string };
  parts?: MessagePart[];
}

// ─── Permissions ─────────────────────────────────────────────────────────────

export interface PermissionRule {
  permission: string;
  pattern: string;
  action: "allow" | "deny" | "ask";
}

export interface PermissionRequestRecord {
  id: string;
  permission?: string;
  pattern?: string;
  tool?: string;
  status?: ApprovalStatus | string;
  sessionId?: string;
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

export type PermissionReply = "allow" | "always" | "deny" | "reject" | "once";

// ─── Questions (AI approval gate) ────────────────────────────────────────────

export interface QuestionRecord {
  id: string;
  text?: string;
  choices?: string[];
  status?: ApprovalStatus | string;
  sessionId?: string;
  [key: string]: unknown;
}

export interface QuestionsListResponse {
  questions: QuestionRecord[];
  [key: string]: unknown;
}

// ─── Providers ───────────────────────────────────────────────────────────────

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
  defaultModel?: string;
}

export interface ProvidersConfigResponse {
  default?: string | null;
  providers: Record<string, ProviderConfigEntry>;
}

// ─── Channels ────────────────────────────────────────────────────────────────

export interface ChannelConfigEntry {
  hasToken?: boolean;
  allowedUsers?: string[];
  mentionOnly?: boolean;
  guildId?: string;
  channelId?: string;
}

export interface ChannelsConfigResponse {
  telegram: ChannelConfigEntry;
  discord: ChannelConfigEntry;
  slack: ChannelConfigEntry;
}

export interface ChannelStatusEntry {
  enabled: boolean;
  connected: boolean;
  lastError?: string | null;
  activeSessions: number;
  meta?: JsonObject;
}

export interface ChannelsStatusResponse {
  telegram: ChannelStatusEntry;
  discord: ChannelStatusEntry;
  slack: ChannelStatusEntry;
}

// ─── MCP ─────────────────────────────────────────────────────────────────────

export interface AddMcpServerOptions {
  name: string;
  transport: string;
  headers?: Record<string, string>;
  enabled?: boolean;
}

// ─── Memory ──────────────────────────────────────────────────────────────────

export interface MemoryItem {
  id?: string;
  text: string;
  tags?: string[];
  source?: string;
  sessionId?: string;
  runId?: string;
  [key: string]: unknown;
}

export interface MemoryPutOptions {
  text: string;
  tags?: string[];
  source?: string;
  sessionId?: string;
  runId?: string;
  capability?: string;
}

export interface MemoryPutResponse {
  id: string;
  ok: boolean;
  [key: string]: unknown;
}

export interface MemorySearchOptions {
  query: string;
  limit?: number;
  tags?: string[];
  sessionId?: string;
  capability?: string;
}

export interface MemorySearchResult {
  id: string;
  text: string;
  score?: number;
  tags?: string[];
  [key: string]: unknown;
}

export interface MemorySearchResponse {
  results: MemorySearchResult[];
  count: number;
}

export interface MemoryListResponse {
  items: MemoryItem[];
  count: number;
}

export interface MemoryPromoteOptions {
  id: string;
  capability?: string;
}

export interface MemoryPromoteResponse {
  ok: boolean;
  id: string;
  [key: string]: unknown;
}

export interface MemoryAuditEntry {
  id?: string;
  tsMs?: number;
  action?: string;
  runId?: string;
  [key: string]: unknown;
}

export interface MemoryAuditResponse {
  entries: MemoryAuditEntry[];
  count: number;
}

// ─── Skills ──────────────────────────────────────────────────────────────────

export type SkillLocation = "user" | "workspace" | "builtin";

export interface SkillRecord {
  name: string;
  location?: SkillLocation;
  description?: string;
  version?: string;
  [key: string]: unknown;
}

export interface SkillsListResponse {
  skills: SkillRecord[];
  count: number;
}

export interface SkillImportOptions {
  content?: string;
  fileOrPath?: string;
  location: SkillLocation;
  namespace?: string;
  conflictPolicy?: "skip" | "overwrite" | "error";
}

export interface SkillImportResponse {
  ok: boolean;
  imported?: number;
  [key: string]: unknown;
}

export interface SkillTemplate {
  name: string;
  description?: string;
  [key: string]: unknown;
}

export interface SkillTemplatesResponse {
  templates: SkillTemplate[];
  count: number;
}

// ─── Resources ───────────────────────────────────────────────────────────────

export interface ResourceRecord {
  key: string;
  value: unknown;
  rev?: number;
  updatedAtMs?: number;
  updatedBy?: string;
  [key: string]: unknown;
}

export interface ResourceListResponse {
  items: ResourceRecord[];
  count: number;
}

export interface ResourceWriteOptions {
  key: string;
  value: unknown;
  ifMatchRev?: number;
  updatedBy?: string;
  ttlMs?: number;
}

export interface ResourceWriteResponse {
  ok: boolean;
  rev?: number;
  [key: string]: unknown;
}

// ─── Routines & Automations ──────────────────────────────────────────────────

export type RoutineFamily = "routines" | "automations";

export type RoutineSchedule =
  | { type: "cron"; cron: string }
  | { type: "interval"; intervalMs: number }
  | { type: "manual" }
  | string;

export interface RoutineRecord {
  id: string;
  name?: string;
  schedule?: RoutineSchedule;
  entrypoint?: string;
  prompt?: string;
  status?: RoutineStatus | string;
  lastRun?: string;
  lastRunAt?: string;
  requiresApproval?: boolean;
  externalIntegrationsAllowed?: boolean;
  [key: string]: unknown;
}

export interface DefinitionListResponse {
  routines?: RoutineRecord[];
  automations?: RoutineRecord[];
  count: number;
}

export interface DefinitionCreateResponse {
  routine?: RoutineRecord;
  automation?: RoutineRecord;
}

export interface CreateRoutineOptions {
  name: string;
  schedule?: RoutineSchedule;
  timezone?: string;
  misfirePolicy?: "skip" | "run_late" | "run_now";
  entrypoint?: string;
  args?: JsonObject;
  allowedTools?: string[];
  outputTargets?: string[];
  requiresApproval?: boolean;
  externalIntegrationsAllowed?: boolean;
  nextFireAtMs?: number;
  [key: string]: unknown;
}

export interface PatchRoutineOptions {
  name?: string;
  status?: RoutineStatus | string;
  schedule?: RoutineSchedule;
  timezone?: string;
  misfirePolicy?: string;
  entrypoint?: string;
  args?: JsonObject;
  allowedTools?: string[];
  outputTargets?: string[];
  requiresApproval?: boolean;
  externalIntegrationsAllowed?: boolean;
  nextFireAtMs?: number;
}

export interface AutomationMissionOptions {
  objective: string;
  successCriteria?: string[];
  briefing?: string;
}

export interface CreateAutomationOptions {
  name: string;
  schedule: RoutineSchedule;
  timezone?: string;
  misfirePolicy?: string;
  mission: AutomationMissionOptions;
  mode?: string;
  policy?: {
    tool?: { runAllowlist?: string[]; externalIntegrationsAllowed?: boolean };
    approval?: { requiresApproval?: boolean };
  };
  outputTargets?: string[];
  modelPolicy?: JsonObject;
  nextFireAtMs?: number;
}

export interface PatchAutomationOptions {
  name?: string;
  status?: RoutineStatus | string;
  schedule?: RoutineSchedule;
  mission?: Partial<AutomationMissionOptions>;
  mode?: string;
  policy?: JsonObject;
  outputTargets?: string[];
  modelPolicy?: JsonObject;
  nextFireAtMs?: number;
}

export interface RunNowResponse {
  ok?: boolean;
  runId?: string;
  status?: RunStatus | string;
}

export interface RunsListResponse {
  runs: RunRecord[];
  count: number;
}

export interface RunRecord {
  id?: string;
  runId?: string;
  routineId?: string;
  automationId?: string;
  status?: RunStatus | string;
  startedAtMs?: number;
  finishedAtMs?: number;
  [key: string]: unknown;
}

export interface ArtifactRecord {
  artifactId?: string;
  uri: string;
  kind: string;
  label?: string;
  metadata?: JsonObject;
  createdAtMs?: number;
}

export interface RunArtifactsResponse {
  runId?: string;
  artifacts: ArtifactRecord[];
  count: number;
}

export interface RoutineHistoryEntry {
  event?: string;
  tsMs?: number;
  status?: RoutineStatus | string;
  [key: string]: unknown;
}

export interface RoutineHistoryResponse {
  history: RoutineHistoryEntry[];
  count: number;
}

// ─── Agent Teams ─────────────────────────────────────────────────────────────

export interface AgentTeamSpawnInput {
  missionId?: string;
  parentInstanceId?: string;
  templateId?: string;
  role: string;
  source?: string;
  justification: string;
  budgetOverride?: JsonObject;
}

export interface AgentTeamSpawnResponse {
  ok?: boolean;
  missionId?: string;
  instanceId?: string;
  sessionId?: string;
  runId?: string | null;
  status?: string;
  code?: string;
  error?: string;
}

export interface AgentTeamTemplate {
  id: string;
  name?: string;
  role?: string;
  [key: string]: unknown;
}

export interface AgentTeamTemplatesResponse {
  templates: AgentTeamTemplate[];
  count: number;
}

export interface AgentTeamInstance {
  instanceId?: string;
  missionId?: string;
  role?: string;
  status?: string;
  sessionId?: string;
  [key: string]: unknown;
}

export interface AgentTeamInstancesResponse {
  instances: AgentTeamInstance[];
  count: number;
}

export interface AgentTeamMissionsResponse {
  missions: JsonObject[];
  count: number;
}

export interface AgentTeamSpawnApproval {
  approvalId?: string;
  status?: ApprovalStatus | string;
  [key: string]: unknown;
}

export interface AgentTeamApprovalsResponse {
  spawnApprovals: AgentTeamSpawnApproval[];
  toolApprovals: JsonObject[];
  count: number;
}

// ─── Missions ────────────────────────────────────────────────────────────────

export interface MissionWorkItem {
  title: string;
  detail?: string;
  assignedAgent?: string;
}

export interface MissionCreateInput {
  title: string;
  goal: string;
  workItems: MissionWorkItem[];
}

export interface MissionRecord {
  id?: string;
  title?: string;
  goal?: string;
  status?: string;
  [key: string]: unknown;
}

export interface MissionCreateResponse {
  mission?: MissionRecord;
}

export interface MissionListResponse {
  missions: MissionRecord[];
  count: number;
}

export interface MissionGetResponse {
  mission: MissionRecord;
}

export interface MissionEventResponse {
  mission?: MissionRecord;
  commands?: unknown[];
  [key: string]: unknown;
}

// ─── Tools ───────────────────────────────────────────────────────────────────

export interface ToolSchema {
  name: string;
  description?: string;
  inputSchema?: JsonObject;
  [key: string]: unknown;
}

export interface ToolExecuteResult {
  output?: string;
  metadata?: JsonObject;
  [key: string]: unknown;
}

// ─── SSE Events ──────────────────────────────────────────────────────────────

export interface EngineEventBase {
  type: string;
  properties: Record<string, unknown>;
  sessionId?: string;
  runId?: string;
  timestamp?: string;
  [key: string]: unknown;
}

export interface RunStartedEvent extends EngineEventBase {
  type: "run.started";
}

export interface RunProgressEvent extends EngineEventBase {
  type: "run.progress";
}

export interface RunCompletedEvent extends EngineEventBase {
  type: "run.completed";
}

export interface RunFailedEvent extends EngineEventBase {
  type: "run.failed";
}

export interface ToolCalledEvent extends EngineEventBase {
  type: "tool.called";
}

export interface ToolResultEvent extends EngineEventBase {
  type: "tool.result";
}

export interface ApprovalRequestedEvent extends EngineEventBase {
  type: "approval.requested";
}

export interface ApprovalResolvedEvent extends EngineEventBase {
  type: "approval.resolved";
}

export interface RoutineTriggeredEvent extends EngineEventBase {
  type: "routine.triggered";
}

export interface RoutineCompletedEvent extends EngineEventBase {
  type: "routine.completed";
}

export interface SessionResponseEvent extends EngineEventBase {
  type: "session.response";
}

export interface UnknownEvent extends EngineEventBase {
  type: string;
}

export type KnownEventType =
  | "run.started"
  | "run.progress"
  | "run.completed"
  | "run.failed"
  | "tool.called"
  | "tool.result"
  | "approval.requested"
  | "approval.resolved"
  | "routine.triggered"
  | "routine.completed"
  | "session.response";

// Union of all possible typed events
export type EngineEvent =
  | RunStartedEvent
  | RunProgressEvent
  | RunCompletedEvent
  | RunFailedEvent
  | ToolCalledEvent
  | ToolResultEvent
  | ApprovalRequestedEvent
  | ApprovalResolvedEvent
  | RoutineTriggeredEvent
  | RoutineCompletedEvent
  | SessionResponseEvent
  | UnknownEvent;
