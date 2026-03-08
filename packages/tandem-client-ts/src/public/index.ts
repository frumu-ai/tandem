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
  workspaceRoot?: string;
  workspace_root?: string;
  [key: string]: unknown;
}

// ─── Browser ─────────────────────────────────────────────────────────────────

export interface BrowserBlockingIssue {
  code?: string;
  message?: string;
  [key: string]: unknown;
}

export interface BrowserBinaryStatus {
  found?: boolean;
  path?: string | null;
  version?: string | null;
  channel?: string | null;
  [key: string]: unknown;
}

export interface BrowserStatusResponse {
  enabled?: boolean;
  runnable?: boolean;
  headless_default?: boolean;
  sidecar?: BrowserBinaryStatus;
  browser?: BrowserBinaryStatus;
  blocking_issues?: BrowserBlockingIssue[];
  recommendations?: string[];
  install_hints?: string[];
  last_error?: string | null;
  [key: string]: unknown;
}

export interface BrowserInstallResponse {
  ok?: boolean;
  code?: string;
  error?: string;
  version?: string;
  asset_name?: string;
  installed_path?: string;
  downloaded_bytes?: number;
  status?: BrowserStatusResponse;
  [key: string]: unknown;
}

export interface BrowserSmokeTestResponse {
  ok?: boolean;
  code?: string;
  error?: string;
  status?: BrowserStatusResponse;
  url?: string;
  final_url?: string;
  title?: string;
  load_state?: string;
  element_count?: number;
  excerpt?: string | null;
  closed?: boolean;
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

export type ToolMode = "auto" | "none" | "required";
export type ContextMode = "auto" | "compact" | "full";

export interface PromptRoutingOptions {
  toolMode?: ToolMode;
  toolAllowlist?: string[];
  contextMode?: ContextMode;
}

export interface PromptTextPartInput {
  type: "text";
  text: string;
}

export interface PromptFilePartInput {
  type: "file";
  mime: string;
  filename?: string;
  url: string;
}

export type PromptPartInput = PromptTextPartInput | PromptFilePartInput;

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
  catalog_source?: string;
  catalog_status?: string;
  catalog_message?: string;
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

export interface PersonalityProfile {
  preset?: string;
  customInstructions?: string | null;
}

export interface PersonalityConfig {
  default?: PersonalityProfile;
  perAgent?: Record<string, PersonalityProfile>;
}

export interface BotIdentityAliases {
  desktop?: string;
  tui?: string;
  portal?: string;
  controlPanel?: string;
  channels?: string;
  protocol?: string;
  cli?: string;
}

export interface BotIdentity {
  canonicalName?: string;
  avatarUrl?: string | null;
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

export interface ChannelVerifyResponse {
  ok: boolean;
  channel: ChannelName;
  checks?: Record<string, boolean | null>;
  statusCodes?: Record<string, number | null>;
  hints?: string[];
  details?: JsonObject;
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
  userId?: string;
  sourceType?: string;
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
  ok?: boolean;
  stored?: boolean;
  tier?: string;
  partitionKey?: string;
  auditId?: string;
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
  text?: string;
  content?: string;
  score?: number;
  sourceType?: string;
  runId?: string;
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
  newMemoryId?: string;
  toTier?: string;
  auditId?: string;
  [key: string]: unknown;
}

export interface MemoryDemoteOptions {
  id: string;
  runId?: string;
}

export interface MemoryDemoteResponse {
  ok: boolean;
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

export type SkillLocation = "project" | "global";

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
  conflictPolicy?: "skip" | "overwrite" | "rename";
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

export interface SkillCatalogRecord {
  info: SkillRecord;
  sections?: string[];
  missing_sections?: string[];
  missingSections?: string[];
  schedule_compatibility?: string[];
  scheduleCompatibility?: string[];
  has_manifest?: boolean;
  hasManifest?: boolean;
  has_workflow?: boolean;
  hasWorkflow?: boolean;
}

export interface SkillValidationIssue {
  code: string;
  level: "error" | "warning" | string;
  message: string;
}

export interface SkillValidationItem {
  source: string;
  valid: boolean;
  name?: string;
  issues?: SkillValidationIssue[];
  sections?: string[];
  missing_sections?: string[];
  missingSections?: string[];
  schedule_compatibility?: string[];
  scheduleCompatibility?: string[];
  has_manifest?: boolean;
  hasManifest?: boolean;
  has_workflow?: boolean;
  hasWorkflow?: boolean;
}

export interface SkillValidationResponse {
  items: SkillValidationItem[];
  total: number;
  valid: number;
  invalid: number;
}

export interface SkillRouterMatch {
  skill_name?: string;
  skillName?: string;
  confidence: number;
  reason: string;
}

export interface SkillRouterMatchResponse {
  decision: "match" | "no_match" | string;
  skill_name?: string;
  skillName?: string;
  confidence: number;
  reason: string;
  top_matches?: SkillRouterMatch[];
  topMatches?: SkillRouterMatch[];
}

export interface SkillsEvalCaseInput {
  prompt: string;
  expectedSkill?: string;
  expected_skill?: string;
}

export interface SkillsBenchmarkEvalResponse {
  status: string;
  total: number;
  passed: number;
  failed: number;
  accuracy: number;
  threshold: number;
  cases: Array<Record<string, unknown>>;
}

export interface SkillsTriggerEvalResponse {
  status: string;
  skillName?: string;
  skill_name?: string;
  threshold: number;
  total: number;
  truePositive?: number;
  true_positive?: number;
  falseNegative?: number;
  false_negative?: number;
  recall: number;
  cases: Array<Record<string, unknown>>;
}

export interface SkillCompileResponse {
  status: string;
  skillName?: string;
  skill_name?: string;
  workflowKind?: string;
  workflow_kind?: string;
  validation?: Record<string, unknown>;
  executionPlan?: Record<string, unknown>;
  execution_plan?: Record<string, unknown>;
}

export interface SkillGenerateResponse {
  status: string;
  prompt: string;
  router?: Record<string, unknown>;
  artifacts?: Record<string, string>;
}

export interface SkillGenerateInstallResponse {
  status: string;
  skill?: SkillRecord;
  validation?: Record<string, unknown>;
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

// ─── Workflows ───────────────────────────────────────────────────────────────

export interface WorkflowRecord {
  id?: string;
  workflowId?: string;
  workflow_id?: string;
  name?: string;
  description?: string;
  enabled?: boolean;
  [key: string]: unknown;
}

export interface WorkflowListResponse {
  workflows: WorkflowRecord[];
  count: number;
}

export interface WorkflowRunRecord {
  id?: string;
  runId?: string;
  run_id?: string;
  workflowId?: string;
  workflow_id?: string;
  status?: string;
  createdAtMs?: number;
  created_at_ms?: number;
  updatedAtMs?: number;
  updated_at_ms?: number;
  [key: string]: unknown;
}

export interface WorkflowRunListResponse {
  runs: WorkflowRunRecord[];
  count: number;
}

export interface WorkflowHookRecord {
  id?: string;
  workflowId?: string;
  workflow_id?: string;
  eventType?: string;
  event_type?: string;
  enabled?: boolean;
  [key: string]: unknown;
}

export interface WorkflowHookListResponse {
  hooks: WorkflowHookRecord[];
  count: number;
}

// ─── Bug Monitor ─────────────────────────────────────────────────────────────

export interface BugMonitorConfigRow {
  enabled?: boolean;
  paused?: boolean;
  workspace_root?: string | null;
  repo?: string | null;
  mcp_server?: string | null;
  provider_preference?: string | null;
  model_policy?: JsonObject | null;
  auto_create_new_issues?: boolean;
  require_approval_for_new_issues?: boolean;
  auto_comment_on_matched_open_issues?: boolean;
  label_mode?: string | null;
  [key: string]: unknown;
}

export interface BugMonitorConfigResponse {
  bug_monitor: BugMonitorConfigRow;
}

export interface BugMonitorStatusRow {
  config?: BugMonitorConfigRow;
  readiness?: Record<string, boolean>;
  runtime?: JsonObject;
  required_capabilities?: Record<string, boolean>;
  missing_required_capabilities?: string[];
  resolved_capabilities?: JsonObject[];
  discovered_mcp_tools?: string[];
  selected_server_binding_candidates?: JsonObject[];
  binding_source_version?: string | null;
  bindings_last_merged_at_ms?: number | null;
  selected_model?: JsonObject | null;
  pending_drafts?: number;
  pending_posts?: number;
  last_activity_at_ms?: number | null;
  last_error?: string | null;
  [key: string]: unknown;
}

export interface BugMonitorStatusResponse {
  status: BugMonitorStatusRow;
}

export interface BugMonitorIncidentRecord {
  incident_id: string;
  fingerprint?: string;
  event_type?: string;
  status?: string;
  repo?: string;
  workspace_root?: string;
  title?: string;
  detail?: string | null;
  excerpt?: string[];
  occurrence_count?: number;
  created_at_ms?: number;
  updated_at_ms?: number;
  draft_id?: string | null;
  triage_run_id?: string | null;
  last_error?: string | null;
  [key: string]: unknown;
}

export interface BugMonitorIncidentListResponse {
  incidents: BugMonitorIncidentRecord[];
  count: number;
}

export interface BugMonitorDraftRecord {
  draft_id: string;
  fingerprint?: string;
  repo?: string;
  status?: string;
  created_at_ms?: number;
  triage_run_id?: string | null;
  issue_number?: number | null;
  title?: string | null;
  detail?: string | null;
  github_status?: string | null;
  github_issue_url?: string | null;
  github_comment_url?: string | null;
  github_posted_at_ms?: number | null;
  matched_issue_number?: number | null;
  matched_issue_state?: string | null;
  evidence_digest?: string | null;
  last_post_error?: string | null;
  [key: string]: unknown;
}

export interface BugMonitorDraftListResponse {
  drafts: BugMonitorDraftRecord[];
  count: number;
}

export interface BugMonitorPostRecord {
  post_id: string;
  draft_id?: string;
  repo?: string;
  operation?: string;
  status?: string;
  issue_number?: number | null;
  issue_url?: string | null;
  comment_url?: string | null;
  error?: string | null;
  updated_at_ms?: number | null;
  [key: string]: unknown;
}

export interface BugMonitorPostListResponse {
  posts: BugMonitorPostRecord[];
  count: number;
}

// ─── Packs + Capabilities ────────────────────────────────────────────────────

export interface PackInstallRecord {
  pack_id: string;
  name: string;
  version: string;
  pack_type?: string;
  install_path?: string;
  sha256?: string;
  installed_at_ms?: number;
  routines_enabled?: boolean;
  [key: string]: unknown;
}

export interface PacksListResponse {
  packs: PackInstallRecord[];
}

export interface PackInspectionResponse {
  pack: {
    installed: PackInstallRecord;
    manifest?: JsonObject;
    trust?: JsonObject;
    risk?: JsonObject;
    permission_sheet?: JsonObject;
  };
}

export interface PackInstallOptions {
  path?: string;
  url?: string;
  source?: JsonObject;
}

export interface PackUninstallOptions {
  pack_id?: string;
  name?: string;
  version?: string;
}

export interface PackExportOptions {
  pack_id?: string;
  name?: string;
  version?: string;
  output_path?: string;
}

export interface PackDetectOptions {
  path: string;
  attachment_id?: string;
  connector?: string;
  channel_id?: string;
  sender_id?: string;
}

export interface CapabilityBindingRecord {
  capability_id: string;
  provider: string;
  tool_name: string;
  tool_name_aliases?: string[];
  request_transform?: JsonObject | null;
  response_transform?: JsonObject | null;
  metadata?: JsonObject;
}

export interface CapabilityBindingsFile {
  schema_version: string;
  generated_at?: string | null;
  bindings: CapabilityBindingRecord[];
}

export interface CapabilityResolveInput {
  workflow_id?: string;
  required_capabilities?: string[];
  optional_capabilities?: string[];
  provider_preference?: string[];
  available_tools?: Array<{
    provider: string;
    tool_name: string;
    schema?: JsonObject;
  }>;
}

export interface CapabilityReadinessInput {
  workflow_id?: string;
  required_capabilities?: string[];
  optional_capabilities?: string[];
  provider_preference?: string[];
  available_tools?: Array<{
    provider: string;
    tool_name: string;
    schema?: JsonObject;
  }>;
  allow_unbound?: boolean;
}

export interface CapabilityBlockingIssue {
  code: string;
  message: string;
  capability_ids?: string[];
  providers?: string[];
  tools?: string[];
}

export interface CapabilityReadinessOutput {
  workflow_id: string;
  runnable: boolean;
  resolved?: JsonObject[];
  missing_required_capabilities?: string[];
  unbound_capabilities?: string[];
  missing_optional_capabilities?: string[];
  missing_servers?: string[];
  disconnected_servers?: string[];
  auth_pending_tools?: string[];
  missing_secret_refs?: string[];
  considered_bindings?: number;
  recommendations?: string[];
  blocking_issues?: CapabilityBlockingIssue[];
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
  activeSessionIds?: string[];
  active_session_ids?: string[];
  latestSessionId?: string | null;
  latest_session_id?: string | null;
  attachEventStream?: string | null;
  attach_event_stream?: string | null;
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

// ─── Coder ───────────────────────────────────────────────────────────────────

export interface CoderRepoBinding {
  projectId?: string;
  project_id?: string;
  workspaceId?: string;
  workspace_id?: string;
  workspaceRoot?: string;
  workspace_root?: string;
  repoSlug: string;
  repo_slug?: string;
  defaultBranch?: string | null;
  default_branch?: string | null;
}

export interface CoderGithubRef {
  kind: "issue" | "pull_request" | string;
  number: number;
  url?: string | null;
}

export interface CoderRunRecord {
  coderRunId?: string;
  coder_run_id?: string;
  workflowMode?: string;
  workflow_mode?: string;
  linkedContextRunId?: string;
  linked_context_run_id?: string;
  repoBinding?: CoderRepoBinding;
  repo_binding?: CoderRepoBinding;
  githubRef?: CoderGithubRef | null;
  github_ref?: CoderGithubRef | null;
  sourceClient?: string | null;
  source_client?: string | null;
  status?: string;
  phase?: string;
  createdAtMs?: number;
  created_at_ms?: number;
  updatedAtMs?: number;
  updated_at_ms?: number;
  [key: string]: unknown;
}

export interface CoderRunsListResponse {
  runs: CoderRunRecord[];
  count?: number;
}

export interface CoderRunGetResponse {
  coderRun?: CoderRunRecord;
  coder_run?: CoderRunRecord;
  run?: JsonObject;
  [key: string]: unknown;
}

export interface CoderArtifactRecord {
  id: string;
  tsMs?: number;
  ts_ms?: number;
  path: string;
  artifactType?: string;
  artifact_type?: string;
  stepId?: string | null;
  step_id?: string | null;
  sourceEventId?: string | null;
  source_event_id?: string | null;
  [key: string]: unknown;
}

export interface CoderArtifactsResponse {
  artifacts: CoderArtifactRecord[];
  count?: number;
  [key: string]: unknown;
}

export interface CoderMemoryHitRecord {
  [key: string]: unknown;
}

export interface CoderMemoryHitsResponse {
  hits: CoderMemoryHitRecord[];
  count?: number;
  [key: string]: unknown;
}

export interface CoderMemoryCandidateRecord {
  candidateId?: string;
  candidate_id?: string;
  kind?: string;
  summary?: string | null;
  payload?: JsonObject;
  artifact?: CoderArtifactRecord | null;
  createdAtMs?: number;
  created_at_ms?: number;
  [key: string]: unknown;
}

export interface CoderMemoryCandidatesResponse {
  candidates: CoderMemoryCandidateRecord[];
  count?: number;
  [key: string]: unknown;
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

export interface AgentTeamTemplateCreateInput {
  template: JsonObject;
}

export interface AgentTeamTemplatePatchInput {
  role?: string;
  systemPrompt?: string;
  skills?: JsonObject[];
  defaultBudget?: JsonObject;
  capabilities?: JsonObject;
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

// ─── Automations V2 ──────────────────────────────────────────────────────────

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
  cronExpression?: string;
  intervalSeconds?: number;
  timezone: string;
  misfirePolicy?: "skip" | "run_once" | "catch_up";
}

export interface AutomationV2AgentProfile {
  agentId: string;
  templateId?: string;
  displayName: string;
  avatarUrl?: string;
  modelPolicy?: JsonObject;
  skills?: string[];
  toolPolicy?: { allowlist?: string[]; denylist?: string[] };
  mcpPolicy?: { allowedServers?: string[]; allowedTools?: string[] };
  approvalPolicy?: string;
}

export interface AutomationV2FlowNode {
  nodeId: string;
  agentId: string;
  objective: string;
  dependsOn?: string[];
  inputRefs?: Array<{ fromStepId?: string; from_step_id?: string; alias: string }>;
  outputContract?: { kind: string };
  output_contract?: { kind: string };
  retryPolicy?: JsonObject;
  timeoutMs?: number;
}

export interface AutomationV2Spec {
  automationId?: string;
  name: string;
  description?: string;
  status?: AutomationV2Status;
  schedule: AutomationV2Schedule;
  agents: AutomationV2AgentProfile[];
  flow: { nodes: AutomationV2FlowNode[] };
  execution?: {
    maxParallelAgents?: number;
    maxTotalRuntimeMs?: number;
    maxTotalToolCalls?: number;
  };
  outputTargets?: string[];
  creatorId?: string;
  workspaceRoot?: string;
  workspace_root?: string;
  metadata?: JsonObject;
  [key: string]: unknown;
}

export interface WorkflowPlanStep {
  stepId?: string;
  step_id?: string;
  kind: string;
  objective: string;
  dependsOn?: string[];
  depends_on?: string[];
  agentRole?: string;
  agent_role?: string;
  inputRefs?: Array<{ fromStepId?: string; from_step_id?: string; alias: string }>;
  input_refs?: Array<{ fromStepId?: string; from_step_id?: string; alias: string }>;
  outputContract?: { kind: string };
  output_contract?: { kind: string };
}

export interface WorkflowPlan {
  planId?: string;
  plan_id?: string;
  plannerVersion?: string;
  planner_version?: string;
  planSource?: string;
  plan_source?: string;
  originalPrompt?: string;
  original_prompt?: string;
  normalizedPrompt?: string;
  normalized_prompt?: string;
  confidence?: string;
  title: string;
  description?: string;
  schedule: AutomationV2Schedule;
  executionTarget?: string;
  execution_target?: string;
  workspaceRoot?: string;
  workspace_root?: string;
  steps: WorkflowPlanStep[];
  allowedMcpServers?: string[];
  allowed_mcp_servers?: string[];
  operatorPreferences?: JsonObject;
  operator_preferences?: JsonObject;
  metadata?: JsonObject;
  [key: string]: unknown;
}

export interface WorkflowPlanPackBuilderExportRequest {
  enabled?: boolean;
  sessionId?: string;
  session_id?: string;
  threadKey?: string;
  thread_key?: string;
  autoApply?: boolean;
  auto_apply?: boolean;
}

export interface WorkflowPlanPackBuilderExportResult {
  status?: string;
  error?: string;
  httpStatus?: number;
  http_status?: number;
  planId?: string;
  plan_id?: string;
  sessionId?: string;
  session_id?: string;
  threadKey?: string;
  thread_key?: string;
  autoApplyRequested?: boolean;
  auto_apply_requested?: boolean;
  autoApplyReady?: boolean;
  auto_apply_ready?: boolean;
  [key: string]: unknown;
}

export interface WorkflowPlanChatMessage {
  role: string;
  text: string;
  createdAtMs?: number;
  created_at_ms?: number;
  [key: string]: unknown;
}

export interface WorkflowPlanConversation {
  conversationId?: string;
  conversation_id?: string;
  planId?: string;
  plan_id?: string;
  createdAtMs?: number;
  created_at_ms?: number;
  updatedAtMs?: number;
  updated_at_ms?: number;
  messages: WorkflowPlanChatMessage[];
  [key: string]: unknown;
}

export interface AutomationV2RunRecord {
  runId: string;
  automationId: string;
  status: AutomationV2RunStatus;
  checkpoint?: JsonObject;
  activeSessionIds?: string[];
  activeInstanceIds?: string[];
  [key: string]: unknown;
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

export interface RunCompleteEvent extends EngineEventBase {
  type: "run.complete";
}

export interface RunFailedEvent extends EngineEventBase {
  type: "run.failed";
}

export interface SessionRunFinishedEvent extends EngineEventBase {
  type: "session.run.finished";
}

export interface SessionRunCompletedEvent extends EngineEventBase {
  type: "session.run.completed";
}

export interface SessionRunFailedEvent extends EngineEventBase {
  type: "session.run.failed";
}

export interface RunCancelledEvent extends EngineEventBase {
  type: "run.cancelled" | "run.canceled";
}

export interface SessionRunCancelledEvent extends EngineEventBase {
  type: "session.run.cancelled" | "session.run.canceled";
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
  | "run.complete"
  | "run.completed"
  | "run.failed"
  | "run.cancelled"
  | "run.canceled"
  | "session.run.finished"
  | "session.run.completed"
  | "session.run.failed"
  | "session.run.cancelled"
  | "session.run.canceled"
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
  | RunCompleteEvent
  | RunCompletedEvent
  | RunFailedEvent
  | RunCancelledEvent
  | SessionRunFinishedEvent
  | SessionRunCompletedEvent
  | SessionRunFailedEvent
  | SessionRunCancelledEvent
  | ToolCalledEvent
  | ToolResultEvent
  | ApprovalRequestedEvent
  | ApprovalResolvedEvent
  | RoutineTriggeredEvent
  | RoutineCompletedEvent
  | SessionResponseEvent
  | UnknownEvent;
