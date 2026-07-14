// ─── Core ─────────────────────────────────────────────────────────────────────

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };
export type JsonObject = Record<string, unknown>;

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
  size_bytes?: number;
  modified_at_ms?: number | null;
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
  sessions_merged?: number;
  messages_recovered?: number;
  parts_recovered?: number;
  legacy_counts?: JsonObject;
  imported_counts?: JsonObject;
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
  created_at_ms: number;
  directory?: string;
  workspace_root?: string;
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
  token_masked?: string | null;
  allowed_users?: string[];
  mention_only?: boolean;
  style_profile?: string;
  guild_id?: string;
  channel_id?: string;
  security_profile?: string;
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
  file_or_path?: string;
  location: SkillLocation;
  namespace?: string;
  conflict_policy?: "skip" | "overwrite" | "rename";
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
  schedule_compatibility?: string[];
  has_manifest?: boolean;
  has_workflow?: boolean;
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
  schedule_compatibility?: string[];
  has_manifest?: boolean;
  has_workflow?: boolean;
}

export interface SkillValidationResponse {
  items: SkillValidationItem[];
  total: number;
  valid: number;
  invalid: number;
}

export interface SkillRouterMatch {
  skill_name: string;
  confidence: number;
  reason: string;
}

export interface SkillRouterMatchResponse {
  decision: "match" | "no_match" | string;
  skill_name?: string;
  confidence: number;
  reason: string;
  top_matches?: SkillRouterMatch[];
}

export interface SkillsEvalCaseInput {
  prompt: string;
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
  skill_name: string;
  threshold: number;
  total: number;
  true_positive: number;
  false_negative: number;
  recall: number;
  cases: Array<Record<string, unknown>>;
}

export interface SkillCompileResponse {
  status: string;
  skill_name?: string;
  workflow_kind?: string;
  validation?: Record<string, unknown>;
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

// ─── Workflows ───────────────────────────────────────────────────────────────

export interface WorkflowRecord {
  id?: string;
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
  run_id?: string;
  workflow_id?: string;
  status?: string;
  created_at_ms?: number;
  updated_at_ms?: number;
  [key: string]: unknown;
}

export interface WorkflowRunListResponse {
  runs: WorkflowRunRecord[];
  count: number;
}

export interface WorkflowHookRecord {
  id?: string;
  workflow_id?: string;
  event_type?: string;
  enabled?: boolean;
  [key: string]: unknown;
}

export interface WorkflowHookListResponse {
  hooks: WorkflowHookRecord[];
  count: number;
}

// ─── Incident Monitor ─────────────────────────────────────────────────────────────

export type IncidentMonitorDestinationKind =
  | "github_issue"
  | "linear_issue"
  | "webhook"
  | "telemetry"
  | "mcp_tool"
  | "internal_memory";

export type IncidentMonitorApprovalPolicy = "inherit" | "always" | "high_risk" | "never";
export type IncidentMonitorSourceKind =
  | "tandem_runtime"
  | "tandem_monitor"
  | "external_app"
  | "ci"
  | "agent_runtime"
  | "mcp_gateway"
  | "customer_system";

export interface IncidentMonitorDestinationConfig {
  destination_id: string;
  name: string;
  kind?: IncidentMonitorDestinationKind | string;
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

export interface IncidentMonitorRouteConfig {
  route_id: string;
  name: string;
  enabled?: boolean;
  priority?: number;
  destination_ids?: string[];
  approval_policy?: IncidentMonitorApprovalPolicy | string;
  match_event_types?: string[];
  match_sources?: string[];
  match_components?: string[];
  match_risk_levels?: string[];
  match_risk_categories?: string[];
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

export interface IncidentMonitorSafetyDefaults {
  require_approval_for_high_risk?: boolean;
  redact_secrets?: boolean;
  block_unready_destinations?: boolean;
  retention_days?: number | null;
  [key: string]: unknown;
}

export interface IncidentMonitorDestinationReadiness {
  destination_id: string;
  kind?: IncidentMonitorDestinationKind | string;
  enabled?: boolean;
  ready?: boolean;
  publish_ready?: boolean;
  requires_approval?: boolean;
  missing?: string[];
  detail?: string | null;
  [key: string]: unknown;
}

export interface IncidentMonitorSourceReadinessConfig {
  source_owner?: string | null;
  system_of_record?: string | null;
  data_classification?: string | null;
  allowed_use?: string | null;
  source_of_truth?: string | null;
  lineage_ref?: string | null;
  freshness_sla_ms?: number | null;
  last_observed_at_ms?: number | null;
  expected_schema_version?: string | null;
  schema_drift_status?: string | null;
  quality_notes?: string | null;
  legal_basis?: string | null;
  authorization_marker?: string | null;
  [key: string]: unknown;
}

export interface IncidentMonitorSourceReadinessFinding {
  finding_id: string;
  rule_id: string;
  category: string;
  severity: string;
  title: string;
  detail: string;
  evidence_refs?: string[];
  recommendation: string;
  [key: string]: unknown;
}

export interface IncidentMonitorSourceReadiness {
  project_id: string;
  source_id?: string | null;
  source_kind?: IncidentMonitorSourceKind | string;
  enabled?: boolean;
  ready?: boolean;
  governance_ready?: boolean;
  lineage_ready?: boolean;
  freshness_ready?: boolean;
  schema_ready?: boolean;
  protection_ready?: boolean;
  missing?: string[];
  warnings?: string[];
  findings?: IncidentMonitorSourceReadinessFinding[];
  last_observed_at_ms?: number | null;
  stale_after_ms?: number | null;
  detail?: string | null;
  [key: string]: unknown;
}

export interface IncidentMonitorRoutePreviewMatch {
  route_id?: string | null;
  route_name?: string | null;
  destination_ids?: string[];
  approval_required?: boolean;
  reason?: string | null;
  [key: string]: unknown;
}

export interface IncidentMonitorRoutePreviewResponse {
  matches?: IncidentMonitorRoutePreviewMatch[];
  destinations?: IncidentMonitorDestinationConfig[];
  readiness?: IncidentMonitorDestinationReadiness[];
  source_readiness?: IncidentMonitorSourceReadiness[];
  source_readiness_warnings?: string[];
  default_destination_ids?: string[];
  effective_destination_ids?: string[];
  approval_required?: boolean;
  blocked?: boolean;
  blocked_reasons?: string[];
  [key: string]: unknown;
}

export interface IncidentMonitorConfigRow {
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
  monitored_projects?: IncidentMonitorMonitoredProject[];
  destinations?: IncidentMonitorDestinationConfig[];
  routes?: IncidentMonitorRouteConfig[];
  default_destination_ids?: string[];
  safety_defaults?: IncidentMonitorSafetyDefaults | null;
  [key: string]: unknown;
}

export interface IncidentMonitorLogSource {
  source_id?: string;
  path?: string;
  source_kind?: IncidentMonitorSourceKind | string | null;
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
  approval_policy?: IncidentMonitorApprovalPolicy | string;
  redaction_profile?: string | null;
  retention_profile?: string | null;
  data_readiness?: IncidentMonitorSourceReadinessConfig;
  [key: string]: unknown;
}

export interface IncidentMonitorMonitoredProject {
  project_id?: string;
  name?: string;
  enabled?: boolean;
  paused?: boolean;
  repo?: string;
  workspace_root?: string;
  source_kind?: IncidentMonitorSourceKind | string;
  mcp_server?: string | null;
  model_policy?: JsonObject | null;
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  default_route_tags?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  approval_policy?: IncidentMonitorApprovalPolicy | string;
  redaction_profile?: string | null;
  retention_profile?: string | null;
  data_readiness?: IncidentMonitorSourceReadinessConfig;
  auto_create_new_issues?: boolean;
  require_approval_for_new_issues?: boolean;
  auto_comment_on_matched_open_issues?: boolean;
  log_sources?: IncidentMonitorLogSource[];
  [key: string]: unknown;
}

export interface IncidentMonitorConfigResponse {
  incident_monitor: IncidentMonitorConfigRow;
}

export interface IncidentMonitorStatusRow {
  config?: IncidentMonitorConfigRow;
  readiness?: Record<string, boolean>;
  runtime?: JsonObject;
  required_capabilities?: Record<string, boolean>;
  missing_required_capabilities?: string[];
  resolved_capabilities?: JsonObject[];
  discovered_mcp_tools?: string[];
  selected_server_binding_candidates?: JsonObject[];
  destinations?: IncidentMonitorDestinationConfig[];
  destination_readiness?: IncidentMonitorDestinationReadiness[];
  source_readiness?: IncidentMonitorSourceReadiness[];
  binding_source_version?: string | null;
  bindings_last_merged_at_ms?: number | null;
  selected_model?: JsonObject | null;
  pending_drafts?: number;
  pending_posts?: number;
  last_activity_at_ms?: number | null;
  last_error?: string | null;
  [key: string]: unknown;
}

export interface IncidentMonitorStatusResponse {
  status: IncidentMonitorStatusRow;
}

export interface IncidentMonitorAuthorityInventoryResponse {
  schema_version: number;
  generated_at_ms?: number;
  scope?: JsonObject;
  inventory: IncidentMonitorAuthorityInventory;
  counts?: Record<string, number>;
  sensitive_values?: JsonObject;
  [key: string]: unknown;
}

export interface IncidentMonitorAuthorityInventory {
  workflows?: IncidentMonitorAuthorityWorkflow[];
  automation_specs?: IncidentMonitorAuthorityAutomationSpec[];
  mcp?: JsonObject;
  incident_monitor?: JsonObject;
  destinations?: IncidentMonitorAuthorityDestination[];
  routes?: IncidentMonitorAuthorityRoute[];
  monitored_sources?: IncidentMonitorAuthorityMonitoredSource[];
  scoped_intake_keys?: IncidentMonitorAuthorityIntakeKey[];
  approval_rules?: JsonObject[];
  pending_approvals?: JsonObject[];
  governance_approval_requests?: JsonObject[];
  policy_decisions?: JsonObject[];
  external_publish_surfaces?: JsonObject;
  [key: string]: unknown;
}

export interface IncidentMonitorAuthorityWorkflow {
  workflow_id?: string;
  name?: string;
  enabled?: boolean;
  source?: string | null;
  steps?: JsonObject[];
  hooks?: JsonObject[];
  [key: string]: unknown;
}

export interface IncidentMonitorAuthorityAutomationSpec {
  automation_id?: string;
  name?: string;
  status?: string;
  creator_id?: string | null;
  workspace_root?: string | null;
  tenant_context?: JsonObject;
  agents?: JsonObject[];
  nodes?: JsonObject[];
  [key: string]: unknown;
}

export interface IncidentMonitorAuthorityDestination {
  destination_id?: string;
  name?: string;
  kind?: IncidentMonitorDestinationKind | string | null;
  enabled?: boolean;
  require_approval?: boolean;
  allow_publish?: boolean | null;
  custom_config_keys?: string[];
  [key: string]: unknown;
}

export interface IncidentMonitorAuthorityRoute {
  route_id?: string;
  name?: string;
  enabled?: boolean;
  priority?: number;
  destination_ids?: string[];
  approval_policy?: IncidentMonitorApprovalPolicy | string | null;
  match?: JsonObject;
  [key: string]: unknown;
}

export interface IncidentMonitorAuthorityMonitoredSource {
  project_id?: string;
  source_id?: string | null;
  name?: string;
  enabled?: boolean;
  paused?: boolean;
  source_kind?: IncidentMonitorSourceKind | string | null;
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  approval_policy?: IncidentMonitorApprovalPolicy | string | null;
  [key: string]: unknown;
}

export interface IncidentMonitorAuthorityIntakeKey {
  key_id?: string;
  project_id?: string;
  name?: string;
  enabled?: boolean;
  scopes?: string[];
  key_hash_present?: boolean;
  created_at_ms?: number;
  last_used_at_ms?: number | null;
  [key: string]: unknown;
}

export interface IncidentMonitorPostureChecksOptions {
  mode?: "dry_run" | "enabled" | "disabled" | string;
  rules?: string[];
  disabledRules?: string[];
  disabled_rules?: string[];
  minSeverity?: "low" | "medium" | "high" | "critical" | string;
  min_severity?: "low" | "medium" | "high" | "critical" | string;
}

export interface IncidentMonitorPostureChecksResponse {
  schema_version: number;
  generated_at_ms?: number;
  scope?: JsonObject;
  baseline_policy?: JsonObject;
  rules?: IncidentMonitorPostureRuleState[];
  findings: IncidentMonitorPostureFinding[];
  counts?: IncidentMonitorPostureCounts;
  authority_inventory?: JsonObject;
  draft_conversion?: JsonObject;
  sensitive_values?: JsonObject;
  [key: string]: unknown;
}

export interface IncidentMonitorPostureRuleState {
  rule_id: string;
  category?: string;
  default_severity?: string;
  enabled?: boolean;
  dry_run?: boolean;
  [key: string]: unknown;
}

export interface IncidentMonitorPostureCounts {
  findings?: number;
  by_severity?: Record<string, number>;
  by_category?: Record<string, number>;
  active_rules?: number;
  [key: string]: unknown;
}

export interface IncidentMonitorPostureFinding {
  finding_id: string;
  fingerprint: string;
  rule_id: string;
  category: string;
  severity: "low" | "medium" | "high" | "critical" | string;
  title: string;
  detail?: string;
  affected_objects?: JsonObject[];
  evidence_refs?: JsonObject[];
  recommendation?: string;
  route_suggestion?: JsonObject;
  destination_suggestion?: JsonObject;
  incident_draft_suggestion?: JsonObject;
  dry_run?: boolean;
  [key: string]: unknown;
}

export interface IncidentMonitorAssessmentProbeRunInput {
  mode?: "dry_run" | "disabled" | string;
  probes?: string[];
  include_draft_suggestions?: boolean;
  includeDraftSuggestions?: boolean;
  [key: string]: unknown;
}

export interface IncidentMonitorAssessmentProbeRunResponse {
  schema_version: number;
  generated_at_ms?: number;
  scope?: JsonObject;
  probe_policy?: JsonObject;
  counts?: IncidentMonitorAssessmentProbeCounts;
  results: IncidentMonitorAssessmentProbeResult[];
  authority_inventory?: JsonObject;
  evidence_pack?: JsonObject;
  draft_conversion?: JsonObject;
  sensitive_values?: JsonObject;
  [key: string]: unknown;
}

export interface IncidentMonitorAssessmentProbeCounts {
  results?: number;
  pass?: number;
  fail?: number;
  blocked?: number;
  by_status?: Record<string, number>;
  draft_suggestions?: number;
  [key: string]: unknown;
}

export interface IncidentMonitorAssessmentProbeResult {
  probe_id: string;
  target?: JsonObject;
  expected_behavior?: string;
  observed_behavior?: string;
  status: "pass" | "fail" | "blocked" | string;
  passed?: boolean;
  blocked?: boolean;
  fingerprint?: string;
  finding_id?: string | null;
  evidence_refs?: JsonObject[];
  source_finding?: JsonObject | null;
  incident_draft_suggestion?: JsonObject | null;
  dry_run?: boolean;
  [key: string]: unknown;
}

export interface IncidentMonitorAssessmentReportInput {
  from_ms?: number;
  to_ms?: number;
  source_kind?: IncidentMonitorSourceKind | string;
  min_severity?: "low" | "medium" | "high" | "critical" | string;
  probes?: string[];
  include_probe_results?: boolean;
  includeProbeResults?: boolean;
  include_draft_suggestions?: boolean;
  includeDraftSuggestions?: boolean;
  persist_artifact?: boolean;
  persistArtifact?: boolean;
  route_destination_ids?: string[];
  routeDestinationIds?: string[];
  include_raw_payloads?: boolean;
  includeRawPayloads?: boolean;
  [key: string]: unknown;
}

export interface IncidentMonitorAssessmentReportResponse {
  schema_version: number;
  generated_at_ms?: number;
  scope?: JsonObject;
  counts?: JsonObject;
  sections?: {
    assessment_scope?: JsonObject;
    monitored_systems_and_sources?: JsonObject;
    authority_inventory?: JsonObject;
    approval_coverage?: JsonObject;
    tenant_workspace_context_coverage?: JsonObject;
    detected_findings?: JsonObject;
    controlled_probe_results?: JsonObject;
    incidents_and_evidence_refs?: JsonObject;
    destination_receipts?: JsonObject;
    recommended_mitigations?: JsonObject[];
    residual_risk_and_follow_up?: JsonObject;
    self_monitoring_boundary?: JsonObject;
    external_audit_export?: JsonObject;
    [key: string]: unknown;
  };
  markdown_summary?: string;
  evidence_pack?: JsonObject;
  sensitive_values?: JsonObject;
  [key: string]: unknown;
}

export interface IncidentMonitorDeploymentCardMetadata {
  intended_purpose?: string;
  purpose?: string;
  business_owner?: string;
  accountable_team?: string;
  autonomy_level?: string;
  allowed_actions?: string[];
  prohibited_actions?: string[];
  data_classification?: string;
  approval_protocol?: string;
  escalation_protocol?: string;
  known_limitations?: string[];
  residual_risks?: string[];
  review_cadence_days?: number;
  next_reassessment_at_ms?: number;
  evidence_refs?: string[];
  [key: string]: unknown;
}

export interface IncidentMonitorDeploymentCardsInput {
  defaults?: IncidentMonitorDeploymentCardMetadata;
  metadata?: Record<string, IncidentMonitorDeploymentCardMetadata>;
  include_markdown?: boolean;
  includeMarkdown?: boolean;
  include_raw_inventory?: boolean;
  includeRawInventory?: boolean;
  [key: string]: unknown;
}

export interface IncidentMonitorDeploymentCardRecord {
  card_id: string;
  card_kind: string;
  target?: JsonObject;
  system_boundary?: string;
  source_kind?: IncidentMonitorSourceKind | string | null;
  intended_purpose?: string | null;
  business_owner?: string | null;
  accountable_team?: string | null;
  autonomy_level?: string | null;
  allowed_actions?: string[];
  prohibited_actions?: string[];
  authority?: JsonObject;
  data?: JsonObject;
  approval_and_escalation?: JsonObject;
  known_limitations?: string[];
  residual_risks?: string[];
  review?: JsonObject;
  linked_evidence?: JsonObject;
  [key: string]: unknown;
}

export interface IncidentMonitorDeploymentCardsResponse {
  schema_version: number;
  generated_at_ms?: number;
  scope?: JsonObject;
  card_policy?: JsonObject;
  counts?: JsonObject;
  cards: IncidentMonitorDeploymentCardRecord[];
  findings: JsonObject[];
  markdown_export?: string | null;
  authority_inventory?: JsonObject;
  sensitive_values?: JsonObject;
  [key: string]: unknown;
}

export interface IncidentMonitorIncidentRecord {
  incident_id: string;
  fingerprint?: string;
  event_type?: string;
  status?: string;
  repo?: string;
  workspace_root?: string;
  title?: string;
  project_id?: string | null;
  log_source_id?: string | null;
  source_kind?: IncidentMonitorSourceKind | string | null;
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
  risk_category?: string | null;
  actor?: string | null;
  model?: string | null;
  tool_name?: string | null;
  action?: string | null;
  policy?: string | null;
  approval_state?: string | null;
  blast_radius?: string | null;
  external_correlation_ids?: string[];
  expected_destination?: string | null;
  route_tags?: string[];
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  source_approval_policy?: IncidentMonitorApprovalPolicy | string | null;
  redaction_profile?: string | null;
  retention_profile?: string | null;
  evidence_refs?: string[];
  quality_gate?: JsonObject | null;
  [key: string]: unknown;
}

export interface IncidentMonitorIncidentListResponse {
  incidents: IncidentMonitorIncidentRecord[];
  count: number;
}

export interface IncidentMonitorDraftRecord {
  draft_id: string;
  fingerprint?: string;
  repo?: string;
  project_id?: string | null;
  log_source_id?: string | null;
  source_kind?: IncidentMonitorSourceKind | string | null;
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
  risk_category?: string | null;
  actor?: string | null;
  model?: string | null;
  tool_name?: string | null;
  action?: string | null;
  policy?: string | null;
  approval_state?: string | null;
  blast_radius?: string | null;
  external_correlation_ids?: string[];
  expected_destination?: string | null;
  route_tags?: string[];
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  source_approval_policy?: IncidentMonitorApprovalPolicy | string | null;
  redaction_profile?: string | null;
  retention_profile?: string | null;
  evidence_refs?: string[];
  quality_gate?: JsonObject | null;
  last_post_error?: string | null;
  [key: string]: unknown;
}

export interface IncidentMonitorDraftListResponse {
  drafts: IncidentMonitorDraftRecord[];
  count: number;
}

export interface IncidentMonitorPostRecord {
  post_id: string;
  draft_id?: string;
  repo?: string;
  operation?: string;
  status?: string;
  incident_id?: string | null;
  fingerprint?: string;
  issue_number?: number | null;
  issue_url?: string | null;
  comment_id?: string | null;
  comment_url?: string | null;
  destination_id?: string | null;
  destination_kind?: IncidentMonitorDestinationKind | string | null;
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

export interface IncidentMonitorPostListResponse {
  posts: IncidentMonitorPostRecord[];
  count: number;
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
  active_session_ids?: string[];
  latest_session_id?: string | null;
  attach_event_stream?: string | null;
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

// ─── Coder ───────────────────────────────────────────────────────────────────

export interface CoderRepoBinding {
  project_id?: string;
  workspace_id?: string;
  workspace_root?: string;
  repo_slug: string;
  default_branch?: string | null;
}

export interface CoderGithubRef {
  kind: "issue" | "pull_request" | string;
  number: number;
  url?: string | null;
}

export interface CoderRunRecord {
  coder_run_id?: string;
  workflow_mode?: string;
  linked_context_run_id?: string;
  repo_binding?: CoderRepoBinding;
  github_ref?: CoderGithubRef | null;
  source_client?: string | null;
  status?: string;
  phase?: string;
  created_at_ms?: number;
  updated_at_ms?: number;
  [key: string]: unknown;
}

export interface CoderRunsListResponse {
  runs: CoderRunRecord[];
  count?: number;
}

export interface CoderRunGetResponse {
  coder_run?: CoderRunRecord;
  run?: JsonObject;
  [key: string]: unknown;
}

export interface CoderArtifactRecord {
  id: string;
  ts_ms?: number;
  path: string;
  artifact_type?: string;
  step_id?: string | null;
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
  candidate_id?: string;
  kind?: string;
  summary?: string | null;
  payload?: JsonObject;
  artifact?: CoderArtifactRecord | null;
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

export type AutomationWaitTimeoutAction = "cancel" | "escalate" | "remind" | "resume";

export interface AutomationWaitTimeoutPolicy {
  expires_after_ms: number;
  on_timeout: AutomationWaitTimeoutAction;
  escalate_to?: string;
  remind_every_ms?: number;
}

export type AutomationWaitValueBinding =
  | { source: "literal"; value: JsonValue }
  | {
      source: "node_output";
      node_id: string;
      json_pointer?: string;
    };

export type AutomationWaitSpec =
  | {
      kind: "timer";
      delay_ms?: number;
      wake_at?: AutomationWaitValueBinding;
      timeout?: AutomationWaitTimeoutPolicy;
    }
  | {
      kind: "approval";
      decisions: string[];
      expires_after_ms?: number;
      timeout?: AutomationWaitTimeoutPolicy;
    }
  | {
      kind: "webhook";
      trigger_id: string;
      provider?: string;
      provider_event_kind?: string;
      correlation: {
        field: "provider_event_id" | "idempotency_key" | "body_digest";
        value: AutomationWaitValueBinding;
      };
      timeout: AutomationWaitTimeoutPolicy;
    }
  | {
      kind: "external_condition";
      condition_key: AutomationWaitValueBinding;
      timeout: AutomationWaitTimeoutPolicy;
      payload_schema?: JsonObject;
    };

export interface AutomationV2FlowNode {
  node_id: string;
  agent_id: string;
  objective: string;
  depends_on?: string[];
  input_refs?: Array<{ from_step_id?: string; alias: string }>;
  output_contract?: { kind: string };
  retry_policy?: JsonObject;
  timeout_ms?: number;
  wait?: AutomationWaitSpec;
}

export type KnowledgeScope = "run" | "project" | "global";
export type KnowledgeTrustLevel = "working" | "promoted" | "approved_default";
export type KnowledgeReuseMode = "disabled" | "preflight" | "on_demand";

export interface KnowledgeSpaceRef {
  scope?: KnowledgeScope;
  project_id?: string;
  namespace?: string;
  space_id?: string;
  [key: string]: unknown;
}

export interface KnowledgeBinding {
  enabled?: boolean;
  reuse_mode?: KnowledgeReuseMode;
  trust_floor?: KnowledgeTrustLevel;
  read_spaces?: KnowledgeSpaceRef[];
  promote_spaces?: KnowledgeSpaceRef[];
  namespace?: string;
  subject?: string;
  freshness_ms?: number;
  [key: string]: unknown;
}

export interface AutomationV2Spec {
  automation_id?: string;
  name: string;
  description?: string;
  status?: AutomationV2Status;
  schedule: AutomationV2Schedule;
  knowledge?: KnowledgeBinding;
  agents: AutomationV2AgentProfile[];
  flow: { nodes: AutomationV2FlowNode[] };
  execution?: {
    max_parallel_agents?: number;
    max_total_runtime_ms?: number;
    max_total_tool_calls?: number;
  };
  output_targets?: string[];
  creator_id?: string;
  workspace_root?: string;
  metadata?: JsonObject;
  [key: string]: unknown;
}

export interface WorkflowPlanStep {
  step_id?: string;
  kind: string;
  objective: string;
  depends_on?: string[];
  agent_role?: string;
  input_refs?: Array<{ from_step_id?: string; alias: string }>;
  output_contract?: { kind: string };
}

export interface WorkflowPlan {
  plan_id?: string;
  planner_version?: string;
  plan_source?: string;
  original_prompt?: string;
  normalized_prompt?: string;
  confidence?: string;
  title: string;
  description?: string;
  schedule: AutomationV2Schedule;
  execution_target?: string;
  workspace_root?: string;
  steps: WorkflowPlanStep[];
  allowed_mcp_servers?: string[];
  operator_preferences?: JsonObject;
  [key: string]: unknown;
}

export interface WorkflowPlanPackBuilderExportRequest {
  enabled?: boolean;
  session_id?: string;
  thread_key?: string;
  auto_apply?: boolean;
}

export interface WorkflowPlanPackBuilderExportResult {
  status?: string;
  error?: string;
  http_status?: number;
  plan_id?: string;
  session_id?: string;
  thread_key?: string;
  auto_apply_requested?: boolean;
  auto_apply_ready?: boolean;
  [key: string]: unknown;
}

export interface WorkflowPlanChatMessage {
  role: string;
  text: string;
  created_at_ms?: number;
  [key: string]: unknown;
}

export interface WorkflowPlanConversation {
  conversation_id?: string;
  plan_id?: string;
  created_at_ms?: number;
  updated_at_ms?: number;
  messages: WorkflowPlanChatMessage[];
  [key: string]: unknown;
}

export interface WorkflowPlanDraftReviewRecord {
  required_capabilities?: string[];
  requested_capabilities?: string[];
  blocked_capabilities?: string[];
  docs_mcp_used?: boolean;
  preview_payload?: JsonObject | null;
  created_at_ms?: number | null;
  updated_at_ms?: number | null;
  validation_state?: string;
  validation_status?: string;
  approval_status?: string;
  [key: string]: unknown;
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

export interface WorkflowPlannerArtifactLink {
  link_id: string;
  kind: string;
  resource_id: string;
  resource_url: string;
  revision?: number;
  chat_run_id?: string;
  tool_name?: string;
  tool_call_id?: string;
  idempotency_key?: string;
  linked_at_ms: number;
}

export interface WorkflowPlannerSessionRecord {
  session_id: string;
  linked_chat_session_id?: string;
  linked_chat_run_id?: string;
  last_referenced_at_ms?: number;
  artifact_links?: WorkflowPlannerArtifactLink[];
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
  operation?: WorkflowPlannerSessionOperationRecord | null;
  published_at_ms?: number | null;
  published_tasks?: JsonValue[];
  created_at_ms: number;
  updated_at_ms: number;
  [key: string]: unknown;
}

export interface WorkflowPlannerSessionListItem {
  session_id: string;
  linked_chat_session_id?: string;
  linked_chat_run_id?: string;
  last_referenced_at_ms?: number;
  title: string;
  project_slug: string;
  workspace_root: string;
  current_plan_id?: string;
  plan_revision?: number;
  published_at_ms?: number;
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

export interface AutomationV2RunRecord {
  run_id: string;
  automation_id: string;
  status: AutomationV2RunStatus;
  checkpoint?: JsonObject;
  active_session_ids?: string[];
  active_instance_ids?: string[];
  workflow_definition_version?: string;
  workflow_definition_snapshot_hash?: string;
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
  goal_seq?: number;
  sessionID?: string;
  runID?: string;
  timestamp?: string;
  [key: string]: unknown;
}
