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

export interface ProvidersConfig {
  openrouter: ProviderConfig;
  opencode_zen: ProviderConfig;
  anthropic: ProviderConfig;
  openai: ProviderConfig;
  ollama: ProviderConfig;
  custom: ProviderConfig[];
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
export type ApiKeyType = "openrouter" | "opencode_zen" | "anthropic" | "openai" | "ollama" | string;

// ============================================================================
// Sidecar Types
// ============================================================================

export type SidecarState = "stopped" | "starting" | "running" | "stopping" | "failed";

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

export interface QuestionOption {
  id: string;
  label: string;
}

export interface QuestionEvent {
  session_id: string;
  question_id: string;
  header?: string;
  question: string;
  options: QuestionOption[];
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
      question_id: string;
      header?: string;
      question: string;
      options: QuestionOption[];
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

export async function getCurrentSessionId(): Promise<string | null> {
  return invoke("get_current_session_id");
}

export async function setCurrentSessionId(sessionId: string | null): Promise<void> {
  return invoke("set_current_session_id", { sessionId });
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

export async function readBinaryFile(path: string): Promise<string> {
  return invoke("read_binary_file", { path });
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
