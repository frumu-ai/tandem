// Tauri API wrapper functions
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ============================================================================
// Provider Configuration Types
// ============================================================================

export interface ProviderConfig {
  enabled: boolean;
  default: boolean;
  endpoint: string;
  model?: string;
}

export interface ProvidersConfig {
  openrouter: ProviderConfig;
  anthropic: ProviderConfig;
  openai: ProviderConfig;
  ollama: ProviderConfig;
  custom: ProviderConfig[];
}

export interface AppStateInfo {
  workspace_path: string | null;
  has_workspace: boolean;
  providers_config: ProvidersConfig;
}

// API Key types
export type ApiKeyType = "openrouter" | "anthropic" | "openai" | "ollama" | string;

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
  provider?: string
): Promise<Session> {
  return invoke("create_session", { title, model, provider });
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
  attachments?: FileAttachmentInput[]
): Promise<void> {
  return invoke("send_message_streaming", { sessionId, content, attachments });
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
// Tool Approval
// ============================================================================

export async function approveTool(sessionId: string, toolCallId: string): Promise<void> {
  return invoke("approve_tool", { sessionId, toolCallId });
}

export async function denyTool(sessionId: string, toolCallId: string): Promise<void> {
  return invoke("deny_tool", { sessionId, toolCallId });
}

// ============================================================================
// Event Listeners
// ============================================================================

export function onSidecarEvent(callback: (event: StreamEvent) => void): Promise<UnlistenFn> {
  return listen<StreamEvent>("sidecar_event", (event) => {
    callback(event.payload);
  });
}
