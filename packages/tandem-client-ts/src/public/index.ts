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

export type ErrorCode =
  | "AUTH_REQUIRED"
  | "TENANT_CONTEXT_DENIED"
  | "TENANT_SCOPE_DENIED"
  | "VALIDATION_FAILED"
  | "SESSION_NOT_FOUND"
  | "SESSION_RUN_CONFLICT"
  | "RATE_LIMITED"
  | "PROMPT_TIMEOUT"
  | "ENGINE_STARTING"
  | "ENGINE_STARTUP_FAILED"
  | "APPROVAL_REPLY_INVALID"
  | "APPROVAL_REQUEST_NOT_FOUND"
  | "APPROVAL_PERSISTENCE_FAILED"
  | "MCP_REQUEST_DENIED"
  | "MCP_STDIO_TRANSPORT_DENIED"
  | "MCP_REFRESH_FAILED"
  | "MCP_OAUTH_FAILED"
  | "SKILLS_ERROR"
  | "OPTIMIZATION_VALIDATION_FAILED"
  | "OPTIMIZATION_NOT_FOUND"
  | "OPTIMIZATION_CONFLICT"
  | "PERSISTENCE_FAILED"
  | "INTERNAL_ERROR";

export interface ErrorEnvelope {
  error: string;
  code?: ErrorCode;
  retryable: boolean;
  [key: string]: unknown;
}

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

// ─── Storage ─────────────────────────────────────────────────────────────────

export interface StorageFileRecord {
  path: string;
  relative_to_base?: string;
  relativeToBase?: string;
  size_bytes?: number;
  sizeBytes?: number;
  modified_at_ms?: number | null;
  modifiedAtMs?: number | null;
  [key: string]: unknown;
}

export interface StorageFilesResponse {
  root?: string;
  base?: string;
  count?: number;
  limit?: number;
  files: StorageFileRecord[];
  [key: string]: unknown;
}

export interface StorageRepairResponse {
  status?: string;
  marker_updated?: boolean;
  markerUpdated?: boolean;
  sessions_merged?: number;
  sessionsMerged?: number;
  messages_recovered?: number;
  messagesRecovered?: number;
  parts_recovered?: number;
  partsRecovered?: number;
  legacy_counts?: JsonObject;
  legacyCounts?: JsonObject;
  imported_counts?: JsonObject;
  importedCounts?: JsonObject;
  [key: string]: unknown;
}

export interface WorktreeCleanupStaleRecord {
  path?: string;
  branch?: string | null;
  [key: string]: unknown;
}

export interface WorktreeCleanupActionRecord {
  path?: string;
  branch?: string | null;
  via?: string | null;
  code?: string | null;
  error?: string | null;
  stderr?: string | null;
  branch_deleted?: boolean | null;
  branchDeleteError?: string | null;
  branch_delete_error?: string | null;
  [key: string]: unknown;
}

export interface WorktreeCleanupResponse {
  ok?: boolean;
  dry_run?: boolean;
  dryRun?: boolean;
  repo_root?: string;
  repoRoot?: string;
  managed_root?: string;
  managedRoot?: string;
  tracked_paths?: string[];
  trackedPaths?: string[];
  active_paths?: string[];
  activePaths?: string[];
  stale_paths?: WorktreeCleanupStaleRecord[];
  stalePaths?: WorktreeCleanupStaleRecord[];
  cleaned_worktrees?: WorktreeCleanupActionRecord[];
  cleanedWorktrees?: WorktreeCleanupActionRecord[];
  orphan_dirs?: string[];
  orphanDirs?: string[];
  orphan_dirs_removed?: WorktreeCleanupActionRecord[];
  orphanDirsRemoved?: WorktreeCleanupActionRecord[];
  failures?: WorktreeCleanupActionRecord[];
  [key: string]: unknown;
}

// ─── Sessions ────────────────────────────────────────────────────────────────

export interface CreateSessionOptions {
  title?: string;
  directory?: string;
  permissions?: PermissionRule[];
  provider?: string;
  model?: string;
  sourceKind?: string;
  sourceMetadata?: JsonObject;
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
  sourceKind?: string;
  sourceMetadata?: JsonObject;
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
  source?: string;
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
  tokenMasked?: string | null;
  allowedUsers?: string[];
  mentionOnly?: boolean;
  styleProfile?: string;
  guildId?: string;
  channelId?: string;
  securityProfile?: string;
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

export interface ChannelToolPreferences {
  enabled_tools: string[];
  disabled_tools: string[];
  enabled_mcp_servers: string[];
}

export interface ChannelToolPreferencesInput {
  enabled_tools?: string[];
  disabled_tools?: string[];
  enabled_mcp_servers?: string[];
  reset?: boolean;
}

// ─── MCP ─────────────────────────────────────────────────────────────────────

export interface AddMcpServerOptions {
  name: string;
  transport: string;
  headers?: Record<string, string>;
  secret_headers?: Record<string, unknown>;
  enabled?: boolean;
  allowed_tools?: string[];
  purpose?: string;
  grounding_required?: boolean;
}

export interface PatchMcpServerOptions {
  enabled?: boolean;
  allowed_tools?: string[];
  clear_allowed_tools?: boolean;
  purpose?: string;
  grounding_required?: boolean;
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

export type MemoryImportFormat = "directory" | "openclaw";
export type MemoryImportTier = "global" | "project" | "session";

export interface MemoryImportPathOptions {
  path: string;
  format?: MemoryImportFormat;
  tier?: MemoryImportTier;
  projectId?: string;
  sessionId?: string;
  sourceBindingId?: string;
  syncDeletes?: boolean;
}

export interface MemoryImportResponse {
  ok: boolean;
  source?: {
    kind: "path";
    path: string;
  };
  format: MemoryImportFormat;
  tier: MemoryImportTier;
  project_id?: string | null;
  session_id?: string | null;
  source_binding_id?: string | null;
  sync_deletes: boolean;
  discovered_files: number;
  files_processed: number;
  indexed_files: number;
  skipped_files: number;
  deleted_files: number;
  chunks_created: number;
  errors: number;
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

export type BugMonitorDestinationKind =
  | "github_issue"
  | "linear_issue"
  | "webhook"
  | "telemetry"
  | "mcp_tool"
  | "internal_memory";

export type BugMonitorApprovalPolicy = "inherit" | "always" | "high_risk" | "never";
export type BugMonitorSourceKind =
  | "tandem_runtime"
  | "external_app"
  | "ci"
  | "agent_runtime"
  | "mcp_gateway"
  | "customer_system";

export interface BugMonitorDestinationConfig {
  destination_id: string;
  name: string;
  kind?: BugMonitorDestinationKind | string;
  enabled?: boolean;
  require_approval?: boolean;
  repo?: string | null;
  mcp_server?: string | null;
  linear_team?: string | null;
  linear_project?: string | null;
  webhook_url?: string | null;
  webhook_secret_ref?: string | null;
  telemetry_path?: string | null;
  mcp_tool?: string | null;
  memory_category?: string | null;
  route_tags?: string[];
  config?: JsonObject | null;
  [key: string]: unknown;
}

export interface BugMonitorRouteConfig {
  route_id: string;
  name: string;
  enabled?: boolean;
  priority?: number;
  destination_ids?: string[];
  approval_policy?: BugMonitorApprovalPolicy | string;
  match_event_types?: string[];
  match_sources?: string[];
  match_components?: string[];
  match_risk_levels?: string[];
  match_confidence?: string[];
  match_expected_destinations?: string[];
  match_project_ids?: string[];
  match_log_source_ids?: string[];
  match_route_tags?: string[];
  match_source_kinds?: string[];
  match_tenant_ids?: string[];
  match_workspace_ids?: string[];
  match_event_schema_versions?: string[];
  [key: string]: unknown;
}

export interface BugMonitorSafetyDefaults {
  require_approval_for_high_risk?: boolean;
  redact_secrets?: boolean;
  block_unready_destinations?: boolean;
  retention_days?: number | null;
  [key: string]: unknown;
}

export interface BugMonitorDestinationReadiness {
  destination_id: string;
  kind?: BugMonitorDestinationKind | string;
  enabled?: boolean;
  ready?: boolean;
  publish_ready?: boolean;
  requires_approval?: boolean;
  missing?: string[];
  detail?: string | null;
  [key: string]: unknown;
}

export interface BugMonitorRoutePreviewMatch {
  route_id?: string | null;
  route_name?: string | null;
  destination_ids?: string[];
  approval_required?: boolean;
  reason?: string | null;
  [key: string]: unknown;
}

export interface BugMonitorRoutePreviewResponse {
  matches?: BugMonitorRoutePreviewMatch[];
  destinations?: BugMonitorDestinationConfig[];
  readiness?: BugMonitorDestinationReadiness[];
  default_destination_ids?: string[];
  effective_destination_ids?: string[];
  approval_required?: boolean;
  blocked?: boolean;
  blocked_reasons?: string[];
  [key: string]: unknown;
}

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
  monitored_projects?: BugMonitorMonitoredProject[];
  destinations?: BugMonitorDestinationConfig[];
  routes?: BugMonitorRouteConfig[];
  default_destination_ids?: string[];
  safety_defaults?: BugMonitorSafetyDefaults | null;
  [key: string]: unknown;
}

export interface BugMonitorLogSource {
  source_id?: string;
  path?: string;
  source_kind?: BugMonitorSourceKind | string | null;
  format?: "auto" | "json" | "plaintext" | string;
  minimum_level?: "error" | "warn" | string;
  watch_interval_seconds?: number;
  enabled?: boolean;
  paused?: boolean;
  start_position?: "end" | "beginning" | string;
  max_bytes_per_poll?: number;
  max_candidates_per_poll?: number;
  fingerprint_cooldown_ms?: number;
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  default_route_tags?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  approval_policy?: BugMonitorApprovalPolicy | string;
  redaction_profile?: string | null;
  retention_profile?: string | null;
}

export interface BugMonitorMonitoredProject {
  project_id?: string;
  name?: string;
  enabled?: boolean;
  paused?: boolean;
  repo?: string;
  workspace_root?: string;
  source_kind?: BugMonitorSourceKind | string;
  mcp_server?: string | null;
  model_policy?: JsonObject | null;
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  default_route_tags?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  approval_policy?: BugMonitorApprovalPolicy | string;
  redaction_profile?: string | null;
  retention_profile?: string | null;
  auto_create_new_issues?: boolean;
  require_approval_for_new_issues?: boolean;
  auto_comment_on_matched_open_issues?: boolean;
  log_sources?: BugMonitorLogSource[];
}

export interface BugMonitorConfigResponse {
  bug_monitor: BugMonitorConfigRow;
}

export interface BugMonitorLogSourceRuntimeStatus {
  project_id?: string;
  source_id?: string;
  path?: string;
  healthy?: boolean;
  offset?: number;
  inode?: string | null;
  file_size?: number | null;
  last_poll_at_ms?: number | null;
  last_candidate_at_ms?: number | null;
  last_submitted_at_ms?: number | null;
  last_error?: string | null;
  consecutive_errors?: number;
  total_bytes_read?: number;
  total_candidates?: number;
  total_submitted?: number;
}

export interface BugMonitorLogWatcherStatus {
  running?: boolean;
  enabled_projects?: number;
  enabled_sources?: number;
  last_poll_at_ms?: number | null;
  last_error?: string | null;
  sources?: BugMonitorLogSourceRuntimeStatus[];
}

export interface BugMonitorStatusRow {
  config?: BugMonitorConfigRow;
  readiness?: Record<string, boolean>;
  runtime?: JsonObject;
  log_watcher?: BugMonitorLogWatcherStatus;
  required_capabilities?: Record<string, boolean>;
  missing_required_capabilities?: string[];
  resolved_capabilities?: JsonObject[];
  discovered_mcp_tools?: string[];
  selected_server_binding_candidates?: JsonObject[];
  destinations?: BugMonitorDestinationConfig[];
  destination_readiness?: BugMonitorDestinationReadiness[];
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
  project_id?: string | null;
  log_source_id?: string | null;
  source_kind?: BugMonitorSourceKind | string | null;
  detail?: string | null;
  excerpt?: string[];
  source?: string | null;
  run_id?: string | null;
  session_id?: string | null;
  correlation_id?: string | null;
  component?: string | null;
  level?: string | null;
  occurrence_count?: number;
  created_at_ms?: number;
  updated_at_ms?: number;
  last_seen_at_ms?: number | null;
  draft_id?: string | null;
  triage_run_id?: string | null;
  last_error?: string | null;
  duplicate_summary?: JsonObject | null;
  duplicate_matches?: JsonValue[] | null;
  event_payload?: JsonValue | null;
  confidence?: string | null;
  risk_level?: string | null;
  expected_destination?: string | null;
  route_tags?: string[];
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  source_approval_policy?: BugMonitorApprovalPolicy | string | null;
  redaction_profile?: string | null;
  retention_profile?: string | null;
  evidence_refs?: string[];
  quality_gate?: JsonObject | null;
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
  project_id?: string | null;
  log_source_id?: string | null;
  source_kind?: BugMonitorSourceKind | string | null;
  status?: string;
  created_at_ms?: number;
  approval_granted_at_ms?: number | null;
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
  confidence?: string | null;
  risk_level?: string | null;
  expected_destination?: string | null;
  route_tags?: string[];
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  source_approval_policy?: BugMonitorApprovalPolicy | string | null;
  redaction_profile?: string | null;
  retention_profile?: string | null;
  evidence_refs?: string[];
  quality_gate?: JsonObject | null;
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
  incident_id?: string | null;
  fingerprint?: string;
  repo?: string;
  operation?: string;
  status?: string;
  issue_number?: number | null;
  issue_url?: string | null;
  comment_id?: string | null;
  comment_url?: string | null;
  destination_id?: string | null;
  destination_kind?: BugMonitorDestinationKind | string | null;
  route_id?: string | null;
  route_match_reason?: string | null;
  external_id?: string | null;
  external_url?: string | null;
  external_title?: string | null;
  target_ref?: string | null;
  receipt?: JsonValue | null;
  evidence_digest?: string | null;
  confidence?: string | null;
  risk_level?: string | null;
  expected_destination?: string | null;
  evidence_refs?: string[];
  quality_gate?: JsonObject | null;
  idempotency_key?: string;
  response_excerpt?: string | null;
  error?: string | null;
  created_at_ms?: number | null;
  updated_at_ms?: number | null;
  [key: string]: unknown;
}

export interface BugMonitorPostListResponse {
  posts: BugMonitorPostRecord[];
  count: number;
}

export interface BugMonitorIntakeKeyRecord {
  key_id: string;
  project_id: string;
  name: string;
  key_hash?: string;
  enabled?: boolean;
  scopes?: string[];
  created_at_ms?: number | null;
  last_used_at_ms?: number | null;
  [key: string]: unknown;
}

export interface BugMonitorIntakeKeyListResponse {
  keys: BugMonitorIntakeKeyRecord[];
}

export interface BugMonitorIntakeKeyCreateInput {
  project_id: string;
  name: string;
  scopes?: string[];
}

export interface BugMonitorIntakeKeyCreateResponse {
  key: BugMonitorIntakeKeyRecord;
  raw_key: string;
}

export interface BugMonitorIntakeKeyDisableResponse {
  key: BugMonitorIntakeKeyRecord;
}

export interface BugMonitorLogSourceStateRecord {
  project_id?: string;
  source_id?: string;
  path?: string;
  inode?: string | null;
  offset?: number;
  partial_line?: string | null;
  partial_line_offset_start?: number | null;
  last_line_hash?: string | null;
  updated_at_ms?: number | null;
  last_error?: string | null;
  consecutive_errors?: number;
  total_bytes_read?: number;
  total_candidates?: number;
  total_submitted?: number;
  [key: string]: unknown;
}

export interface BugMonitorLogSourceResetResponse {
  project_id: string;
  source_id: string;
  state: BugMonitorLogSourceStateRecord;
}

export interface BugMonitorLogSourceReplayResponse {
  project_id: string;
  source_id: string;
  incident: BugMonitorIncidentRecord;
  draft?: BugMonitorDraftRecord | null;
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
  dryRun?: boolean;
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

export type CoderRemoteSyncState =
  | "in_sync"
  | "schema_drift"
  | "remote_state_diverged"
  | "projection_unavailable";

export interface CoderGithubProjectStatusOption {
  id: string;
  name: string;
}

export interface CoderGithubProjectStatusMapping {
  fieldId?: string;
  field_id?: string;
  fieldName?: string;
  field_name?: string;
  todo: CoderGithubProjectStatusOption;
  inProgress?: CoderGithubProjectStatusOption;
  in_progress: CoderGithubProjectStatusOption;
  inReview?: CoderGithubProjectStatusOption;
  in_review: CoderGithubProjectStatusOption;
  blocked: CoderGithubProjectStatusOption;
  done: CoderGithubProjectStatusOption;
}

export interface CoderGithubProjectBinding {
  owner: string;
  projectNumber?: number;
  project_number: number;
  repoSlug?: string | null;
  repo_slug?: string | null;
  mcpServer?: string | null;
  mcp_server?: string | null;
  schemaSnapshot?: JsonObject;
  schema_snapshot: JsonObject;
  schemaFingerprint?: string;
  schema_fingerprint: string;
  statusMapping?: CoderGithubProjectStatusMapping;
  status_mapping: CoderGithubProjectStatusMapping;
}

export interface CoderGithubProjectRef {
  owner: string;
  projectNumber?: number;
  project_number: number;
  projectItemId?: string;
  project_item_id: string;
  issueNumber?: number;
  issue_number: number;
  issueUrl?: string | null;
  issue_url?: string | null;
  schemaFingerprint?: string;
  schema_fingerprint: string;
  statusMapping?: CoderGithubProjectStatusMapping;
  status_mapping: CoderGithubProjectStatusMapping;
}

export interface CoderProjectBindingRecord {
  projectId?: string;
  project_id: string;
  repoBinding?: CoderRepoBinding;
  repo_binding: CoderRepoBinding;
  githubProjectBinding?: CoderGithubProjectBinding | null;
  github_project_binding?: CoderGithubProjectBinding | null;
  updatedAtMs?: number;
  updated_at_ms: number;
}

export interface CoderGithubProjectInboxItem {
  projectItemId?: string;
  project_item_id: string;
  title: string;
  statusName?: string;
  status_name: string;
  statusOptionId?: string | null;
  status_option_id?: string | null;
  issue?: {
    number: number;
    title: string;
    htmlUrl?: string | null;
    html_url?: string | null;
  } | null;
  actionable: boolean;
  unsupportedReason?: string | null;
  unsupported_reason?: string | null;
  linkedRun?: {
    coderRun?: CoderRunRecord;
    coder_run?: CoderRunRecord;
    active: boolean;
  } | null;
  linked_run?: {
    coderRun?: CoderRunRecord;
    coder_run?: CoderRunRecord;
    active: boolean;
  } | null;
  remoteSyncState?: CoderRemoteSyncState;
  remote_sync_state: CoderRemoteSyncState;
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
  githubProjectRef?: CoderGithubProjectRef | null;
  github_project_ref?: CoderGithubProjectRef | null;
  remoteSyncState?: CoderRemoteSyncState | null;
  remote_sync_state?: CoderRemoteSyncState | null;
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

export interface CoderProjectBindingGetResponse {
  binding?: CoderProjectBindingRecord | null;
}

export interface CoderProjectBindingPutResponse {
  ok?: boolean;
  binding: CoderProjectBindingRecord;
}

export interface CoderGithubProjectInboxResponse {
  projectId?: string;
  project_id: string;
  binding: CoderGithubProjectBinding;
  schemaDrift?: boolean;
  schema_drift: boolean;
  liveSchemaFingerprint?: string;
  live_schema_fingerprint: string;
  items: CoderGithubProjectInboxItem[];
}

export interface CoderGithubProjectIntakeResponse {
  ok?: boolean;
  deduped?: boolean;
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
  templateID?: string;
  displayName?: string;
  avatarUrl?: string;
  defaultModel?: JsonObject;
  systemPrompt?: string;
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
  displayName?: string;
  avatarUrl?: string;
  role?: string;
  systemPrompt?: string;
  defaultModel?: JsonObject;
  skills?: JsonObject[];
  defaultBudget?: JsonObject;
  capabilities?: JsonObject;
}

export interface AgentStandupComposeInput {
  name: string;
  workspaceRoot: string;
  schedule: JsonObject;
  participantTemplateIds: string[];
  reportPathTemplate?: string;
  modelPolicy?: JsonObject;
}

export interface AgentStandupComposeResponse {
  ok?: boolean;
  automation?: JsonObject;
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

export type KnowledgeScope = "run" | "project" | "global";
export type KnowledgeTrustLevel = "working" | "promoted" | "approved_default";
export type KnowledgeReuseMode = "disabled" | "preflight" | "on_demand";

export interface KnowledgeSpaceRef {
  scope?: KnowledgeScope;
  projectId?: string;
  project_id?: string;
  namespace?: string;
  spaceId?: string;
  space_id?: string;
  [key: string]: unknown;
}

export interface KnowledgeBinding {
  enabled?: boolean;
  reuseMode?: KnowledgeReuseMode;
  reuse_mode?: KnowledgeReuseMode;
  trustFloor?: KnowledgeTrustLevel;
  trust_floor?: KnowledgeTrustLevel;
  readSpaces?: KnowledgeSpaceRef[];
  read_spaces?: KnowledgeSpaceRef[];
  promoteSpaces?: KnowledgeSpaceRef[];
  promote_spaces?: KnowledgeSpaceRef[];
  namespace?: string;
  subject?: string;
  freshnessMs?: number;
  freshness_ms?: number;
  [key: string]: unknown;
}

export interface AutomationV2Spec {
  automationId?: string;
  name: string;
  description?: string;
  status?: AutomationV2Status;
  schedule: AutomationV2Schedule;
  knowledge?: KnowledgeBinding;
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

export interface WorkflowPlanDraftRecord {
  initial_plan: WorkflowPlan;
  current_plan: WorkflowPlan;
  plan_revision?: number;
  conversation: WorkflowPlanConversation;
  planner_diagnostics?: JsonValue;
  last_success_materialization?: JsonValue;
  review?: WorkflowPlanDraftReviewRecord | null;
  [key: string]: unknown;
}

export interface WorkflowPlanDraftReviewRecord {
  required_capabilities?: string[];
  requested_capabilities?: string[];
  blocked_capabilities?: string[];
  docs_mcp_used?: boolean;
  preview_payload?: JsonObject | null;
  createdAtMs?: number | null;
  created_at_ms?: number | null;
  updatedAtMs?: number | null;
  updated_at_ms?: number | null;
  validation_state?: string;
  validation_status?: string;
  approval_status?: string;
  [key: string]: unknown;
}

export interface WorkflowPlannerSessionOperationRecord {
  request_id: string;
  kind: string;
  status: string;
  started_at_ms: number;
  finished_at_ms?: number | null;
  response?: JsonValue;
  error?: string | null;
  [key: string]: unknown;
}

export interface WorkflowPlannerSessionPlanningRecord {
  mode?: string;
  source_platform?: string;
  source_channel?: string | null;
  requesting_actor?: string | null;
  created_by_agent?: string | null;
  draft_id?: string | null;
  linked_channel_session_id?: string | null;
  linked_draft_plan_id?: string | null;
  allowed_tools?: string[];
  blocked_tools?: string[];
  known_requirements?: string[];
  missing_requirements?: string[];
  validation_state?: string;
  validation_status?: string;
  approval_status?: string;
  docs_mcp_enabled?: boolean | null;
  started_at_ms?: number | null;
  updated_at_ms?: number | null;
  [key: string]: unknown;
}

export interface WorkflowPlanPreviewResponse {
  plan: WorkflowPlan;
  clarifier?: JsonObject | null;
  assistant_message?: JsonObject;
  planner_diagnostics?: JsonObject | null;
  plan_package?: JsonObject;
  plan_package_bundle?: JsonObject;
  plan_package_validation?: JsonObject;
  overlap_analysis?: JsonObject | null;
  teaching_library?: JsonObject;
}

export interface WorkflowPlanApplyResponse {
  ok?: boolean;
  plan?: WorkflowPlan;
  plan_package?: JsonObject;
  plan_package_bundle?: JsonObject;
  plan_package_validation?: JsonObject;
  approved_plan_materialization?: JsonObject;
  automation?: JsonObject;
  overlap_analysis?: JsonObject;
  pack_builder_export?: WorkflowPlanPackBuilderExportResult;
}

export interface WorkflowPlanChatResponse {
  plan: WorkflowPlan;
  conversation: WorkflowPlanConversation;
  assistant_message?: JsonObject;
  change_summary?: string[];
  clarifier?: JsonObject | null;
  planner_diagnostics?: JsonObject | null;
  plan_package?: JsonObject;
  plan_package_bundle?: JsonObject;
  plan_package_validation?: JsonObject;
  overlap_analysis?: JsonObject | null;
  teaching_library?: JsonObject;
}

export interface WorkflowPlanGetResponse {
  plan: WorkflowPlan;
  conversation: WorkflowPlanConversation;
  planner_diagnostics?: JsonObject | null;
  overlap_analysis?: JsonObject | null;
  plan_package?: JsonObject;
  plan_package_bundle?: JsonObject;
  plan_package_validation?: JsonObject;
  plan_package_replay?: JsonObject;
  teaching_library?: JsonObject;
}

export interface WorkflowPlanImportPreviewResponse {
  ok?: boolean;
  persisted?: boolean;
  bundle?: JsonObject;
  import_validation?: JsonObject;
  plan_package_preview?: JsonObject;
  plan_package_validation?: JsonObject;
  derived_scope_snapshot?: JsonObject;
  summary?: JsonObject;
  import_transform_log?: JsonValue[];
  import_source_bundle_digest?: string;
  session?: WorkflowPlannerSessionRecord;
}

export type WorkflowPlanImportResponse = WorkflowPlanImportPreviewResponse;

export interface WorkflowPlanPackExportResponse {
  ok?: boolean;
  pack?: JsonObject;
  exported?: {
    path?: string;
    download_url?: string;
    sha256?: string;
    bytes?: number;
    [key: string]: unknown;
  };
  manifest?: JsonObject;
  bundle?: JsonObject;
  marketplace_ready?: boolean;
  [key: string]: unknown;
}

export interface WorkflowPlanPackImportPreviewResponse {
  ok?: boolean;
  persisted?: boolean;
  is_pack?: boolean;
  manifest?: JsonObject;
  cover_image?: string | null;
  cover_image_data_url?: string | null;
  workflows?: JsonObject[];
  pack?: JsonObject;
  installed?: JsonObject;
  sessions?: WorkflowPlannerSessionRecord[];
  [key: string]: unknown;
}

export type WorkflowPlanPackImportResponse = WorkflowPlanPackImportPreviewResponse;

export interface WorkflowPlannerSessionRecord {
  session_id: string;
  project_slug: string;
  title: string;
  workspace_root: string;
  source_kind?: string;
  source_bundle_digest?: string | null;
  source_pack_id?: string | null;
  source_pack_version?: string | null;
  current_plan_id?: string;
  draft?: WorkflowPlanDraftRecord;
  goal?: string;
  notes?: string;
  planner_provider?: string;
  planner_model?: string;
  plan_source?: string;
  allowed_mcp_servers?: string[];
  operator_preferences?: JsonObject;
  planning?: WorkflowPlannerSessionPlanningRecord | null;
  import_validation?: JsonObject | null;
  import_transform_log?: string[];
  import_scope_snapshot?: JsonObject | null;
  operation?: WorkflowPlannerSessionOperationRecord | null;
  published_at_ms?: number | null;
  published_tasks?: JsonValue[];
  created_at_ms: number;
  updated_at_ms: number;
  [key: string]: unknown;
}

export interface WorkflowPlannerSessionListItem {
  session_id: string;
  title: string;
  project_slug: string;
  workspace_root: string;
  source_kind?: string;
  source_bundle_digest?: string | null;
  current_plan_id?: string;
  created_at_ms: number;
  updated_at_ms: number;
  goal?: string | null;
  planner_provider?: string | null;
  planner_model?: string | null;
  source_platform?: string | null;
  source_channel?: string | null;
  validation_status?: string | null;
  approval_status?: string | null;
}

export interface WorkflowPlannerSessionListResponse {
  sessions: WorkflowPlannerSessionListItem[];
  count: number;
}

export interface WorkflowPlannerSessionResponse {
  session: WorkflowPlannerSessionRecord;
}

export interface WorkflowPlannerSessionCreateResponse extends WorkflowPlannerSessionResponse {}
export interface WorkflowPlannerSessionPatchResponse extends WorkflowPlannerSessionResponse {}
export interface WorkflowPlannerSessionDuplicateResponse extends WorkflowPlannerSessionResponse {}

export interface WorkflowPlannerSessionStartResponse extends WorkflowPlanChatResponse {
  session?: WorkflowPlannerSessionRecord;
}

export interface WorkflowPlannerSessionMessageResponse extends WorkflowPlanChatResponse {
  session?: WorkflowPlannerSessionRecord;
}

export interface WorkflowPlannerSessionResetResponse extends WorkflowPlanChatResponse {
  session?: WorkflowPlannerSessionRecord;
}

export type ExecutionProfile = "strict" | "guided" | "yolo";

export interface AutomationV2RunRecord {
  runId: string;
  automationId: string;
  status: AutomationV2RunStatus;
  checkpoint?: JsonObject;
  activeSessionIds?: string[];
  activeInstanceIds?: string[];
  /** Resolved execution profile for this run (snake_case from server). */
  effective_execution_profile?: ExecutionProfile;
  /** Caller-supplied profile override at run-now time, if any. */
  requested_execution_profile?: ExecutionProfile;
  [key: string]: unknown;
}

export type AutomationWebhookDataClass =
  | "public"
  | "internal"
  | "confidential"
  | "restricted"
  | "executive"
  | "credential"
  | "regulated"
  | "customer_data"
  | "source_code"
  | "financial_record";

export type AutomationWebhookRiskTier =
  | "read_discover"
  | "internal_write"
  | "external_draft"
  | "external_send"
  | "customer_data_access"
  | "source_code_mutation"
  | "financial_record_access"
  | "credential_admin"
  | "destructive_delete"
  | "money_movement_contract";

export interface AutomationWebhookSecretStatus {
  configured?: boolean;
  secret_version?: number;
  secretVersion?: number;
  created_at_ms?: number;
  createdAtMs?: number;
  rotated_at_ms?: number | null;
  rotatedAtMs?: number | null;
  rotated_by?: string | null;
  rotatedBy?: string | null;
  [key: string]: unknown;
}

export interface AutomationWebhookDeliveryCounts {
  total: number;
  received?: number;
  accepted?: number;
  rejected?: number;
  duplicate?: number;
  disabled?: number;
  failed?: number;
  [key: string]: unknown;
}

export interface AutomationWebhookProviderVerification {
  signature_scheme?: string;
  signatureScheme?: string;
  provider_specific?: boolean;
  providerSpecific?: boolean;
  [key: string]: unknown;
}

export interface AutomationWebhookProviderPolling {
  supported?: boolean;
  reconciliation_supported?: boolean;
  reconciliationSupported?: boolean;
  [key: string]: unknown;
}

export interface AutomationWebhookProviderMetadata {
  canonical_provider?: string;
  canonicalProvider?: string;
  provider_event_kind?: string | null;
  providerEventKind?: string | null;
  event_id_headers?: string[];
  eventIdHeaders?: string[];
  verification?: AutomationWebhookProviderVerification;
  polling?: AutomationWebhookProviderPolling;
  [key: string]: unknown;
}

export interface AutomationWebhookTrigger {
  trigger_id?: string;
  triggerID?: string;
  automation_id?: string;
  automationID?: string;
  name?: string;
  provider?: string;
  provider_event_kind?: string | null;
  providerEventKind?: string | null;
  provider_metadata?: AutomationWebhookProviderMetadata;
  providerMetadata?: AutomationWebhookProviderMetadata;
  enabled?: boolean;
  callback_path?: string;
  callbackPath?: string;
  callback_url?: string;
  callbackUrl?: string;
  tenant_label?: string;
  tenantLabel?: string;
  owning_org_unit_id?: string | null;
  owningOrgUnitId?: string | null;
  resource_scope?: JsonObject | null;
  resourceScope?: JsonObject | null;
  default_data_class?: AutomationWebhookDataClass | string;
  defaultDataClass?: AutomationWebhookDataClass | string;
  default_risk_tier?: AutomationWebhookRiskTier | string | null;
  defaultRiskTier?: AutomationWebhookRiskTier | string | null;
  signature_scheme?: string;
  signatureScheme?: string;
  secret_status?: AutomationWebhookSecretStatus;
  secretStatus?: AutomationWebhookSecretStatus;
  delivery_counts?: AutomationWebhookDeliveryCounts;
  deliveryCounts?: AutomationWebhookDeliveryCounts;
  last_received_at_ms?: number | null;
  lastReceivedAtMs?: number | null;
  last_accepted_at_ms?: number | null;
  lastAcceptedAtMs?: number | null;
  last_rejected_at_ms?: number | null;
  lastRejectedAtMs?: number | null;
  created_at_ms?: number;
  createdAtMs?: number;
  updated_at_ms?: number;
  updatedAtMs?: number;
  [key: string]: unknown;
}

export interface AutomationWebhookDelivery {
  delivery_id?: string;
  deliveryID?: string;
  trigger_id?: string;
  triggerID?: string;
  automation_id?: string;
  automationID?: string;
  provider_event_id?: string | null;
  providerEventID?: string | null;
  body_digest?: string;
  bodyDigest?: string;
  status?: "received" | "accepted" | "rejected" | "duplicate" | "disabled" | "failed" | string;
  rejection_reason_code?: string | null;
  rejectionReasonCode?: string | null;
  queued_run_id?: string | null;
  queuedRunID?: string | null;
  queued_run_path?: string | null;
  queuedRunPath?: string | null;
  received_at_ms?: number;
  receivedAtMs?: number;
  accepted_at_ms?: number | null;
  acceptedAtMs?: number | null;
  rejected_at_ms?: number | null;
  rejectedAtMs?: number | null;
  sanitized_preview?: JsonValue;
  sanitizedPreview?: JsonValue;
  audit_event_id?: string | null;
  auditEventID?: string | null;
  [key: string]: unknown;
}

export interface AutomationWebhookTriggerCreateInput {
  name?: string;
  provider: string;
  provider_event_kind?: string | null;
  providerEventKind?: string | null;
  enabled?: boolean;
  owning_org_unit_id?: string | null;
  owningOrgUnitId?: string | null;
  resource_scope?: JsonObject | null;
  resourceScope?: JsonObject | null;
  default_data_class?: AutomationWebhookDataClass | string;
  defaultDataClass?: AutomationWebhookDataClass | string;
  default_risk_tier?: AutomationWebhookRiskTier | string | null;
  defaultRiskTier?: AutomationWebhookRiskTier | string | null;
}

export interface AutomationWebhookTriggerUpdateInput {
  name?: string;
  provider?: string;
  provider_event_kind?: string | null;
  providerEventKind?: string | null;
  enabled?: boolean;
  default_data_class?: AutomationWebhookDataClass | string;
  defaultDataClass?: AutomationWebhookDataClass | string;
  default_risk_tier?: AutomationWebhookRiskTier | string | null;
  defaultRiskTier?: AutomationWebhookRiskTier | string | null;
}

export interface AutomationWebhookTriggerListResponse {
  triggers: AutomationWebhookTrigger[];
  count: number;
}

export interface AutomationWebhookTriggerResponse {
  trigger: AutomationWebhookTrigger;
}

export interface AutomationWebhookTriggerSecretResponse extends AutomationWebhookTriggerResponse {
  new_secret?: string;
  newSecret?: string;
  secret_one_time?: boolean;
  secretOneTime?: boolean;
}

export interface AutomationWebhookDeleteResponse {
  ok?: boolean;
  deleted?: boolean;
  trigger_id?: string;
  triggerID?: string;
}

export interface AutomationWebhookDeliveryListResponse {
  deliveries: AutomationWebhookDelivery[];
  count: number;
  limit?: number;
}

export interface AutomationWebhookDeliveryResponse {
  delivery: AutomationWebhookDelivery;
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

/**
 * Canonical envelope stamped on every event by the engine event bus
 * (schema_version 1). See docs/RUNTIME_EVENTS.md in the Tandem repo.
 */
export interface RuntimeEventEnvelope {
  event_id: string;
  seq: number;
  schema_version: number;
  occurred_at_ms: number;
  session_id?: string;
  run_id?: string;
  node_id?: string;
  [key: string]: unknown;
}

export interface EngineEventBase {
  type: string;
  properties: Record<string, unknown>;
  sessionId?: string;
  runId?: string;
  timestamp?: string;
  envelope?: RuntimeEventEnvelope;
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
