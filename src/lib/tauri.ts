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
  last_update_check: string | null;
  update_available: boolean;
  remote_version: string | null;
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
  | { type: "raw"; event_type: string; data: unknown };

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
  allowAllTools?: boolean
): Promise<Session> {
  return invoke("create_session", {
    title,
    model,
    provider,
    allow_all_tools: allowAllTools,
  });
}

export async function getSession(sessionId: string): Promise<Session> {
  return invoke("get_session", { sessionId });
}

export async function listSessions(): Promise<Session[]> {
  return invoke("list_sessions");
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
  return invoke("get_session_messages", { sessionId });
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

export async function sendMessageStreaming(
  sessionId: string,
  content: string,
  attachments?: FileAttachmentInput[],
  agent?: string
): Promise<void> {
  return invoke("send_message_streaming", { sessionId, content, attachments, agent });
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
  return invoke("read_directory", { path });
}

export async function readFileContent(path: string, maxSize?: number): Promise<string> {
  return invoke("read_file_content", { path, maxSize });
}

export async function readBinaryFile(path: string, maxSize?: number): Promise<string> {
  return invoke("read_binary_file", { path, maxSize });
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

// ============================================================================
// Skills Management
// ============================================================================

export interface SkillInfo {
  name: string;
  description: string;
  location: "project" | "global";
  path: string;
}

export type SkillLocation = "project" | "global";

export async function listSkills(): Promise<SkillInfo[]> {
  return invoke<SkillInfo[]>("list_skills");
}

export async function importSkill(content: string, location: SkillLocation): Promise<SkillInfo> {
  return invoke<SkillInfo>("import_skill", { content, location });
}

export async function deleteSkill(name: string, location: SkillLocation): Promise<void> {
  return invoke<void>("delete_skill", { name, location });
}

export interface SkillTemplateInfo {
  id: string;
  name: string;
  description: string;
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
