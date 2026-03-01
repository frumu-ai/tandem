// ─── Core ─────────────────────────────────────────────────────────────────────

export type JsonObject = Record<string, unknown>;

export interface TandemClientOptions {
  /** Base URL of the Tandem engine, e.g. http://localhost:39731 */
  baseUrl: string;
  /** Engine API token (from `tandem-engine token generate`) */
  token: string;
  /** Request timeout in ms (default 20000) */
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
  created_at_ms: number;
  directory?: string;
  workspace_root?: string;
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
    runID?: string;
    runId?: string;
    run_id?: string;
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

export type ToolMode = "auto" | "none" | "required";
export type ContextMode = "auto" | "compact" | "full";

export interface PromptRoutingOptions {
  toolMode?: ToolMode;
  toolAllowlist?: string[];
  contextMode?: ContextMode;
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
  status?: string;
  sessionID?: string;
  sessionId?: string;
  session_id?: string;
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
  status?: string;
  sessionID?: string;
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
  default_model?: string;
}

export interface ProvidersConfigResponse {
  default?: string | null;
  providers: Record<string, ProviderConfigEntry>;
}

export interface PersonalityProfile {
  preset?: string;
  custom_instructions?: string | null;
}

export interface PersonalityConfig {
  default?: PersonalityProfile;
  per_agent?: Record<string, PersonalityProfile>;
}

export interface BotIdentityAliases {
  desktop?: string;
  tui?: string;
  portal?: string;
  control_panel?: string;
  channels?: string;
  protocol?: string;
  cli?: string;
}

export interface BotIdentity {
  canonical_name?: string;
  avatar_url?: string | null;
  aliases?: BotIdentityAliases;
}

export interface IdentityConfig {
  bot?: BotIdentity;
  personality?: PersonalityConfig;
}

export interface PersonalityPresetEntry {
  id: string;
  label: string;
  description?: string;
}

export interface IdentityConfigResponse {
  identity: IdentityConfig;
  presets?: PersonalityPresetEntry[];
}

// ─── Channels ────────────────────────────────────────────────────────────────

export type ChannelName = "telegram" | "discord" | "slack";

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
  text?: string;
  content?: string;
  user_id?: string;
  source_type?: string;
  tags?: string[];
  source?: string;
  session_id?: string;
  run_id?: string;
  [key: string]: unknown;
}

export interface MemoryPutOptions {
  text: string;
  tags?: string[];
  source?: string;
  session_id?: string;
  run_id?: string;
  capability?: string;
}

export interface MemoryPutResponse {
  id: string;
  ok?: boolean;
  stored?: boolean;
  tier?: string;
  partition_key?: string;
  audit_id?: string;
  [key: string]: unknown;
}

export interface MemorySearchOptions {
  query: string;
  limit?: number;
  tags?: string[];
  session_id?: string;
  capability?: string;
}

export interface MemorySearchResult {
  id: string;
  text?: string;
  content?: string;
  score?: number;
  source_type?: string;
  run_id?: string;
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
  ok?: boolean;
  id?: string;
  promoted?: boolean;
  new_memory_id?: string;
  to_tier?: string;
  audit_id?: string;
  [key: string]: unknown;
}

export interface MemoryDemoteOptions {
  id: string;
  run_id?: string;
}

export interface MemoryDemoteResponse {
  ok: boolean;
  [key: string]: unknown;
}

export interface MemoryAuditEntry {
  id?: string;
  ts_ms?: number;
  action?: string;
  run_id?: string;
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
  file_or_path?: string;
  location: SkillLocation;
  namespace?: string;
  conflict_policy?: "skip" | "overwrite" | "error";
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

// ─── Resources (key-value store) ─────────────────────────────────────────────

export interface ResourceRecord {
  key: string;
  value: unknown;
  rev?: number;
  updated_at_ms?: number;
  updated_by?: string;
  [key: string]: unknown;
}

export interface ResourceListResponse {
  items: ResourceRecord[];
  count: number;
}

export interface ResourceWriteOptions {
  key: string;
  value: unknown;
  if_match_rev?: number;
  updated_by?: string;
  ttl_ms?: number;
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
  | { type: "interval"; interval_ms: number }
  | { type: "manual" }
  | string; // cron shorthand

export interface RoutineRecord {
  id: string;
  name?: string;
  schedule?: RoutineSchedule;
  entrypoint?: string;
  prompt?: string;
  status?: string;
  last_run?: string;
  last_run_at?: string;
  requires_approval?: boolean;
  external_integrations_allowed?: boolean;
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
  misfire_policy?: "skip" | "run_late" | "run_now";
  entrypoint?: string;
  args?: JsonObject;
  allowed_tools?: string[];
  output_targets?: string[];
  requires_approval?: boolean;
  external_integrations_allowed?: boolean;
  next_fire_at_ms?: number;
  [key: string]: unknown;
}

export interface PatchRoutineOptions {
  name?: string;
  status?: string;
  schedule?: RoutineSchedule;
  timezone?: string;
  misfire_policy?: string;
  entrypoint?: string;
  args?: JsonObject;
  allowed_tools?: string[];
  output_targets?: string[];
  requires_approval?: boolean;
  external_integrations_allowed?: boolean;
  next_fire_at_ms?: number;
}

export interface AutomationMissionOptions {
  objective: string;
  success_criteria?: string[];
  briefing?: string;
}

export interface CreateAutomationOptions {
  name: string;
  schedule: RoutineSchedule;
  timezone?: string;
  misfire_policy?: string;
  mission: AutomationMissionOptions;
  mode?: string;
  policy?: {
    tool?: { run_allowlist?: string[]; external_integrations_allowed?: boolean };
    approval?: { requires_approval?: boolean };
  };
  output_targets?: string[];
  model_policy?: JsonObject;
  next_fire_at_ms?: number;
}

export interface PatchAutomationOptions {
  name?: string;
  status?: string;
  schedule?: RoutineSchedule;
  mission?: Partial<AutomationMissionOptions>;
  mode?: string;
  policy?: JsonObject;
  output_targets?: string[];
  model_policy?: JsonObject;
  next_fire_at_ms?: number;
}

export interface RunNowResponse {
  ok?: boolean;
  runID?: string;
  runId?: string;
  run_id?: string;
  status?: string;
}

export interface RunsListResponse {
  runs: JsonObject[];
  count: number;
}

export interface RunRecord {
  id?: string;
  runID?: string;
  routine_id?: string;
  automation_id?: string;
  status?: string;
  started_at_ms?: number;
  finished_at_ms?: number;
  [key: string]: unknown;
}

export interface ArtifactRecord {
  artifact_id?: string;
  uri: string;
  kind: string;
  label?: string;
  metadata?: JsonObject;
  created_at_ms?: number;
}

export interface RunArtifactsResponse {
  runID?: string;
  artifacts: ArtifactRecord[];
  count: number;
}

export interface RoutineHistoryEntry {
  event?: string;
  ts_ms?: number;
  status?: string;
  [key: string]: unknown;
}

export interface RoutineHistoryResponse {
  history: RoutineHistoryEntry[];
  count: number;
}

// ─── Agent Teams ─────────────────────────────────────────────────────────────

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

export interface AgentTeamTemplateCreateInput {
  template: JsonObject;
}

export interface AgentTeamTemplatePatchInput {
  role?: string;
  system_prompt?: string;
  skills?: JsonObject[];
  default_budget?: JsonObject;
  capabilities?: JsonObject;
}

export interface AgentTeamInstance {
  instanceID?: string;
  missionID?: string;
  role?: string;
  status?: string;
  sessionID?: string;
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
  approvalID?: string;
  status?: string;
  [key: string]: unknown;
}

export interface AgentTeamApprovalsResponse {
  spawnApprovals: AgentTeamSpawnApproval[];
  toolApprovals: JsonObject[];
  count: number;
}

export type AutomationV2Status = "active" | "paused" | "draft";
export type AutomationV2ScheduleType = "cron" | "interval" | "manual";
export type AutomationV2RunStatus =
  | "queued"
  | "running"
  | "pausing"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

export interface AutomationV2Schedule {
  type: AutomationV2ScheduleType;
  cron_expression?: string;
  interval_seconds?: number;
  timezone: string;
  misfire_policy?: "skip" | "run_once" | "catch_up";
}

export interface AutomationV2AgentProfile {
  agent_id: string;
  template_id?: string;
  display_name: string;
  avatar_url?: string;
  model_policy?: JsonObject;
  skills?: string[];
  tool_policy?: { allowlist?: string[]; denylist?: string[] };
  mcp_policy?: { allowed_servers?: string[]; allowed_tools?: string[] };
  approval_policy?: string;
}

export interface AutomationV2FlowNode {
  node_id: string;
  agent_id: string;
  objective: string;
  depends_on?: string[];
  retry_policy?: JsonObject;
  timeout_ms?: number;
}

export interface AutomationV2Spec {
  automation_id?: string;
  name: string;
  description?: string;
  status?: AutomationV2Status;
  schedule: AutomationV2Schedule;
  agents: AutomationV2AgentProfile[];
  flow: { nodes: AutomationV2FlowNode[] };
  execution?: {
    max_parallel_agents?: number;
    max_total_runtime_ms?: number;
    max_total_tool_calls?: number;
  };
  output_targets?: string[];
  creator_id?: string;
  [key: string]: unknown;
}

export interface AutomationV2RunRecord {
  run_id: string;
  automation_id: string;
  status: AutomationV2RunStatus;
  checkpoint?: JsonObject;
  active_session_ids?: string[];
  active_instance_ids?: string[];
  [key: string]: unknown;
}

// ─── Missions ────────────────────────────────────────────────────────────────

export interface MissionWorkItem {
  title: string;
  detail?: string;
  assigned_agent?: string;
}

export interface MissionCreateInput {
  title: string;
  goal: string;
  work_items: MissionWorkItem[];
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
  input_schema?: JsonObject;
  [key: string]: unknown;
}

export interface ToolExecuteResult {
  output?: string;
  metadata?: JsonObject;
  [key: string]: unknown;
}

// ─── SSE events ──────────────────────────────────────────────────────────────

export interface EngineEvent {
  type: string;
  properties: Record<string, unknown>;
  sessionID?: string;
  runID?: string;
  timestamp?: string;
  [key: string]: unknown;
}
