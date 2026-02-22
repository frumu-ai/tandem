// Tauri API wrapper functions
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ============================================================================
// Utility Functions
// ============================================================================

export async function logFrontendError(message: string, details?: string): Promise<void> {
  return invoke("log_frontend_error", { message, details });
}

// ============================================================================
// Provider Configuration Types
// ============================================================================

export interface ProviderConfig {
  enabled: boolean;
  default: boolean;
  endpoint: string;
  model?: string;
  has_key: boolean;
}

export interface SelectedModel {
  provider_id: string;
  model_id: string;
}

export interface ProvidersConfig {
  openrouter: ProviderConfig;
  opencode_zen: ProviderConfig;
  anthropic: ProviderConfig;
  openai: ProviderConfig;
  ollama: ProviderConfig;
  poe: ProviderConfig;
  custom: ProviderConfig[];
  selected_model?: SelectedModel | null;
}

export interface AppStateInfo {
  workspace_path: string | null;
  has_workspace: boolean;
  user_projects: UserProject[];
  active_project_id: string | null;
  providers_config: ProvidersConfig;
}

export interface UserProject {
  id: string;
  name: string;
  path: string;
  created_at: string;
  last_accessed: string;
}

// API Key types
export type ApiKeyType =
  | "openrouter"
  | "opencode_zen"
  | "anthropic"
  | "openai"
  | "ollama"
  | "poe"
  | string;

// ============================================================================
// Sidecar Types
// ============================================================================

export type SidecarState = "stopped" | "starting" | "running" | "stopping" | "failed";

export interface SidecarStatus {
  installed: boolean;
  version: string | null;
  latestVersion: string | null;
  latestOverallVersion: string | null;
  updateAvailable: boolean;
  compatibilityMessage: string | null;
  binaryPath: string | null;
}

export interface SidecarStartupHealth {
  healthy: boolean;
  ready: boolean;
  phase: string;
  startup_attempt_id: string;
  startup_elapsed_ms: number;
  last_error?: string | null;
}

export interface SessionTime {
  created: number;
  updated: number;
}

export interface SessionSummary {
  additions: number;
  deletions: number;
  files: number;
}

export interface Session {
  id: string;
  slug?: string;
  version?: string;
  projectID?: string;
  directory?: string;
  title?: string;
  time?: SessionTime;
  summary?: SessionSummary;
  // Legacy fields
  model?: string;
  provider?: string;
  messages: Message[];
}

export interface Project {
  id: string;
  worktree: string;
  vcs?: string;
  sandboxes: unknown[];
  time: {
    created: number;
    updated: number;
  };
}

export interface MessageInfo {
  id: string;
  sessionID: string;
  role: string;
  time: {
    created: number;
    completed?: number;
  };
  summary?: {
    title?: string;
    diffs: unknown[];
  };
  agent?: string;
  model?: unknown;
  deleted?: boolean;
  reverted?: boolean;
}

export interface SessionMessage {
  info: MessageInfo;
  parts: unknown[];
}

const EPOCH_MS_THRESHOLD = 1_000_000_000_000;

function normalizeEpochMs(value: number | undefined | null): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return Date.now();
  }
  return value < EPOCH_MS_THRESHOLD ? value * 1000 : value;
}

function normalizeSessionTime(time?: SessionTime): SessionTime | undefined {
  if (!time) return undefined;
  return {
    created: normalizeEpochMs(time.created),
    updated: normalizeEpochMs(time.updated),
  };
}

function normalizeSession(session: Session): Session {
  return {
    ...session,
    time: normalizeSessionTime(session.time),
  };
}

function normalizeSessionMessage(message: SessionMessage): SessionMessage {
  return {
    ...message,
    info: {
      ...message.info,
      time: {
        ...message.info.time,
        created: normalizeEpochMs(message.info.time?.created),
        completed:
          message.info.time?.completed === undefined
            ? undefined
            : normalizeEpochMs(message.info.time.completed),
      },
    },
  };
}

export interface StorageStatus {
  canonical_root: string;
  legacy_root: string;
  migration_report_exists: boolean;
  storage_version_exists: boolean;
  migration_reason?: string | null;
  migration_timestamp_ms?: number | null;
}

export interface StorageMigrationSource {
  id: string;
  path: string;
  exists: boolean;
}

export interface StorageMigrationStatus {
  canonical_root: string;
  migration_report_exists: boolean;
  storage_version_exists: boolean;
  migration_reason?: string | null;
  migration_timestamp_ms?: number | null;
  migration_needed: boolean;
  sources_detected: StorageMigrationSource[];
}

export interface StorageMigrationOptions {
  dryRun?: boolean;
  force?: boolean;
  includeWorkspaceScan?: boolean;
}

export interface StorageMigrationProgressEvent {
  phase: string;
  phase_percent: number;
  overall_percent: number;
  sessions_imported: number;
  sessions_repaired: number;
  messages_recovered: number;
  parts_recovered: number;
  conflicts_merged: number;
  copied_count: number;
  skipped_count: number;
  error_count: number;
}

export interface StorageMigrationRunResult {
  status: "success" | "partial" | "failed";
  started_at_ms: number;
  ended_at_ms: number;
  duration_ms: number;
  sources_detected: StorageMigrationSource[];
  copied: string[];
  skipped: string[];
  errors: string[];
  sessions_imported: number;
  sessions_repaired: number;
  messages_recovered: number;
  parts_recovered: number;
  conflicts_merged: number;
  tool_rows_upserted: number;
  report_path: string;
  reason: string;
  dry_run: boolean;
}

export interface ToolHistoryBackfillResult {
  sessions_scanned: number;
  tool_rows_upserted: number;
}

export interface ToolHistoryBackfillStatus {
  tool_rows_total: number;
  sessions_with_tool_rows: number;
}

export interface SidecarCircuitSnapshot {
  state: "closed" | "open" | "half_open" | string;
  failure_count: number;
  last_failure_age_ms?: number;
}

export interface SidecarRuntimeSnapshot {
  state: SidecarState;
  shared_mode: boolean;
  owns_process: boolean;
  port?: number;
  pid?: number;
  binary_path?: string;
  circuit: SidecarCircuitSnapshot;
}

export interface StreamRuntimeSnapshot {
  running: boolean;
  health: "healthy" | "degraded" | "recovering";
  health_reason?: string;
  sequence: number;
  last_event_ts_ms?: number;
  last_health_change_ts_ms: number;
}

export interface RuntimeDiagnostics {
  sidecar: SidecarRuntimeSnapshot;
  stream: StreamRuntimeSnapshot;
  lease_count: number;
  logging: {
    initialized: boolean;
    process: string;
    active_files: string[];
    last_write_ts_ms?: number;
    dropped_events: number;
  };
}

export interface EngineApiTokenInfo {
  token_masked: string;
  token?: string | null;
  path: string;
  storage_backend?: string;
}

export interface EngineLeaseInfo {
  lease_id: string;
  client_id: string;
  client_type: string;
  acquired_at_ms: number;
  last_renewed_at_ms: number;
  ttl_ms: number;
}

export interface ToolExecutionRow {
  id: string;
  session_id: string;
  message_id?: string;
  part_id?: string;
  correlation_id?: string;
  tool: string;
  status: "pending" | "running" | "completed" | "failed";
  args?: unknown;
  result?: unknown;
  error?: string;
  started_at_ms: number;
  ended_at_ms?: number;
}

export interface ActiveRunStatusResponse {
  run_id: string;
  started_at_ms: number;
  last_activity_at_ms: number;
  client_id?: string;
}

export interface FileAttachment {
  id: string;
  type: "image" | "file";
  name: string;
  mime: string;
  url: string;
  size: number;
  preview?: string;
}

export interface Message {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  tool_calls?: ToolCall[];
  created_at?: string;
}

export interface ToolCall {
  id: string;
  tool: string;
  args: Record<string, unknown>;
  result?: unknown;
  status?: "pending" | "running" | "completed" | "failed";
}

export interface TodoItem {
  id: string;
  content: string;
  status: "pending" | "in_progress" | "completed" | "cancelled";
}

export interface QuestionChoice {
  label: string;
  description: string;
}

export interface QuestionInfo {
  header: string;
  question: string;
  options: QuestionChoice[];
  multiple?: boolean;
  custom?: boolean;
}

export interface QuestionRequestEvent {
  session_id: string;
  request_id: string;
  questions: QuestionInfo[];
  tool_call_id?: string;
  tool_message_id?: string;
}

export interface FileEntry {
  name: string;
  path: string;
  is_directory: boolean;
  size?: number;
  extension?: string;
}

export interface ModelInfo {
  id: string;
  name: string;
  provider?: string;
  context_length?: number;
}

export interface ProviderInfo {
  id: string;
  name: string;
  models: string[];
  configured: boolean;
}

export type ModeBase = "immediate" | "plan" | "orchestrate" | "coder" | "ask" | "explore";
export type ModeSource = "builtin" | "user" | "project";
export type ModeScope = "user" | "project";

export interface ModeDefinition {
  id: string;
  label: string;
  base_mode: ModeBase;
  icon?: string;
  system_prompt_append?: string;
  allowed_tools?: string[];
  edit_globs?: string[];
  auto_approve?: boolean;
  source?: ModeSource;
}

// Stream event types from OpenCode (matches Rust StreamEvent enum)
export type StreamEvent =
  | { type: "content"; session_id: string; message_id: string; content: string; delta?: string }
  | {
      type: "tool_start";
      session_id: string;
      message_id: string;
      part_id: string;
      tool: string;
      args: Record<string, unknown>;
    }
  | {
      type: "tool_end";
      session_id: string;
      message_id: string;
      part_id: string;
      tool: string;
      result?: unknown;
      error?: string;
    }
  | { type: "session_status"; session_id: string; status: string }
  | {
      type: "run_started";
      session_id: string;
      run_id: string;
      started_at_ms: number;
      client_id?: string;
    }
  | {
      type: "run_finished";
      session_id: string;
      run_id: string;
      finished_at_ms: number;
      status: "completed" | "cancelled" | "error" | "timeout" | string;
      error?: string;
    }
  | {
      type: "run_conflict";
      session_id: string;
      run_id: string;
      retry_after_ms: number;
      attach_event_stream: string;
    }
  | { type: "session_idle"; session_id: string }
  | { type: "session_error"; session_id: string; error: string }
  | {
      type: "permission_asked";
      session_id: string;
      request_id: string;
      tool?: string;
      args?: Record<string, unknown>;
    }
  | {
      type: "question_asked";
      session_id: string;
      request_id: string;
      questions: QuestionInfo[];
      tool_call_id?: string;
      tool_message_id?: string;
    }
  | {
      type: "todo_updated";
      session_id: string;
      todos: TodoItem[];
    }
  | {
      type: "file_edited";
      session_id: string;
      file_path: string;
    }
  | {
      type: "memory_retrieval";
      session_id: string;
      status?:
        | "not_attempted"
        | "attempted_no_hits"
        | "retrieved_used"
        | "degraded_disabled"
        | "error_fallback";
      used: boolean;
      chunks_total: number;
      session_chunks: number;
      history_chunks: number;
      project_fact_chunks: number;
      latency_ms: number;
      query_hash: string;
      score_min?: number;
      score_max?: number;
      embedding_status?: string;
      embedding_reason?: string;
    }
  | {
      type: "memory_storage";
      session_id: string;
      message_id?: string;
      role: "user" | "assistant" | string;
      session_chunks_stored: number;
      project_chunks_stored: number;
      status?: "ok" | "error" | string;
      error?: string;
    }
  | { type: "raw"; event_type: string; data: unknown };

export type StreamEventSource = "sidecar" | "memory" | "system";

export interface StreamEventEnvelopeV2 {
  event_id: string;
  correlation_id: string;
  ts_ms: number;
  session_id?: string | null;
  source: StreamEventSource;
  payload: StreamEvent;
}

// ============================================================================
// Vault (PIN) Commands
// ============================================================================

export type VaultStatus = "not_created" | "locked" | "unlocked";

export async function getVaultStatus(): Promise<VaultStatus> {
  return invoke("get_vault_status");
}

export async function createVault(pin: string): Promise<void> {
  return invoke("create_vault", { pin });
}

export async function unlockVault(pin: string): Promise<void> {
  return invoke("unlock_vault", { pin });
}

export async function lockVault(): Promise<void> {
  return invoke("lock_vault");
}

// ============================================================================
// Basic Commands
// ============================================================================

export async function greet(name: string): Promise<string> {
  return invoke("greet", { name });
}

export async function getAppState(): Promise<AppStateInfo> {
  return invoke("get_app_state");
}

export async function getStorageStatus(): Promise<StorageStatus> {
  return invoke("get_storage_status");
}

export async function getStorageMigrationStatus(): Promise<StorageMigrationStatus> {
  return invoke("get_storage_migration_status");
}

export async function runStorageMigration(
  options?: StorageMigrationOptions
): Promise<StorageMigrationRunResult> {
  return invoke("run_storage_migration", { options });
}

export async function runToolHistoryBackfill(): Promise<ToolHistoryBackfillResult> {
  return invoke("run_tool_history_backfill");
}

export async function getToolHistoryBackfillStatus(): Promise<ToolHistoryBackfillStatus> {
  return invoke("get_tool_history_backfill_status");
}

export async function setWorkspacePath(path: string): Promise<void> {
  return invoke("set_workspace_path", { path });
}

export async function getWorkspacePath(): Promise<string | null> {
  return invoke("get_workspace_path");
}

// ============================================================================
// Project Management
// ============================================================================

export async function isGitRepo(path: string): Promise<boolean> {
  return invoke("is_git_repo", { path });
}

export interface GitStatus {
  git_installed: boolean;
  is_repo: boolean;
  can_enable_undo: boolean;
}

export async function isGitInstalled(): Promise<boolean> {
  return invoke("is_git_installed");
}

export async function initializeGitRepo(path: string): Promise<void> {
  return invoke("initialize_git_repo", { path });
}

export async function checkGitStatus(path: string): Promise<GitStatus> {
  return invoke("check_git_status", { path });
}

export async function addProject(path: string, name?: string): Promise<UserProject> {
  return invoke("add_project", { path, name });
}

export async function removeProject(projectId: string): Promise<void> {
  return invoke("remove_project", { projectId });
}

export async function getUserProjects(): Promise<UserProject[]> {
  return invoke("get_user_projects");
}

export async function setActiveProject(projectId: string): Promise<void> {
  return invoke("set_active_project", { projectId });
}

export async function getActiveProject(): Promise<UserProject | null> {
  return invoke("get_active_project");
}

// ============================================================================
// API Key Management
// ============================================================================

export async function storeApiKey(keyType: ApiKeyType, apiKey: string): Promise<void> {
  return invoke("store_api_key", { keyType, apiKey });
}

export async function hasApiKey(keyType: ApiKeyType): Promise<boolean> {
  return invoke("has_api_key", { keyType });
}

export async function deleteApiKey(keyType: ApiKeyType): Promise<void> {
  return invoke("delete_api_key", { keyType });
}

// ============================================================================
// Theme / Appearance
// ============================================================================

export async function getUserTheme(): Promise<string> {
  return invoke("get_user_theme");
}

export async function setUserTheme(themeId: string): Promise<void> {
  return invoke("set_user_theme", { themeId });
}

export async function getLanguageSetting(): Promise<string> {
  return invoke("get_language_setting");
}

export async function setLanguageSetting(language: string): Promise<void> {
  return invoke("set_language_setting", { language });
}

export type CustomBackgroundFit = "cover" | "contain" | "tile";

export interface CustomBackgroundSettings {
  enabled: boolean;
  file_name: string | null;
  fit: CustomBackgroundFit;
  opacity: number; // 0..1
}

export interface CustomBackgroundInfo {
  settings: CustomBackgroundSettings;
  file_path: string | null;
}

export async function getCustomBackground(): Promise<CustomBackgroundInfo> {
  return invoke("get_custom_background");
}

export async function setCustomBackgroundImage(sourcePath: string): Promise<CustomBackgroundInfo> {
  return invoke("set_custom_background_image", { sourcePath });
}

export async function setCustomBackgroundImageBytes(
  fileName: string,
  bytes: number[] | Uint8Array
): Promise<CustomBackgroundInfo> {
  // Tauri supports Uint8Array, but number[] works everywhere.
  return invoke("set_custom_background_image_bytes", { fileName, bytes });
}

export async function setCustomBackgroundSettings(
  settings: CustomBackgroundSettings
): Promise<void> {
  return invoke("set_custom_background_settings", { settings });
}

export async function clearCustomBackgroundImage(): Promise<void> {
  return invoke("clear_custom_background_image");
}

// ============================================================================
// Provider Configuration
// ============================================================================

export async function getProvidersConfig(): Promise<ProvidersConfig> {
  return invoke("get_providers_config");
}

export async function setProvidersConfig(config: ProvidersConfig): Promise<void> {
  return invoke("set_providers_config", { config });
}

// ============================================================================
// Channel Connections (Telegram / Discord / Slack)
// ============================================================================

export type ChannelName = "telegram" | "discord" | "slack";

export interface ChannelRuntimeStatus {
  enabled: boolean;
  connected: boolean;
  last_error?: string | null;
  active_sessions: number;
}

export interface ChannelConnectionConfigView {
  has_token: boolean;
  allowed_users: string[];
  mention_only?: boolean | null;
  guild_id?: string | null;
  channel_id?: string | null;
}

export interface ChannelConnectionView {
  status: ChannelRuntimeStatus;
  config: ChannelConnectionConfigView;
}

export interface ChannelConnectionsView {
  telegram: ChannelConnectionView;
  discord: ChannelConnectionView;
  slack: ChannelConnectionView;
}

export interface ChannelConnectionInput {
  token?: string;
  allowed_users?: string[];
  mention_only?: boolean;
  guild_id?: string | null;
  channel_id?: string | null;
}

export async function getChannelConnections(): Promise<ChannelConnectionsView> {
  return invoke("get_channel_connections");
}

export async function setChannelConnection(
  channel: ChannelName,
  input: ChannelConnectionInput
): Promise<ChannelConnectionsView> {
  return invoke("set_channel_connection", { channel, input });
}

export async function disableChannelConnection(
  channel: ChannelName
): Promise<ChannelConnectionsView> {
  return invoke("disable_channel_connection", { channel });
}

export async function deleteChannelConnectionToken(
  channel: ChannelName
): Promise<ChannelConnectionsView> {
  return invoke("delete_channel_connection_token", { channel });
}

export interface McpToolCacheEntry {
  tool_name: string;
  description: string;
  input_schema: Record<string, unknown>;
  fetched_at_ms: number;
  schema_hash: string;
}

export interface McpServerRecord {
  name: string;
  transport: string;
  enabled: boolean;
  connected: boolean;
  pid?: number | null;
  last_error?: string | null;
  headers: Record<string, string>;
  tool_cache: McpToolCacheEntry[];
  tools_fetched_at_ms?: number | null;
}

export interface McpRemoteTool {
  server_name: string;
  tool_name: string;
  namespaced_name: string;
  description: string;
  input_schema: Record<string, unknown>;
  fetched_at_ms: number;
  schema_hash: string;
}

export interface McpAddRequest {
  name: string;
  transport: string;
  headers?: Record<string, string>;
  enabled?: boolean;
}

export interface McpActionResponse {
  ok: boolean;
  error?: string | null;
  count?: number | null;
}

export async function mcpListServers(): Promise<McpServerRecord[]> {
  return invoke("mcp_list_servers");
}

export async function mcpAddServer(request: McpAddRequest): Promise<McpActionResponse> {
  return invoke("mcp_add_server", { request });
}

export async function mcpSetEnabled(name: string, enabled: boolean): Promise<McpActionResponse> {
  return invoke("mcp_set_enabled", { name, enabled });
}

export async function mcpConnect(name: string): Promise<McpActionResponse> {
  return invoke("mcp_connect", { name });
}

export async function mcpDisconnect(name: string): Promise<McpActionResponse> {
  return invoke("mcp_disconnect", { name });
}

export async function mcpRefresh(name: string): Promise<McpActionResponse> {
  return invoke("mcp_refresh", { name });
}

export async function mcpListTools(): Promise<McpRemoteTool[]> {
  return invoke("mcp_list_tools");
}

export type RoutineStatus = "active" | "paused";

export type RoutineSchedule =
  | { interval_seconds: { seconds: number } }
  | { cron: { expression: string } };

export type RoutineMisfirePolicy =
  | { type: "skip" }
  | { type: "run_once" }
  | { type: "catch_up"; max_runs: number };

export interface RoutineSpec {
  routine_id: string;
  name: string;
  status: RoutineStatus;
  schedule: RoutineSchedule;
  timezone: string;
  misfire_policy: RoutineMisfirePolicy;
  entrypoint: string;
  args: Record<string, unknown>;
  allowed_tools: string[];
  output_targets: string[];
  creator_type: string;
  creator_id: string;
  requires_approval: boolean;
  external_integrations_allowed: boolean;
  next_fire_at_ms?: number | null;
  last_fired_at_ms?: number | null;
}

export interface RoutineCreateRequest {
  routine_id?: string;
  name: string;
  schedule: RoutineSchedule;
  timezone?: string;
  misfire_policy?: RoutineMisfirePolicy;
  entrypoint: string;
  args?: Record<string, unknown>;
  allowed_tools?: string[];
  output_targets?: string[];
  creator_type?: string;
  creator_id?: string;
  requires_approval?: boolean;
  external_integrations_allowed?: boolean;
  next_fire_at_ms?: number;
}

export interface RoutinePatchRequest {
  name?: string;
  status?: RoutineStatus;
  schedule?: RoutineSchedule;
  timezone?: string;
  misfire_policy?: RoutineMisfirePolicy;
  entrypoint?: string;
  args?: Record<string, unknown>;
  allowed_tools?: string[];
  output_targets?: string[];
  requires_approval?: boolean;
  external_integrations_allowed?: boolean;
  next_fire_at_ms?: number;
}

export async function routinesList(): Promise<RoutineSpec[]> {
  return invoke("routines_list");
}

export async function routinesCreate(request: RoutineCreateRequest): Promise<RoutineSpec> {
  return invoke("routines_create", { request });
}

export async function routinesPatch(
  routineId: string,
  request: RoutinePatchRequest
): Promise<RoutineSpec> {
  return invoke("routines_patch", { routineId, request });
}

export async function routinesDelete(routineId: string): Promise<boolean> {
  return invoke("routines_delete", { routineId });
}

export type RoutineRunStatus =
  | "queued"
  | "pending_approval"
  | "running"
  | "paused"
  | "blocked_policy"
  | "denied"
  | "completed"
  | "failed"
  | "cancelled";

export interface RoutineRunArtifact {
  artifact_id: string;
  uri: string;
  kind: string;
  label?: string | null;
  created_at_ms: number;
  metadata?: Record<string, unknown> | null;
}

export interface RoutineRunRecord {
  run_id: string;
  routine_id: string;
  trigger_type: string;
  run_count: number;
  status: RoutineRunStatus;
  created_at_ms: number;
  updated_at_ms: number;
  fired_at_ms?: number | null;
  started_at_ms?: number | null;
  finished_at_ms?: number | null;
  requires_approval: boolean;
  approval_reason?: string | null;
  denial_reason?: string | null;
  paused_reason?: string | null;
  detail?: string | null;
  entrypoint: string;
  args: Record<string, unknown>;
  allowed_tools: string[];
  output_targets: string[];
  artifacts: RoutineRunArtifact[];
}

export interface RoutineRunDecisionRequest {
  reason?: string;
}

export interface RoutineRunArtifactAddRequest {
  uri: string;
  kind: string;
  label?: string;
  metadata?: Record<string, unknown>;
}

export async function routinesRuns(
  routineId: string,
  limit?: number
): Promise<RoutineRunRecord[]> {
  return invoke("routines_runs", { routineId, limit });
}

export async function routinesRunsAll(
  routineId?: string,
  limit?: number
): Promise<RoutineRunRecord[]> {
  return invoke("routines_runs_all", { routineId, limit });
}

export async function routinesRunGet(runId: string): Promise<RoutineRunRecord> {
  return invoke("routines_run_get", { runId });
}

export async function routinesRunApprove(
  runId: string,
  request?: RoutineRunDecisionRequest
): Promise<RoutineRunRecord> {
  return invoke("routines_run_approve", { runId, request });
}

export async function routinesRunDeny(
  runId: string,
  request?: RoutineRunDecisionRequest
): Promise<RoutineRunRecord> {
  return invoke("routines_run_deny", { runId, request });
}

export async function routinesRunPause(
  runId: string,
  request?: RoutineRunDecisionRequest
): Promise<RoutineRunRecord> {
  return invoke("routines_run_pause", { runId, request });
}

export async function routinesRunResume(
  runId: string,
  request?: RoutineRunDecisionRequest
): Promise<RoutineRunRecord> {
  return invoke("routines_run_resume", { runId, request });
}

export async function routinesRunArtifacts(runId: string): Promise<RoutineRunArtifact[]> {
  return invoke("routines_run_artifacts", { runId });
}

export async function routinesRunAddArtifact(
  runId: string,
  request: RoutineRunArtifactAddRequest
): Promise<RoutineRunRecord> {
  return invoke("routines_run_add_artifact", { runId, request });
}

// ============================================================================
// Sidecar Management
// ============================================================================

export async function startSidecar(): Promise<number> {
  return invoke("start_sidecar");
}

export async function stopSidecar(): Promise<void> {
  return invoke("stop_sidecar");
}

export async function getSidecarStatus(): Promise<SidecarState> {
  return invoke("get_sidecar_status");
}

export async function getSidecarStartupHealth(): Promise<SidecarStartupHealth | null> {
  return invoke("get_sidecar_startup_health");
}

export async function getRuntimeDiagnostics(): Promise<RuntimeDiagnostics> {
  return invoke("get_runtime_diagnostics");
}

export async function getEngineApiToken(reveal = false): Promise<EngineApiTokenInfo> {
  return invoke("get_engine_api_token", { reveal });
}

export async function engineAcquireLease(
  clientId: string,
  clientType: string,
  ttlMs?: number
): Promise<EngineLeaseInfo> {
  return invoke("engine_acquire_lease", { clientId, clientType, ttlMs });
}

export async function engineRenewLease(leaseId: string): Promise<boolean> {
  return invoke("engine_renew_lease", { leaseId });
}

export async function engineReleaseLease(leaseId: string): Promise<boolean> {
  return invoke("engine_release_lease", { leaseId });
}

export async function checkSidecarStatus(): Promise<SidecarStatus> {
  return invoke("check_sidecar_status");
}

export async function downloadSidecar(): Promise<void> {
  return invoke("download_sidecar");
}

// ============================================================================
// Session Management
// ============================================================================

export async function createSession(
  title?: string,
  model?: string,
  provider?: string,
  allowAllTools?: boolean,
  modeId?: string
): Promise<Session> {
  const session = await invoke<Session>("create_session", {
    title,
    model,
    provider,
    allowAllTools,
    modeId,
  });
  return normalizeSession(session);
}

export async function getSession(sessionId: string): Promise<Session> {
  const session = await invoke<Session>("get_session", { sessionId });
  return normalizeSession(session);
}

export async function listSessions(): Promise<Session[]> {
  const sessions = await invoke<Session[]>("list_sessions");
  return sessions.map(normalizeSession);
}

export async function getSessionActiveRun(
  sessionId: string
): Promise<ActiveRunStatusResponse | null> {
  return invoke("get_session_active_run", { sessionId });
}

export async function deleteSession(sessionId: string): Promise<void> {
  return invoke("delete_session", { sessionId });
}

export async function deleteOrchestratorRun(runId: string): Promise<void> {
  return invoke("orchestrator_delete_run", { runId });
}

export async function getCurrentSessionId(): Promise<string | null> {
  return invoke("get_current_session_id");
}

export async function setCurrentSessionId(sessionId: string | null): Promise<void> {
  return invoke("set_current_session_id", { sessionId });
}

export interface PlanSessionResult {
  session: Session;
  plan_path: string;
}

export async function startPlanSession(goal?: string): Promise<PlanSessionResult> {
  return invoke("start_plan_session", { goal });
}

// ============================================================================
// Project & History
// ============================================================================

export async function listProjects(): Promise<Project[]> {
  return invoke("list_projects");
}

export async function getSessionMessages(sessionId: string): Promise<SessionMessage[]> {
  const messages = await invoke<SessionMessage[]>("get_session_messages", { sessionId });
  return messages.map(normalizeSessionMessage);
}

export async function listToolExecutions(
  sessionId: string,
  limit: number = 200,
  beforeTsMs?: number
): Promise<ToolExecutionRow[]> {
  return invoke("list_tool_executions", { sessionId, limit, beforeTsMs });
}

export async function getSessionTodos(sessionId: string): Promise<TodoItem[]> {
  return invoke("get_session_todos", { sessionId });
}

// ============================================================================
// Message Handling
// ============================================================================

export interface FileAttachmentInput {
  mime: string;
  filename?: string;
  url: string;
}

export async function sendMessage(
  sessionId: string,
  content: string,
  attachments?: FileAttachmentInput[]
): Promise<void> {
  return invoke("send_message", { sessionId, content, attachments });
}

export async function sendMessageAndStartRun(
  sessionId: string,
  content: string,
  attachments?: FileAttachmentInput[],
  agent?: string,
  modeId?: string,
  model?: string,
  provider?: string
): Promise<void> {
  return invoke("send_message_and_start_run", {
    sessionId,
    content,
    attachments,
    agent,
    modeId,
    model,
    provider,
  });
}

export async function listModes(): Promise<ModeDefinition[]> {
  return invoke("list_modes");
}

export async function upsertMode(scope: ModeScope, mode: ModeDefinition): Promise<void> {
  return invoke("upsert_mode", { scope, mode });
}

export async function deleteMode(scope: ModeScope, id: string): Promise<void> {
  return invoke("delete_mode", { scope, id });
}

export async function importModes(scope: ModeScope, json: string): Promise<void> {
  return invoke("import_modes", { scope, json });
}

export async function exportModes(scope: ModeScope): Promise<string> {
  return invoke("export_modes", { scope });
}

export interface QueuedAttachment {
  mime: string;
  filename?: string;
  url: string;
}

export interface QueuedMessage {
  id: string;
  content: string;
  attachments: QueuedAttachment[];
  created_at_ms: number;
}

export async function queueMessage(
  sessionId: string,
  content: string,
  attachments?: FileAttachmentInput[]
): Promise<QueuedMessage> {
  return invoke("queue_message", { sessionId, content, attachments });
}

export async function queueList(sessionId: string): Promise<QueuedMessage[]> {
  return invoke("queue_list", { sessionId });
}

export async function queueRemove(sessionId: string, itemId: string): Promise<boolean> {
  return invoke("queue_remove", { sessionId, itemId });
}

export async function queueSendNext(sessionId: string): Promise<boolean> {
  return invoke("queue_send_next", { sessionId });
}

export async function queueSendAll(sessionId: string): Promise<number> {
  return invoke("queue_send_all", { sessionId });
}

export async function cancelGeneration(sessionId: string): Promise<void> {
  return invoke("cancel_generation", { sessionId });
}

// ============================================================================
// Model & Provider Info
// ============================================================================

export async function listModels(): Promise<ModelInfo[]> {
  return invoke("list_models");
}

export async function listProvidersFromSidecar(): Promise<ProviderInfo[]> {
  return invoke("list_providers_from_sidecar");
}

export async function listOllamaModels(): Promise<ModelInfo[]> {
  return invoke("list_ollama_models");
}

export async function listRunningOllamaModels(): Promise<ModelInfo[]> {
  return invoke("list_running_ollama_models");
}

export async function stopOllamaModel(name: string): Promise<void> {
  return invoke("stop_ollama_model", { name });
}

export async function runOllamaModel(name: string): Promise<void> {
  return invoke("run_ollama_model", { name });
}

// ============================================================================
// File Operation Undo
// ============================================================================

export interface JournalEntry {
  id: string;
  timestamp: string;
  tool_name: string;
  args: unknown;
  status: "pending_approval" | "approved" | "denied" | "completed" | "rolled_back" | "failed";
  before_state?: FileSnapshot;
  after_state?: FileSnapshot;
  user_approved: boolean;
}

export interface FileSnapshot {
  path: string;
  content?: string;
  exists: boolean;
  is_directory: boolean;
}

export interface UndoResult {
  reverted_entry_id: string;
  path: string;
  operation: string;
}

export async function canUndoFileChange(): Promise<boolean> {
  return invoke("can_undo_file_change");
}

export async function undoLastFileChange(): Promise<UndoResult | null> {
  return invoke("undo_last_file_change");
}

export async function getRecentFileOperations(count: number): Promise<JournalEntry[]> {
  return invoke("get_recent_file_operations", { count });
}

// ============================================================================
// Conversation Rewind
// ============================================================================

export async function rewindToMessage(
  sessionId: string,
  messageId: string,
  editedContent?: string
): Promise<Session> {
  return invoke("rewind_to_message", { sessionId, messageId, editedContent });
}

// ============================================================================
// Message Undo/Redo (OpenCode native)
// ============================================================================

export async function undoMessage(sessionId: string, messageId: string): Promise<void> {
  return invoke("undo_message", { sessionId, messageId });
}

export async function redoMessage(sessionId: string): Promise<void> {
  return invoke("redo_message", { sessionId });
}

export async function undoViaCommand(sessionId: string): Promise<void> {
  return invoke("undo_via_command", { sessionId });
}

// ============================================================================
// Undo (Message + Files)
// ============================================================================

export async function undoMessageWithFiles(
  sessionId: string,
  messageId: string
): Promise<string[]> {
  return invoke("undo_message_with_files", { sessionId, messageId });
}

export async function snapshotFileForMessage(
  filePath: string,
  tool: string,
  messageId: string
): Promise<void> {
  return invoke("snapshot_file_for_message", { filePath, tool, messageId });
}

// ============================================================================
// Tool Approval
// ============================================================================

export async function approveTool(
  sessionId: string,
  toolCallId: string,
  meta?: {
    tool?: string;
    args?: Record<string, unknown>;
    messageId?: string;
  }
): Promise<void> {
  return invoke("approve_tool", {
    sessionId,
    toolCallId,
    tool: meta?.tool,
    args: meta?.args,
    messageId: meta?.messageId,
  });
}

export async function denyTool(
  sessionId: string,
  toolCallId: string,
  meta?: {
    tool?: string;
    args?: Record<string, unknown>;
    messageId?: string;
  }
): Promise<void> {
  return invoke("deny_tool", {
    sessionId,
    toolCallId,
    tool: meta?.tool,
    args: meta?.args,
    messageId: meta?.messageId,
  });
}

export async function answerQuestion(
  sessionId: string,
  questionId: string,
  answer: string
): Promise<void> {
  return invoke("answer_question", { sessionId, questionId, answer });
}

export async function listQuestions(): Promise<QuestionRequestEvent[]> {
  return invoke("list_questions");
}

export async function replyQuestion(requestId: string, answers: string[][]): Promise<void> {
  return invoke("reply_question", { requestId, answers });
}

export async function rejectQuestion(requestId: string): Promise<void> {
  return invoke("reject_question", { requestId });
}

// ============================================================================
// Agent Teams / Command Center
// ============================================================================

export interface AgentTeamBudgetLimit {
  max_tokens?: number | null;
  max_steps?: number | null;
  max_tool_calls?: number | null;
  max_duration_ms?: number | null;
  max_cost_usd?: number | null;
}

export interface AgentTeamTemplate {
  template_id: string;
  role: string;
  system_prompt?: string | null;
  skills: unknown[];
  default_budget: AgentTeamBudgetLimit;
  capabilities: Record<string, unknown>;
}

export interface AgentTeamInstance {
  instance_id: string;
  mission_id: string;
  parent_instance_id?: string | null;
  role: string;
  template_id: string;
  session_id: string;
  run_id?: string | null;
  status: string;
  budget: AgentTeamBudgetLimit;
  skill_hash: string;
  capabilities: Record<string, unknown>;
  metadata?: Record<string, unknown> | null;
}

export interface AgentTeamMissionSummary {
  mission_id: string;
  instance_count: number;
  running_count: number;
  completed_count: number;
  failed_count: number;
  cancelled_count: number;
  queued_count: number;
  token_used_total: number;
  tool_calls_used_total: number;
  steps_used_total: number;
  cost_used_usd_total: number;
}

export interface AgentTeamSpawnApproval {
  approval_id: string;
  created_at_ms: number;
  request: Record<string, unknown>;
  decision_code?: string | null;
  reason?: string | null;
}

export interface AgentTeamApprovals {
  spawn_approvals: AgentTeamSpawnApproval[];
  tool_approvals: AgentTeamToolApproval[];
}

export interface AgentTeamToolApproval {
  approval_id: string;
  session_id?: string | null;
  tool_call_id: string;
  tool?: string | null;
  args?: Record<string, unknown> | null;
  status: string;
}

export interface AgentTeamSpawnRequest {
  mission_id?: string | null;
  parent_instance_id?: string | null;
  template_id?: string | null;
  role: string;
  source?: string | null;
  justification: string;
  budget_override?: AgentTeamBudgetLimit | null;
}

export interface AgentTeamSpawnResult {
  ok: boolean;
  mission_id?: string | null;
  instance_id?: string | null;
  session_id?: string | null;
  run_id?: string | null;
  status?: string | null;
  skill_hash?: string | null;
  code?: string | null;
  error?: string | null;
  requires_user_approval?: boolean | null;
}

export interface AgentTeamDecisionResult {
  ok: boolean;
  approval_id?: string | null;
  decision?: string | null;
  instance_id?: string | null;
  session_id?: string | null;
  mission_id?: string | null;
  status?: string | null;
  reason?: string | null;
  code?: string | null;
  error?: string | null;
}

export async function agentTeamListTemplates(): Promise<AgentTeamTemplate[]> {
  return invoke("agent_team_list_templates");
}

export async function agentTeamListInstances(params?: {
  mission_id?: string;
  parent_instance_id?: string;
  status?: string;
}): Promise<AgentTeamInstance[]> {
  return invoke("agent_team_list_instances", {
    missionId: params?.mission_id,
    parentInstanceId: params?.parent_instance_id,
    status: params?.status,
  });
}

export async function agentTeamListMissions(): Promise<AgentTeamMissionSummary[]> {
  return invoke("agent_team_list_missions");
}

export async function agentTeamListApprovals(): Promise<AgentTeamApprovals> {
  return invoke("agent_team_list_approvals");
}

export async function agentTeamSpawn(
  request: AgentTeamSpawnRequest
): Promise<AgentTeamSpawnResult> {
  return invoke("agent_team_spawn", { request });
}

export async function agentTeamCancelInstance(
  instanceId: string,
  reason?: string
): Promise<AgentTeamDecisionResult> {
  return invoke("agent_team_cancel_instance", { instanceId, reason });
}

export async function agentTeamCancelMission(
  missionId: string,
  reason?: string
): Promise<AgentTeamDecisionResult> {
  return invoke("agent_team_cancel_mission", { missionId, reason });
}

export async function agentTeamApproveSpawn(
  approvalId: string,
  reason?: string
): Promise<AgentTeamDecisionResult> {
  return invoke("agent_team_approve_spawn", { approvalId, reason });
}

export async function agentTeamDenySpawn(
  approvalId: string,
  reason?: string
): Promise<AgentTeamDecisionResult> {
  return invoke("agent_team_deny_spawn", { approvalId, reason });
}

// ============================================================================
// Execution Planning / Staging Area
// ============================================================================

export interface StagedOperation {
  id: string;
  request_id: string;
  session_id: string;
  tool: string;
  args: Record<string, unknown>;
  before_snapshot?: FileSnapshot;
  proposed_content?: string;
  timestamp: string;
  description: string;
  message_id?: string;
}

export async function stageToolOperation(
  requestId: string,
  sessionId: string,
  tool: string,
  args: Record<string, unknown>,
  messageId?: string
): Promise<void> {
  return invoke("stage_tool_operation", {
    requestId,
    sessionId,
    tool,
    args,
    messageId,
  });
}

export async function getStagedOperations(): Promise<StagedOperation[]> {
  return invoke("get_staged_operations");
}

export async function executeStagedPlan(): Promise<string[]> {
  return invoke("execute_staged_plan");
}

export async function removeStagedOperation(operationId: string): Promise<boolean> {
  return invoke("remove_staged_operation", { operationId });
}

export async function clearStagingArea(): Promise<number> {
  return invoke("clear_staging_area");
}

export async function getStagedCount(): Promise<number> {
  return invoke("get_staged_count");
}

// ============================================================================
// File Browser
// ============================================================================

export async function readDirectory(path: string): Promise<FileEntry[]> {
  const entries = await invoke<FileEntry[]>("read_directory", { path });

  // Tauri/serde serializes Rust `Option<T>` as `null` when `None`.
  // Normalize `null` to `undefined` so UI checks like `typeof size === "number"` behave predictably.
  return entries.map((e) => {
    const raw = e as unknown as { size?: unknown; extension?: unknown };
    const size = typeof raw.size === "number" ? raw.size : undefined;
    const extension = typeof raw.extension === "string" ? raw.extension : undefined;
    return { ...e, size, extension };
  });
}

export async function readFileContent(path: string, maxSize?: number): Promise<string> {
  return invoke("read_file_content", { path, maxSize });
}

export async function readFileText(
  path: string,
  maxSize?: number,
  maxChars?: number
): Promise<string> {
  return invoke("read_file_text", { path, maxSize, maxChars });
}

export async function readBinaryFile(path: string, maxSize?: number): Promise<string> {
  return invoke("read_binary_file", { path, maxSize });
}

// ============================================================================
// Files View: File Tree Watcher (auto-refresh)
// ============================================================================

export interface FileTreeChangedPayload {
  root: string;
  paths: string[];
}

export async function startFileTreeWatcher(windowLabel: string, rootPath: string): Promise<void> {
  return invoke("start_file_tree_watcher", { windowLabel, rootPath });
}

export async function stopFileTreeWatcher(): Promise<void> {
  return invoke("stop_file_tree_watcher");
}

// ============================================================================
// Python Environment (Workspace Venv Wizard)
// ============================================================================

export interface PythonCandidate {
  kind: "py" | "python" | "python3" | string;
  version: string;
  command: string[];
}

export interface PythonStatus {
  found: boolean;
  candidates: PythonCandidate[];
  workspace_path?: string | null;
  venv_root?: string | null;
  venv_python?: string | null;
  venv_exists: boolean;
  config_path?: string | null;
}

export interface PythonInstallResult {
  ok: boolean;
  exit_code?: number | null;
  stdout: string;
  stderr: string;
}

export async function pythonGetStatus(): Promise<PythonStatus> {
  return invoke("python_get_status");
}

export async function pythonCreateVenv(
  selected?: "py" | "python" | "python3"
): Promise<PythonStatus> {
  return invoke("python_create_venv", { selected });
}

export async function pythonInstallRequirements(
  requirementsPath: string
): Promise<PythonInstallResult> {
  return invoke("python_install_requirements", { requirementsPath });
}

// ============================================================================
// Tool Definitions (for conditional tool injection)
// ============================================================================

export interface ToolGuidance {
  category: string;
  instructions: string;
  json_schema: Record<string, unknown>;
  example: string;
}

export async function getToolGuidance(categories: string[]): Promise<ToolGuidance[]> {
  return invoke("get_tool_guidance", { categories });
}

// ============================================================================
// Presentation Export
// ============================================================================

export async function exportPresentation(jsonPath: string, outputPath: string): Promise<string> {
  return invoke("export_presentation", { jsonPath, outputPath });
}

// ============================================================================
// Event Listeners
// ============================================================================

export function onSidecarEvent(callback: (event: StreamEvent) => void): Promise<UnlistenFn> {
  return listen<StreamEvent>("sidecar_event", (event) => {
    callback(event.payload);
  });
}

export function onSidecarEventV2(
  callback: (event: StreamEventEnvelopeV2) => void
): Promise<UnlistenFn> {
  return listen<StreamEventEnvelopeV2>("sidecar_event_v2", (event) => {
    callback(event.payload);
  });
}

// ============================================================================
// Log Streaming (On-Demand Diagnostics)
// ============================================================================

export type LogSource = "tandem" | "sidecar";

export interface LogFileInfo {
  name: string;
  size: number;
  modified_ms: number;
}

export interface LogStreamBatch {
  stream_id: string;
  source: LogSource;
  lines: string[];
  dropped?: number;
  ts_ms?: number;
}

export async function listAppLogFiles(): Promise<LogFileInfo[]> {
  return invoke("list_app_log_files");
}

export async function startLogStream(args: {
  windowLabel?: string;
  source: LogSource;
  fileName?: string;
  tailLines?: number;
}): Promise<string> {
  return invoke("start_log_stream", {
    windowLabel: args.windowLabel ?? "main",
    source: args.source,
    fileName: args.fileName,
    tailLines: args.tailLines,
  });
}

export async function stopLogStream(streamId: string): Promise<void> {
  return invoke("stop_log_stream", { streamId });
}

export function onLogStreamEvent(callback: (batch: LogStreamBatch) => void): Promise<UnlistenFn> {
  return listen<LogStreamBatch>("log_stream_event", (event) => {
    callback(event.payload);
  });
}

export function onStorageMigrationProgress(
  callback: (event: StorageMigrationProgressEvent) => void
): Promise<UnlistenFn> {
  return listen<StorageMigrationProgressEvent>("storage-migration-progress", (event) => {
    callback(event.payload);
  });
}

export function onStorageMigrationComplete(
  callback: (result: StorageMigrationRunResult) => void
): Promise<UnlistenFn> {
  return listen<StorageMigrationRunResult>("storage-migration-complete", (event) => {
    callback(event.payload);
  });
}

// ============================================================================
// Skills Management
// ============================================================================

export interface SkillInfo {
  name: string;
  description: string;
  location: "project" | "global";
  path: string;
  version?: string;
  author?: string;
  tags: string[];
  requires: string[];
  compatibility?: string;
  triggers: string[];
  parse_error?: string;
}

export type SkillLocation = "project" | "global";

export async function listSkills(): Promise<SkillInfo[]> {
  return invoke<SkillInfo[]>("list_skills");
}

export async function importSkill(content: string, location: SkillLocation): Promise<SkillInfo> {
  return invoke<SkillInfo>("import_skill", { content, location });
}

export type SkillsConflictPolicy = "skip" | "overwrite" | "rename";

export interface SkillsImportPreviewItem {
  source: string;
  valid: boolean;
  name?: string;
  description?: string;
  conflict: boolean;
  action: string;
  target_path?: string;
  error?: string;
  version?: string;
  author?: string;
  tags: string[];
  requires: string[];
  compatibility?: string;
  triggers: string[];
}

export interface SkillsImportPreview {
  items: SkillsImportPreviewItem[];
  total: number;
  valid: number;
  invalid: number;
  conflicts: number;
}

export interface SkillsImportResult {
  imported: SkillInfo[];
  skipped: string[];
  errors: string[];
}

export async function skillsImportPreview(
  fileOrPath: string,
  location: SkillLocation,
  namespace?: string,
  conflictPolicy: SkillsConflictPolicy = "skip"
): Promise<SkillsImportPreview> {
  return invoke("skills_import_preview", { fileOrPath, location, namespace, conflictPolicy });
}

export async function skillsImport(
  fileOrPath: string,
  location: SkillLocation,
  namespace?: string,
  conflictPolicy: SkillsConflictPolicy = "skip"
): Promise<SkillsImportResult> {
  return invoke("skills_import", { fileOrPath, location, namespace, conflictPolicy });
}

export async function deleteSkill(name: string, location: SkillLocation): Promise<void> {
  return invoke<void>("delete_skill", { name, location });
}

export interface SkillTemplateInfo {
  id: string;
  name: string;
  description: string;
  requires?: string[];
}

export async function listSkillTemplates(): Promise<SkillTemplateInfo[]> {
  return invoke<SkillTemplateInfo[]>("skills_list_templates");
}

export async function installSkillTemplate(
  templateId: string,
  location: SkillLocation
): Promise<SkillInfo> {
  return invoke<SkillInfo>("skills_install_template", { templateId, location });
}

// ============================================================================
// Memory Management
// ============================================================================

export interface MemoryStats {
  total_chunks: number;
  session_chunks: number;
  project_chunks: number;
  global_chunks: number;
  total_bytes: number;
  session_bytes: number;
  project_bytes: number;
  global_bytes: number;
  file_size: number;
  last_cleanup: string | null;
}

export async function getMemoryStats(): Promise<MemoryStats> {
  return invoke<MemoryStats>("get_memory_stats");
}

export interface MemorySettings {
  auto_index_on_project_load: boolean;
  embedding_status?: string;
  embedding_reason?: string | null;
}

export async function getMemorySettings(): Promise<MemorySettings> {
  return invoke<MemorySettings>("get_memory_settings");
}

export async function setMemorySettings(settings: MemorySettings): Promise<void> {
  return invoke<void>("set_memory_settings", { settings });
}

export interface ProjectMemoryStats {
  project_id: string;
  project_chunks: number;
  project_bytes: number;
  file_index_chunks: number;
  file_index_bytes: number;
  indexed_files: number;
  last_indexed_at: string | null;
  last_total_files: number | null;
  last_processed_files: number | null;
  last_indexed_files: number | null;
  last_skipped_files: number | null;
  last_errors: number | null;
}

export async function getProjectMemoryStats(projectId: string): Promise<ProjectMemoryStats> {
  return invoke<ProjectMemoryStats>("get_project_memory_stats", { projectId });
}

export interface ClearFileIndexResult {
  chunks_deleted: number;
  bytes_estimated: number;
  did_vacuum: boolean;
}

export async function clearProjectFileIndex(
  projectId: string,
  vacuum: boolean
): Promise<ClearFileIndexResult> {
  return invoke<ClearFileIndexResult>("clear_project_file_index", { projectId, vacuum });
}

export interface IndexingStats {
  total_files: number;
  files_processed: number;
  indexed_files: number;
  skipped_files: number;
  deleted_files: number;
  chunks_created: number;
  errors: number;
}

export async function indexWorkspace(projectId: string): Promise<IndexingStats> {
  return invoke<IndexingStats>("index_workspace_command", { projectId });
}

export interface IndexingStart {
  project_id: string;
  total_files: number;
}

export interface IndexingProgress {
  project_id: string;
  files_processed: number;
  total_files: number;
  indexed_files: number;
  skipped_files: number;
  deleted_files: number;
  errors: number;
  chunks_created: number;
  current_file: string;
}

export interface IndexingComplete {
  project_id: string;
  total_files: number;
  files_processed: number;
  indexed_files: number;
  skipped_files: number;
  deleted_files: number;
  chunks_created: number;
  errors: number;
}

// ============================================================================
// OpenCode Config (Plugins + MCP)
// ============================================================================

export type OpenCodeConfigScope = "global" | "project";

export async function opencodeListPlugins(scope: OpenCodeConfigScope): Promise<string[]> {
  return invoke<string[]>("opencode_list_plugins", { scope });
}

export async function opencodeAddPlugin(
  scope: OpenCodeConfigScope,
  name: string
): Promise<string[]> {
  return invoke<string[]>("opencode_add_plugin", { scope, name });
}

export async function opencodeRemovePlugin(
  scope: OpenCodeConfigScope,
  name: string
): Promise<string[]> {
  return invoke<string[]>("opencode_remove_plugin", { scope, name });
}

export interface OpencodeMcpServerEntry {
  name: string;
  config: Record<string, unknown>;
}

export async function opencodeListMcpServers(
  scope: OpenCodeConfigScope
): Promise<OpencodeMcpServerEntry[]> {
  return invoke<OpencodeMcpServerEntry[]>("opencode_list_mcp_servers", { scope });
}

export async function opencodeAddMcpServer(
  scope: OpenCodeConfigScope,
  name: string,
  config: Record<string, unknown>
): Promise<OpencodeMcpServerEntry[]> {
  return invoke<OpencodeMcpServerEntry[]>("opencode_add_mcp_server", { scope, name, config });
}

export async function opencodeRemoveMcpServer(
  scope: OpenCodeConfigScope,
  name: string
): Promise<OpencodeMcpServerEntry[]> {
  return invoke<OpencodeMcpServerEntry[]>("opencode_remove_mcp_server", { scope, name });
}

export interface OpencodeMcpTestResult {
  status: "connected" | "failed" | "not_supported" | "not_found" | string;
  ok: boolean;
  http_status?: number | null;
  error?: string | null;
}

export async function opencodeTestMcpConnection(
  scope: OpenCodeConfigScope,
  name: string
): Promise<OpencodeMcpTestResult> {
  return invoke<OpencodeMcpTestResult>("opencode_test_mcp_connection", { scope, name });
}

// ============================================================================
// Packs (guided workflows)
// ============================================================================

export interface PackMeta {
  id: string;
  title: string;
  description: string;
  complexity: string;
  time_estimate: string;
  tags: string[];
}

export interface PackInstallResult {
  installed_path: string;
}

export async function listPacks(): Promise<PackMeta[]> {
  return invoke<PackMeta[]>("packs_list");
}

export async function installPack(
  packId: string,
  destinationDir: string
): Promise<PackInstallResult> {
  return invoke<PackInstallResult>("packs_install", { packId, destinationDir });
}

export async function installPackDefault(packId: string): Promise<PackInstallResult> {
  return invoke<PackInstallResult>("packs_install_default", { packId });
}

// ============================================================================
// Ralph Loop (Iterative Task Agent)
// ============================================================================

export type RalphRunStatus = "idle" | "running" | "paused" | "completed" | "cancelled" | "error";

export interface RalphStateSnapshot {
  run_id: string;
  status: RalphRunStatus;
  iteration: number;
  total_iterations: number;
  last_duration_ms?: number;
  files_modified_count: number;
  struggle_detected: boolean;
}

export interface IterationRecord {
  iteration: number;
  started_at: string;
  ended_at: string;
  duration_ms: number;
  completion_detected: boolean;
  tools_used: Record<string, number>;
  files_modified: string[];
  errors: string[];
  context_injected?: string;
}

export async function ralphStart(goal: string, permissions: string[]): Promise<string> {
  return invoke("ralph_start", { goal, permissions });
}

export async function ralphCancel(runId: string): Promise<void> {
  return invoke("ralph_cancel", { runId });
}

export async function ralphPause(runId: string): Promise<void> {
  return invoke("ralph_pause", { runId });
}

export async function ralphResume(runId: string): Promise<void> {
  return invoke("ralph_resume", { runId });
}

export async function ralphAddContext(runId: string, text: string): Promise<void> {
  return invoke("ralph_add_context", { runId, text });
}

export async function ralphStatus(runId?: string): Promise<RalphStateSnapshot> {
  return invoke("ralph_status", { runId });
}

export async function ralphHistory(runId: string, limit?: number): Promise<IterationRecord[]> {
  return invoke("ralph_history", { runId, limit });
}
