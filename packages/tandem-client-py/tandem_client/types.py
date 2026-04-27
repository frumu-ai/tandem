"""Pydantic v2 models for the Tandem engine HTTP API — full parity with tandem-server."""
from __future__ import annotations

from typing import Any, Literal, Optional, Union
from pydantic import BaseModel, ConfigDict, Field, AliasChoices

# ─── Enums & Core ──────────────────────────────────────────────────────────────

RunStatus = Literal["queued", "running", "succeeded", "failed", "canceled", "unknown"]
RoutineStatus = Literal["enabled", "disabled", "paused", "unknown"]
ApprovalStatus = Literal["pending", "approved", "rejected", "unknown"]
ChannelName = Literal["telegram", "discord", "slack"]
SkillLocation = Literal["user", "workspace", "builtin"]

JsonValue = Union[str, int, float, bool, None, dict[str, Any], list[Any]]
ToolMode = Literal["auto", "none", "required"]
ContextMode = Literal["auto", "compact", "full"]


class SystemHealth(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ready: Optional[bool] = None
    phase: Optional[str] = None


# ─── Browser ──────────────────────────────────────────────────────────────────


class BrowserBlockingIssue(BaseModel):
    model_config = ConfigDict(extra="ignore")
    code: Optional[str] = None
    message: Optional[str] = None


class BrowserBinaryStatus(BaseModel):
    model_config = ConfigDict(extra="ignore")
    found: Optional[bool] = None
    path: Optional[str] = None
    version: Optional[str] = None
    channel: Optional[str] = None


class BrowserStatusResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    enabled: Optional[bool] = None
    runnable: Optional[bool] = None
    headless_default: Optional[bool] = None
    sidecar: Optional[BrowserBinaryStatus] = None
    browser: Optional[BrowserBinaryStatus] = None
    blocking_issues: list[BrowserBlockingIssue] = []
    recommendations: list[str] = []
    install_hints: list[str] = []
    last_error: Optional[str] = None


class BrowserInstallResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    ok: Optional[bool] = None
    code: Optional[str] = None
    error: Optional[str] = None
    version: Optional[str] = None
    asset_name: Optional[str] = None
    installed_path: Optional[str] = None
    downloaded_bytes: Optional[int] = None
    status: Optional[BrowserStatusResponse] = None


class BrowserSmokeTestResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    ok: Optional[bool] = None
    code: Optional[str] = None
    error: Optional[str] = None
    status: Optional[BrowserStatusResponse] = None
    url: Optional[str] = None
    final_url: Optional[str] = None
    title: Optional[str] = None
    load_state: Optional[str] = None
    element_count: Optional[int] = None
    excerpt: Optional[str] = None
    closed: Optional[bool] = None


# ─── Sessions ─────────────────────────────────────────────────────────────────


class SessionRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    title: str
    created_at_ms: Optional[int] = Field(None, validation_alias=AliasChoices("createdAtMs", "created_at_ms"))
    directory: Optional[str] = None
    workspace_root: Optional[str] = Field(None, validation_alias=AliasChoices("workspaceRoot", "workspace_root"))
    archived: Optional[bool] = None


class SessionListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    sessions: list[SessionRecord] = []
    count: int = 0


class SessionRunState(BaseModel):
    model_config = ConfigDict(extra="ignore")
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))
    attach_event_stream: Optional[str] = Field(None, validation_alias=AliasChoices("attachEventStream", "attach_event_stream"))


class SessionRunStateResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    active: Optional[SessionRunState] = None


class PromptAsyncResult:
    def __init__(self, run_id: str) -> None:
        self.run_id = run_id

    def __repr__(self) -> str:
        return f"PromptAsyncResult(run_id={self.run_id!r})"


class PromptTextPartInput(BaseModel):
    model_config = ConfigDict(extra="ignore")
    type: Literal["text"] = "text"
    text: str


class PromptFilePartInput(BaseModel):
    model_config = ConfigDict(extra="ignore")
    type: Literal["file"] = "file"
    mime: str
    filename: Optional[str] = None
    url: str


PromptPartInput = Union[PromptTextPartInput, PromptFilePartInput]


class SessionDiff(BaseModel):
    model_config = ConfigDict(extra="ignore")
    diff: Optional[str] = None
    files: Optional[list[str]] = None


class SessionTodo(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: Optional[str] = None
    content: str
    status: Optional[str] = None


# ─── Messages ─────────────────────────────────────────────────────────────────


class MessagePart(BaseModel):
    model_config = ConfigDict(extra="ignore")
    type: Optional[str] = None
    text: Optional[str] = None


class EngineMessage(BaseModel):
    model_config = ConfigDict(extra="ignore")
    info: Optional[dict[str, Any]] = None
    parts: Optional[list[MessagePart]] = None


# ─── Permissions ──────────────────────────────────────────────────────────────


class PermissionRule(BaseModel):
    model_config = ConfigDict(extra="ignore")
    permission: str
    pattern: str
    action: str


class PermissionRequestRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    permission: Optional[str] = None
    pattern: Optional[str] = None
    tool: Optional[str] = None
    status: Optional[ApprovalStatus] = None
    session_id: Optional[str] = Field(None, validation_alias=AliasChoices("sessionID", "sessionId", "session_id"))


class PermissionSnapshotResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    requests: list[PermissionRequestRecord] = []
    rules: list[dict[str, Any]] = []


# ─── Questions ────────────────────────────────────────────────────────────────


class QuestionRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    text: Optional[str] = None
    choices: Optional[list[str]] = None
    status: Optional[ApprovalStatus] = None
    session_id: Optional[str] = Field(None, validation_alias=AliasChoices("sessionID", "sessionId", "session_id"))


class QuestionsListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    questions: list[QuestionRecord] = []


# ─── Providers ────────────────────────────────────────────────────────────────


class ProviderModelEntry(BaseModel):
    model_config = ConfigDict(extra="ignore")
    name: Optional[str] = None


class ProviderEntry(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    name: Optional[str] = None
    models: Optional[dict[str, ProviderModelEntry]] = None
    catalog_source: Optional[str] = None
    catalog_status: Optional[str] = None
    catalog_message: Optional[str] = None


class ProviderCatalog(BaseModel):
    model_config = ConfigDict(extra="ignore")
    all: list[ProviderEntry] = []
    connected: Optional[list[str]] = None
    default: Optional[str] = None


class ProviderConfigEntry(BaseModel):
    model_config = ConfigDict(extra="ignore")
    default_model: Optional[str] = Field(None, validation_alias=AliasChoices("defaultModel", "default_model"))


class ProvidersConfigResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    default: Optional[str] = None
    providers: dict[str, ProviderConfigEntry] = {}


class PersonalityProfile(BaseModel):
    model_config = ConfigDict(extra="ignore")
    preset: Optional[str] = None
    custom_instructions: Optional[str] = Field(
        None, validation_alias=AliasChoices("customInstructions", "custom_instructions")
    )


class PersonalityConfig(BaseModel):
    model_config = ConfigDict(extra="ignore")
    default: Optional[PersonalityProfile] = None
    per_agent: dict[str, PersonalityProfile] = Field(
        default_factory=dict, validation_alias=AliasChoices("perAgent", "per_agent")
    )


class BotIdentityAliases(BaseModel):
    model_config = ConfigDict(extra="ignore")
    desktop: Optional[str] = None
    tui: Optional[str] = None
    portal: Optional[str] = None
    control_panel: Optional[str] = Field(
        None, validation_alias=AliasChoices("controlPanel", "control_panel")
    )
    channels: Optional[str] = None
    protocol: Optional[str] = None
    cli: Optional[str] = None


class BotIdentity(BaseModel):
    model_config = ConfigDict(extra="ignore")
    canonical_name: Optional[str] = Field(
        None, validation_alias=AliasChoices("canonicalName", "canonical_name")
    )
    avatar_url: Optional[str] = Field(
        None, validation_alias=AliasChoices("avatarUrl", "avatar_url")
    )
    aliases: Optional[BotIdentityAliases] = None


class IdentityConfig(BaseModel):
    model_config = ConfigDict(extra="ignore")
    bot: Optional[BotIdentity] = None
    personality: Optional[PersonalityConfig] = None


class PersonalityPresetEntry(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    label: str
    description: Optional[str] = None


class IdentityConfigResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    identity: IdentityConfig
    presets: list[PersonalityPresetEntry] = []


# ─── Channels ─────────────────────────────────────────────────────────────────


class ChannelConfigEntry(BaseModel):
    model_config = ConfigDict(extra="ignore")
    has_token: Optional[bool] = Field(None, validation_alias=AliasChoices("hasToken", "has_token"))
    token_masked: Optional[str] = Field(None, validation_alias=AliasChoices("tokenMasked", "token_masked"))
    allowed_users: Optional[list[str]] = Field(None, validation_alias=AliasChoices("allowedUsers", "allowed_users"))
    mention_only: Optional[bool] = Field(None, validation_alias=AliasChoices("mentionOnly", "mention_only"))
    style_profile: Optional[str] = Field(None, validation_alias=AliasChoices("styleProfile", "style_profile"))
    guild_id: Optional[str] = Field(None, validation_alias=AliasChoices("guildId", "guild_id"))
    channel_id: Optional[str] = Field(None, validation_alias=AliasChoices("channelId", "channel_id"))
    security_profile: Optional[str] = Field(None, validation_alias=AliasChoices("securityProfile", "security_profile"))


class ChannelsConfigResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    telegram: ChannelConfigEntry = Field(default_factory=ChannelConfigEntry)
    discord: ChannelConfigEntry = Field(default_factory=ChannelConfigEntry)
    slack: ChannelConfigEntry = Field(default_factory=ChannelConfigEntry)


class ChannelStatusEntry(BaseModel):
    model_config = ConfigDict(extra="ignore")
    enabled: bool = False
    connected: bool = False
    last_error: Optional[str] = Field(None, validation_alias=AliasChoices("lastError", "last_error"))
    active_sessions: int = Field(0, validation_alias=AliasChoices("activeSessions", "active_sessions"))


class ChannelsStatusResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    telegram: ChannelStatusEntry = Field(default_factory=ChannelStatusEntry)
    discord: ChannelStatusEntry = Field(default_factory=ChannelStatusEntry)
    slack: ChannelStatusEntry = Field(default_factory=ChannelStatusEntry)


class ChannelVerifyResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: bool
    channel: ChannelName
    checks: Optional[dict[str, Optional[bool]]] = None
    status_codes: Optional[dict[str, Optional[int]]] = Field(
        None, validation_alias=AliasChoices("statusCodes", "status_codes")
    )
    hints: Optional[list[str]] = None
    details: Optional[dict[str, Any]] = None


class ChannelToolPreferences(BaseModel):
    model_config = ConfigDict(extra="ignore")
    enabled_tools: list[str] = Field(default_factory=list)
    disabled_tools: list[str] = Field(default_factory=list)
    enabled_mcp_servers: list[str] = Field(
        default_factory=list, validation_alias=AliasChoices("enabledMcpServers", "enabled_mcp_servers")
    )


# ─── Memory ───────────────────────────────────────────────────────────────────


class MemoryItem(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: Optional[str] = None
    text: Optional[str] = Field(None, validation_alias=AliasChoices("text", "content"))
    content: Optional[str] = Field(None, validation_alias=AliasChoices("content", "text"))
    user_id: Optional[str] = Field(None, validation_alias=AliasChoices("userID", "userId", "user_id"))
    source_type: Optional[str] = Field(None, validation_alias=AliasChoices("sourceType", "source_type"))
    tags: Optional[list[str]] = None
    source: Optional[str] = None
    session_id: Optional[str] = Field(None, validation_alias=AliasChoices("sessionID", "sessionId", "session_id"))
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))


class MemoryPutResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    ok: Optional[bool] = None
    stored: Optional[bool] = None
    tier: Optional[str] = None
    partition_key: Optional[str] = Field(None, validation_alias=AliasChoices("partitionKey", "partition_key"))
    audit_id: Optional[str] = Field(None, validation_alias=AliasChoices("auditID", "audit_id"))


class MemorySearchResult(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    text: Optional[str] = Field(None, validation_alias=AliasChoices("text", "content"))
    content: Optional[str] = Field(None, validation_alias=AliasChoices("content", "text"))
    score: Optional[float] = None
    source_type: Optional[str] = Field(None, validation_alias=AliasChoices("sourceType", "source_type"))
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))
    tags: Optional[list[str]] = None


class MemorySearchResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    results: list[MemorySearchResult] = []
    count: int = 0


class MemoryListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    items: list[MemoryItem] = []
    count: int = 0


MemoryImportFormat = Literal["directory", "openclaw"]
MemoryImportTier = Literal["global", "project", "session"]


class MemoryImportSource(BaseModel):
    model_config = ConfigDict(extra="ignore")
    kind: Literal["path"] = "path"
    path: str


class MemoryImportResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: bool = False
    source: Optional[MemoryImportSource] = None
    format: MemoryImportFormat = "directory"
    tier: MemoryImportTier = "project"
    project_id: Optional[str] = None
    session_id: Optional[str] = None
    sync_deletes: bool = False
    discovered_files: int = 0
    files_processed: int = 0
    indexed_files: int = 0
    skipped_files: int = 0
    deleted_files: int = 0
    chunks_created: int = 0
    errors: int = 0


class MemoryPromoteResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: Optional[bool] = None
    id: Optional[str] = None
    promoted: Optional[bool] = None
    new_memory_id: Optional[str] = Field(None, validation_alias=AliasChoices("newMemoryId", "new_memory_id"))
    to_tier: Optional[str] = Field(None, validation_alias=AliasChoices("toTier", "to_tier"))
    audit_id: Optional[str] = Field(None, validation_alias=AliasChoices("auditID", "audit_id"))


class MemoryAuditEntry(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: Optional[str] = None
    ts_ms: Optional[int] = Field(None, validation_alias=AliasChoices("tsMs", "ts_ms"))
    action: Optional[str] = None
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))


class MemoryAuditResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    entries: list[MemoryAuditEntry] = []
    count: int = 0


# ─── Skills ───────────────────────────────────────────────────────────────────


class SkillRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    name: str
    location: Optional[SkillLocation] = None
    description: Optional[str] = None
    version: Optional[str] = None


class SkillsListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    skills: list[SkillRecord] = []
    count: int = 0


class SkillImportResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: bool
    imported: Optional[int] = None


class SkillTemplate(BaseModel):
    model_config = ConfigDict(extra="ignore")
    name: str
    description: Optional[str] = None


class SkillTemplatesResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    templates: list[SkillTemplate] = []
    count: int = 0


# ─── Resources ────────────────────────────────────────────────────────────────


class ResourceRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    key: str
    value: JsonValue = None
    rev: Optional[int] = None
    updated_at_ms: Optional[int] = Field(None, validation_alias=AliasChoices("updatedAtMs", "updated_at_ms"))
    updated_by: Optional[str] = Field(None, validation_alias=AliasChoices("updatedBy", "updated_by"))


class ResourceListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    items: list[ResourceRecord] = []
    count: int = 0


class ResourceWriteResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: bool
    rev: Optional[int] = None


# ─── Workflows ────────────────────────────────────────────────────────────────


class WorkflowRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: Optional[str] = None
    workflow_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("workflowId", "workflow_id")
    )
    name: Optional[str] = None
    description: Optional[str] = None
    enabled: Optional[bool] = None


class WorkflowListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    workflows: list[WorkflowRecord] = []
    count: int = 0


class WorkflowRunRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: Optional[str] = None
    run_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("runId", "runID", "run_id")
    )
    workflow_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("workflowId", "workflowID", "workflow_id")
    )
    status: Optional[str] = None
    created_at_ms: Optional[int] = Field(
        None, validation_alias=AliasChoices("createdAtMs", "created_at_ms")
    )
    updated_at_ms: Optional[int] = Field(
        None, validation_alias=AliasChoices("updatedAtMs", "updated_at_ms")
    )


class WorkflowRunListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    runs: list[WorkflowRunRecord] = []
    count: int = 0


class WorkflowHookRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: Optional[str] = None
    workflow_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("workflowId", "workflowID", "workflow_id")
    )
    event_type: Optional[str] = Field(
        None, validation_alias=AliasChoices("eventType", "event_type")
    )
    enabled: Optional[bool] = None


class WorkflowHookListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    hooks: list[WorkflowHookRecord] = []
    count: int = 0


# ─── Bug Monitor ──────────────────────────────────────────────────────────────


class BugMonitorConfigRow(BaseModel):
    model_config = ConfigDict(extra="allow")
    enabled: Optional[bool] = None
    paused: Optional[bool] = None
    workspace_root: Optional[str] = None
    repo: Optional[str] = None
    mcp_server: Optional[str] = None
    provider_preference: Optional[str] = None
    model_policy: Optional[dict[str, Any]] = None
    auto_create_new_issues: Optional[bool] = None
    require_approval_for_new_issues: Optional[bool] = None
    auto_comment_on_matched_open_issues: Optional[bool] = None
    label_mode: Optional[str] = None


class BugMonitorConfigResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    bug_monitor: BugMonitorConfigRow


class BugMonitorStatusRow(BaseModel):
    model_config = ConfigDict(extra="allow")
    config: Optional[BugMonitorConfigRow] = None
    readiness: Optional[dict[str, bool]] = None
    runtime: Optional[dict[str, Any]] = None
    required_capabilities: Optional[dict[str, bool]] = None
    missing_required_capabilities: list[str] = []
    resolved_capabilities: list[dict[str, Any]] = []
    discovered_mcp_tools: list[str] = []
    selected_server_binding_candidates: list[dict[str, Any]] = []
    binding_source_version: Optional[str] = None
    bindings_last_merged_at_ms: Optional[int] = None
    selected_model: Optional[dict[str, Any]] = None
    pending_drafts: Optional[int] = None
    pending_posts: Optional[int] = None
    last_activity_at_ms: Optional[int] = None
    last_error: Optional[str] = None


class BugMonitorStatusResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    status: BugMonitorStatusRow


class BugMonitorIncidentRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    incident_id: str
    fingerprint: Optional[str] = None
    event_type: Optional[str] = None
    status: Optional[str] = None
    repo: Optional[str] = None
    workspace_root: Optional[str] = None
    title: Optional[str] = None
    detail: Optional[str] = None
    excerpt: Optional[list[str]] = None
    occurrence_count: Optional[int] = None
    created_at_ms: Optional[int] = None
    updated_at_ms: Optional[int] = None
    draft_id: Optional[str] = None
    triage_run_id: Optional[str] = None
    last_error: Optional[str] = None


class BugMonitorIncidentListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    incidents: list[BugMonitorIncidentRecord] = []
    count: int = 0


class BugMonitorDraftRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    draft_id: str
    fingerprint: Optional[str] = None
    repo: Optional[str] = None
    status: Optional[str] = None
    created_at_ms: Optional[int] = None
    triage_run_id: Optional[str] = None
    issue_number: Optional[int] = None
    title: Optional[str] = None
    detail: Optional[str] = None
    github_status: Optional[str] = None
    github_issue_url: Optional[str] = None
    github_comment_url: Optional[str] = None
    github_posted_at_ms: Optional[int] = None
    matched_issue_number: Optional[int] = None
    matched_issue_state: Optional[str] = None
    evidence_digest: Optional[str] = None
    last_post_error: Optional[str] = None


class BugMonitorDraftListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    drafts: list[BugMonitorDraftRecord] = []
    count: int = 0


class BugMonitorPostRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    post_id: str
    draft_id: Optional[str] = None
    repo: Optional[str] = None
    operation: Optional[str] = None
    status: Optional[str] = None
    issue_number: Optional[int] = None
    issue_url: Optional[str] = None
    comment_url: Optional[str] = None
    error: Optional[str] = None
    updated_at_ms: Optional[int] = None


class BugMonitorPostListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    posts: list[BugMonitorPostRecord] = []
    count: int = 0


# ─── Routines & Automations ───────────────────────────────────────────────────


class RoutineRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    name: Optional[str] = None
    schedule: Optional[Any] = None
    entrypoint: Optional[str] = None
    status: Optional[RoutineStatus] = None
    requires_approval: Optional[bool] = Field(None, validation_alias=AliasChoices("requiresApproval", "requires_approval"))
    external_integrations_allowed: Optional[bool] = Field(None, validation_alias=AliasChoices("externalIntegrationsAllowed", "external_integrations_allowed"))
    last_run: Optional[str] = Field(None, validation_alias=AliasChoices("lastRun", "last_run"))


class DefinitionListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    routines: Optional[list[RoutineRecord]] = None
    automations: Optional[list[RoutineRecord]] = None
    count: int = 0


class DefinitionCreateResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    routine: Optional[RoutineRecord] = None
    automation: Optional[RoutineRecord] = None


class RunNowResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: Optional[bool] = None
    dry_run: Optional[bool] = None
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))
    status: Optional[RunStatus] = None


class ArtifactRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    artifact_id: Optional[str] = Field(None, validation_alias=AliasChoices("artifactId", "artifact_id"))
    uri: str
    kind: str
    label: Optional[str] = None
    created_at_ms: Optional[int] = Field(None, validation_alias=AliasChoices("createdAtMs", "created_at_ms"))


class RunArtifactsResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))
    artifacts: list[ArtifactRecord] = []
    count: int = 0


class RoutineHistoryEntry(BaseModel):
    model_config = ConfigDict(extra="ignore")
    event: Optional[str] = None
    ts_ms: Optional[int] = Field(None, validation_alias=AliasChoices("tsMs", "ts_ms"))
    status: Optional[RoutineStatus] = None


class RoutineHistoryResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    history: list[RoutineHistoryEntry] = []
    count: int = 0


class RunRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: Optional[str] = None
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))
    routine_id: Optional[str] = Field(None, validation_alias=AliasChoices("routineId", "routine_id"))
    automation_id: Optional[str] = Field(None, validation_alias=AliasChoices("automationId", "automation_id"))
    status: Optional[RunStatus] = None
    started_at_ms: Optional[int] = Field(None, validation_alias=AliasChoices("startedAtMs", "started_at_ms"))
    finished_at_ms: Optional[int] = Field(None, validation_alias=AliasChoices("finishedAtMs", "finished_at_ms"))


# ─── Coder ────────────────────────────────────────────────────────────────────


class CoderRepoBinding(BaseModel):
    model_config = ConfigDict(extra="ignore")
    project_id: Optional[str] = Field(None, validation_alias=AliasChoices("projectId", "project_id"))
    workspace_id: Optional[str] = Field(None, validation_alias=AliasChoices("workspaceId", "workspace_id"))
    workspace_root: Optional[str] = Field(None, validation_alias=AliasChoices("workspaceRoot", "workspace_root"))
    repo_slug: str = Field(validation_alias=AliasChoices("repoSlug", "repo_slug"))
    default_branch: Optional[str] = Field(None, validation_alias=AliasChoices("defaultBranch", "default_branch"))


class CoderGithubRef(BaseModel):
    model_config = ConfigDict(extra="ignore")
    kind: str
    number: int
    url: Optional[str] = None


class CoderGithubProjectStatusOption(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    name: str


class CoderGithubProjectStatusMapping(BaseModel):
    model_config = ConfigDict(extra="ignore")
    field_id: Optional[str] = Field(None, validation_alias=AliasChoices("fieldId", "field_id"))
    field_name: Optional[str] = Field(None, validation_alias=AliasChoices("fieldName", "field_name"))
    todo: CoderGithubProjectStatusOption
    in_progress: CoderGithubProjectStatusOption = Field(
        validation_alias=AliasChoices("inProgress", "in_progress")
    )
    in_review: CoderGithubProjectStatusOption = Field(
        validation_alias=AliasChoices("inReview", "in_review")
    )
    blocked: CoderGithubProjectStatusOption
    done: CoderGithubProjectStatusOption


class CoderGithubProjectBinding(BaseModel):
    model_config = ConfigDict(extra="allow")
    owner: str
    project_number: int = Field(validation_alias=AliasChoices("projectNumber", "project_number"))
    repo_slug: Optional[str] = Field(None, validation_alias=AliasChoices("repoSlug", "repo_slug"))
    mcp_server: Optional[str] = Field(None, validation_alias=AliasChoices("mcpServer", "mcp_server"))
    schema_snapshot: Optional[dict[str, Any]] = Field(
        None, validation_alias=AliasChoices("schemaSnapshot", "schema_snapshot")
    )
    schema_fingerprint: str = Field(
        validation_alias=AliasChoices("schemaFingerprint", "schema_fingerprint")
    )
    status_mapping: CoderGithubProjectStatusMapping = Field(
        validation_alias=AliasChoices("statusMapping", "status_mapping")
    )


class CoderGithubProjectRef(BaseModel):
    model_config = ConfigDict(extra="ignore")
    owner: str
    project_number: int = Field(validation_alias=AliasChoices("projectNumber", "project_number"))
    project_item_id: str = Field(validation_alias=AliasChoices("projectItemId", "project_item_id"))
    issue_number: int = Field(validation_alias=AliasChoices("issueNumber", "issue_number"))
    issue_url: Optional[str] = Field(None, validation_alias=AliasChoices("issueUrl", "issue_url"))
    schema_fingerprint: str = Field(
        validation_alias=AliasChoices("schemaFingerprint", "schema_fingerprint")
    )
    status_mapping: CoderGithubProjectStatusMapping = Field(
        validation_alias=AliasChoices("statusMapping", "status_mapping")
    )


class CoderProjectBindingRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    project_id: str = Field(validation_alias=AliasChoices("projectId", "project_id"))
    repo_binding: CoderRepoBinding = Field(validation_alias=AliasChoices("repoBinding", "repo_binding"))
    github_project_binding: Optional[CoderGithubProjectBinding] = Field(
        None, validation_alias=AliasChoices("githubProjectBinding", "github_project_binding")
    )
    updated_at_ms: int = Field(validation_alias=AliasChoices("updatedAtMs", "updated_at_ms"))


class CoderLinkedProjectRun(BaseModel):
    model_config = ConfigDict(extra="ignore")
    coder_run: Optional["CoderRunRecord"] = Field(
        None, validation_alias=AliasChoices("coderRun", "coder_run")
    )
    active: bool = False


class CoderGithubProjectInboxIssue(BaseModel):
    model_config = ConfigDict(extra="ignore")
    number: int
    title: str
    html_url: Optional[str] = Field(None, validation_alias=AliasChoices("htmlUrl", "html_url"))


class CoderGithubProjectInboxItem(BaseModel):
    model_config = ConfigDict(extra="ignore")
    project_item_id: str = Field(validation_alias=AliasChoices("projectItemId", "project_item_id"))
    title: str
    status_name: str = Field(validation_alias=AliasChoices("statusName", "status_name"))
    status_option_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("statusOptionId", "status_option_id")
    )
    issue: Optional[CoderGithubProjectInboxIssue] = None
    actionable: bool
    unsupported_reason: Optional[str] = Field(
        None, validation_alias=AliasChoices("unsupportedReason", "unsupported_reason")
    )
    linked_run: Optional[CoderLinkedProjectRun] = Field(
        None, validation_alias=AliasChoices("linkedRun", "linked_run")
    )
    remote_sync_state: Optional[str] = Field(
        None, validation_alias=AliasChoices("remoteSyncState", "remote_sync_state")
    )


class CoderRunRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    coder_run_id: Optional[str] = Field(None, validation_alias=AliasChoices("coderRunId", "coder_run_id"))
    workflow_mode: Optional[str] = Field(None, validation_alias=AliasChoices("workflowMode", "workflow_mode"))
    linked_context_run_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("linkedContextRunId", "linked_context_run_id")
    )
    repo_binding: Optional[CoderRepoBinding] = Field(
        None, validation_alias=AliasChoices("repoBinding", "repo_binding")
    )
    github_ref: Optional[CoderGithubRef] = Field(
        None, validation_alias=AliasChoices("githubRef", "github_ref")
    )
    github_project_ref: Optional[CoderGithubProjectRef] = Field(
        None, validation_alias=AliasChoices("githubProjectRef", "github_project_ref")
    )
    remote_sync_state: Optional[str] = Field(
        None, validation_alias=AliasChoices("remoteSyncState", "remote_sync_state")
    )
    source_client: Optional[str] = Field(None, validation_alias=AliasChoices("sourceClient", "source_client"))
    status: Optional[str] = None
    phase: Optional[str] = None
    created_at_ms: Optional[int] = Field(None, validation_alias=AliasChoices("createdAtMs", "created_at_ms"))
    updated_at_ms: Optional[int] = Field(None, validation_alias=AliasChoices("updatedAtMs", "updated_at_ms"))


class CoderRunsListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    runs: list[CoderRunRecord] = []
    count: int = 0


class CoderRunGetResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    coder_run: Optional[CoderRunRecord] = Field(
        None, validation_alias=AliasChoices("coderRun", "coder_run")
    )
    run: Optional[dict[str, Any]] = None


class CoderProjectBindingGetResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    binding: Optional[CoderProjectBindingRecord] = None


class CoderProjectBindingPutResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: Optional[bool] = None
    binding: CoderProjectBindingRecord


class CoderGithubProjectInboxResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    project_id: str = Field(validation_alias=AliasChoices("projectId", "project_id"))
    binding: CoderGithubProjectBinding
    schema_drift: bool = Field(validation_alias=AliasChoices("schemaDrift", "schema_drift"))
    live_schema_fingerprint: str = Field(
        validation_alias=AliasChoices("liveSchemaFingerprint", "live_schema_fingerprint")
    )
    items: list[CoderGithubProjectInboxItem] = []


class CoderGithubProjectIntakeResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    ok: Optional[bool] = None
    deduped: Optional[bool] = None
    coder_run: Optional[CoderRunRecord] = Field(
        None, validation_alias=AliasChoices("coderRun", "coder_run")
    )
    run: Optional[dict[str, Any]] = None


class CoderArtifactRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: str
    ts_ms: Optional[int] = Field(None, validation_alias=AliasChoices("tsMs", "ts_ms"))
    path: str
    artifact_type: Optional[str] = Field(None, validation_alias=AliasChoices("artifactType", "artifact_type"))
    step_id: Optional[str] = Field(None, validation_alias=AliasChoices("stepId", "step_id"))
    source_event_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("sourceEventId", "source_event_id")
    )


class CoderArtifactsResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    artifacts: list[CoderArtifactRecord] = []
    count: int = 0


class CoderMemoryHitRecord(BaseModel):
    model_config = ConfigDict(extra="allow")


class CoderMemoryHitsResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    hits: list[CoderMemoryHitRecord] = []
    count: int = 0


class CoderMemoryCandidateRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    candidate_id: Optional[str] = Field(None, validation_alias=AliasChoices("candidateId", "candidate_id"))
    kind: Optional[str] = None
    summary: Optional[str] = None
    payload: Optional[dict[str, Any]] = None
    artifact: Optional[CoderArtifactRecord] = None
    created_at_ms: Optional[int] = Field(None, validation_alias=AliasChoices("createdAtMs", "created_at_ms"))


class CoderMemoryCandidatesResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    candidates: list[CoderMemoryCandidateRecord] = []
    count: int = 0


# ─── Agent Teams ──────────────────────────────────────────────────────────────


class AgentTeamTemplate(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: str
    name: Optional[str] = None
    role: Optional[str] = None


class AgentTeamTemplatesResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    templates: list[AgentTeamTemplate] = []
    count: int = 0


class AgentTeamTemplateWriteResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: Optional[bool] = None
    template: Optional[dict[str, Any]] = None
    deleted: Optional[bool] = None
    template_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("templateID", "templateId", "template_id")
    )


class AgentTeamInstance(BaseModel):
    model_config = ConfigDict(extra="ignore")
    instance_id: Optional[str] = Field(None, validation_alias=AliasChoices("instanceID", "instanceId", "instance_id"))
    mission_id: Optional[str] = Field(None, validation_alias=AliasChoices("missionID", "missionId", "mission_id"))
    role: Optional[str] = None
    status: Optional[str] = None
    session_id: Optional[str] = Field(None, validation_alias=AliasChoices("sessionID", "sessionId", "session_id"))


class AgentTeamInstancesResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    instances: list[AgentTeamInstance] = []
    count: int = 0


class AgentTeamMissionsResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    missions: list[dict[str, Any]] = []
    count: int = 0


class AgentTeamSpawnApproval(BaseModel):
    model_config = ConfigDict(extra="ignore")
    approval_id: Optional[str] = Field(None, validation_alias=AliasChoices("approvalID", "approvalId", "approval_id"))
    status: Optional[ApprovalStatus] = None


class AgentTeamApprovalsResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    spawn_approvals: list[AgentTeamSpawnApproval] = Field(default_factory=list, validation_alias=AliasChoices("spawnApprovals", "spawn_approvals"))
    tool_approvals: list[dict[str, Any]] = Field(default_factory=list, validation_alias=AliasChoices("toolApprovals", "tool_approvals"))
    count: int = 0


class AgentTeamSpawnResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    ok: Optional[bool] = None
    mission_id: Optional[str] = Field(None, validation_alias=AliasChoices("missionID", "missionId", "mission_id"))
    instance_id: Optional[str] = Field(None, validation_alias=AliasChoices("instanceID", "instanceId", "instance_id"))
    session_id: Optional[str] = Field(None, validation_alias=AliasChoices("sessionID", "sessionId", "session_id"))
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))
    status: Optional[str] = None
    code: Optional[str] = None
    error: Optional[str] = None


# ─── Automations V2 ───────────────────────────────────────────────────────────


AutomationV2Status = Literal["active", "paused", "draft"]
AutomationV2RunStatus = Literal[
    "queued",
    "running",
    "pausing",
    "paused",
    "completed",
    "failed",
    "cancelled",
]


class AutomationV2Record(BaseModel):
    model_config = ConfigDict(extra="allow")
    automation_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("automationID", "automationId", "automation_id", "id")
    )
    name: Optional[str] = None
    description: Optional[str] = None
    status: Optional[AutomationV2Status] = None


class AutomationV2ListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    automations: list[AutomationV2Record] = []
    count: int = 0


class AutomationV2RunRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))
    automation_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("automationID", "automationId", "automation_id")
    )
    status: Optional[AutomationV2RunStatus] = None


class AutomationV2RunListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    runs: list[AutomationV2RunRecord] = []
    count: int = 0


# ─── Workflow Plans ───────────────────────────────────────────────────────────


class WorkflowPlanInputRef(BaseModel):
    model_config = ConfigDict(extra="allow")
    from_step_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("fromStepId", "from_step_id")
    )
    alias: str


class WorkflowPlanOutputContract(BaseModel):
    model_config = ConfigDict(extra="allow")
    kind: Optional[str] = None


class WorkflowPlanStep(BaseModel):
    model_config = ConfigDict(extra="allow")
    step_id: Optional[str] = Field(None, validation_alias=AliasChoices("stepId", "step_id"))
    kind: str
    objective: str
    depends_on: list[str] = Field(
        default_factory=list, validation_alias=AliasChoices("dependsOn", "depends_on")
    )
    agent_role: Optional[str] = Field(None, validation_alias=AliasChoices("agentRole", "agent_role"))
    input_refs: list[WorkflowPlanInputRef] = Field(
        default_factory=list, validation_alias=AliasChoices("inputRefs", "input_refs")
    )
    output_contract: Optional[WorkflowPlanOutputContract] = Field(
        None, validation_alias=AliasChoices("outputContract", "output_contract")
    )


class WorkflowPlan(BaseModel):
    model_config = ConfigDict(extra="allow")
    plan_id: Optional[str] = Field(None, validation_alias=AliasChoices("planId", "plan_id"))
    planner_version: Optional[str] = Field(
        None, validation_alias=AliasChoices("plannerVersion", "planner_version")
    )
    plan_source: Optional[str] = Field(None, validation_alias=AliasChoices("planSource", "plan_source"))
    original_prompt: Optional[str] = Field(
        None, validation_alias=AliasChoices("originalPrompt", "original_prompt")
    )
    normalized_prompt: Optional[str] = Field(
        None, validation_alias=AliasChoices("normalizedPrompt", "normalized_prompt")
    )
    confidence: Optional[str] = None
    title: str
    description: Optional[str] = None
    schedule: Optional[dict[str, Any]] = None
    execution_target: Optional[str] = Field(
        None, validation_alias=AliasChoices("executionTarget", "execution_target")
    )
    workspace_root: Optional[str] = Field(
        None, validation_alias=AliasChoices("workspaceRoot", "workspace_root")
    )
    steps: list[WorkflowPlanStep] = []
    allowed_mcp_servers: list[str] = Field(
        default_factory=list,
        validation_alias=AliasChoices("allowedMcpServers", "allowed_mcp_servers"),
    )
    operator_preferences: Optional[dict[str, Any]] = Field(
        None, validation_alias=AliasChoices("operatorPreferences", "operator_preferences")
    )
    metadata: Optional[dict[str, Any]] = None


class WorkflowPlanPackBuilderExportRequest(BaseModel):
    model_config = ConfigDict(extra="allow")
    enabled: Optional[bool] = None
    session_id: Optional[str] = Field(None, validation_alias=AliasChoices("sessionId", "session_id"))
    thread_key: Optional[str] = Field(None, validation_alias=AliasChoices("threadKey", "thread_key"))
    auto_apply: Optional[bool] = Field(None, validation_alias=AliasChoices("autoApply", "auto_apply"))


class WorkflowPlanPackBuilderExportResult(BaseModel):
    model_config = ConfigDict(extra="allow")
    status: Optional[str] = None
    error: Optional[str] = None
    http_status: Optional[int] = Field(None, validation_alias=AliasChoices("httpStatus", "http_status"))
    plan_id: Optional[str] = Field(None, validation_alias=AliasChoices("planId", "plan_id"))
    session_id: Optional[str] = Field(None, validation_alias=AliasChoices("sessionId", "session_id"))
    thread_key: Optional[str] = Field(None, validation_alias=AliasChoices("threadKey", "thread_key"))
    auto_apply_requested: Optional[bool] = Field(
        None, validation_alias=AliasChoices("autoApplyRequested", "auto_apply_requested")
    )
    auto_apply_ready: Optional[bool] = Field(
        None, validation_alias=AliasChoices("autoApplyReady", "auto_apply_ready")
    )


class WorkflowPlanChatMessage(BaseModel):
    model_config = ConfigDict(extra="allow")
    role: str
    text: str
    created_at_ms: Optional[int] = Field(
        None, validation_alias=AliasChoices("createdAtMs", "created_at_ms")
    )


class WorkflowPlanConversation(BaseModel):
    model_config = ConfigDict(extra="allow")
    conversation_id: Optional[str] = Field(
        None, validation_alias=AliasChoices("conversationId", "conversation_id")
    )
    plan_id: Optional[str] = Field(None, validation_alias=AliasChoices("planId", "plan_id"))
    created_at_ms: Optional[int] = Field(
        None, validation_alias=AliasChoices("createdAtMs", "created_at_ms")
    )
    updated_at_ms: Optional[int] = Field(
        None, validation_alias=AliasChoices("updatedAtMs", "updated_at_ms")
    )
    messages: list[WorkflowPlanChatMessage] = []


class WorkflowPlanPreviewResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    plan: WorkflowPlan
    clarifier: Optional[dict[str, Any]] = None
    assistant_message: Optional[dict[str, Any]] = None
    planner_diagnostics: Optional[dict[str, Any]] = None
    plan_package: Optional[dict[str, Any]] = None
    plan_package_bundle: Optional[dict[str, Any]] = None
    plan_package_validation: Optional[dict[str, Any]] = None
    overlap_analysis: Optional[dict[str, Any]] = None
    teaching_library: Optional[dict[str, Any]] = None


class WorkflowPlanApplyResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    ok: Optional[bool] = None
    plan: Optional[WorkflowPlan] = None
    automation: Optional[dict[str, Any]] = None
    plan_package: Optional[dict[str, Any]] = None
    plan_package_bundle: Optional[dict[str, Any]] = None
    plan_package_validation: Optional[dict[str, Any]] = None
    approved_plan_materialization: Optional[dict[str, Any]] = None
    overlap_analysis: Optional[dict[str, Any]] = None
    pack_builder_export: Optional[WorkflowPlanPackBuilderExportResult] = Field(
        None, validation_alias=AliasChoices("packBuilderExport", "pack_builder_export")
    )


class WorkflowPlanChatResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    plan: WorkflowPlan
    conversation: WorkflowPlanConversation
    assistant_message: Optional[dict[str, Any]] = None
    change_summary: list[str] = Field(
        default_factory=list, validation_alias=AliasChoices("changeSummary", "change_summary")
    )
    clarifier: Optional[dict[str, Any]] = None
    planner_diagnostics: Optional[dict[str, Any]] = None
    plan_package: Optional[dict[str, Any]] = None
    plan_package_bundle: Optional[dict[str, Any]] = None
    plan_package_validation: Optional[dict[str, Any]] = None
    overlap_analysis: Optional[dict[str, Any]] = None
    teaching_library: Optional[dict[str, Any]] = None


class WorkflowPlanGetResponse(WorkflowPlanChatResponse):
    model_config = ConfigDict(extra="allow")
    plan_package_replay: Optional[dict[str, Any]] = None


class WorkflowPlanImportPreviewResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    ok: Optional[bool] = None
    bundle: Optional[dict[str, Any]] = None
    import_validation: Optional[dict[str, Any]] = None
    plan_package_preview: Optional[dict[str, Any]] = None
    plan_package_validation: Optional[dict[str, Any]] = None
    derived_scope_snapshot: Optional[dict[str, Any]] = None
    summary: Optional[dict[str, Any]] = None
    import_transform_log: Optional[list[dict[str, Any]]] = None
    import_source_bundle_digest: Optional[str] = None


class WorkflowPlanImportResponse(WorkflowPlanImportPreviewResponse):
    model_config = ConfigDict(extra="allow")


# ─── Missions ─────────────────────────────────────────────────────────────────


class MissionRecord(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: Optional[str] = None
    title: Optional[str] = None
    goal: Optional[str] = None
    status: Optional[str] = None


class MissionCreateResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    mission: Optional[MissionRecord] = None


class MissionListResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    missions: list[MissionRecord] = []
    count: int = 0


class MissionGetResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    mission: MissionRecord


class MissionEventResponse(BaseModel):
    model_config = ConfigDict(extra="ignore")
    mission: Optional[MissionRecord] = None
    commands: Optional[list[Any]] = None


# ─── Tools ────────────────────────────────────────────────────────────────────


class ToolSchema(BaseModel):
    model_config = ConfigDict(extra="ignore")
    name: str
    description: Optional[str] = None
    input_schema: Optional[dict[str, Any]] = Field(None, validation_alias=AliasChoices("inputSchema", "input_schema"))


class ToolExecuteResult(BaseModel):
    model_config = ConfigDict(extra="ignore")
    output: Optional[str] = None
    metadata: Optional[dict[str, Any]] = None


# ─── SSE events (Discriminated Union) ─────────────────────────────────────────

class EngineEventBase(BaseModel):
    properties: dict[str, Any] = Field(default_factory=dict)
    session_id: Optional[str] = Field(None, validation_alias=AliasChoices("sessionID", "sessionId", "session_id"))
    run_id: Optional[str] = Field(None, validation_alias=AliasChoices("runID", "runId", "run_id"))
    timestamp: Optional[str] = None

class RunStartedEvent(EngineEventBase):
    type: Literal["run.started"]

class RunProgressEvent(EngineEventBase):
    type: Literal["run.progress"]

class RunCompletedEvent(EngineEventBase):
    type: Literal["run.completed"]

class RunFailedEvent(EngineEventBase):
    type: Literal["run.failed"]

class ToolCalledEvent(EngineEventBase):
    type: Literal["tool.called"]

class ToolResultEvent(EngineEventBase):
    type: Literal["tool.result"]

class ApprovalRequestedEvent(EngineEventBase):
    type: Literal["approval.requested"]

class ApprovalResolvedEvent(EngineEventBase):
    type: Literal["approval.resolved"]

class RoutineTriggeredEvent(EngineEventBase):
    type: Literal["routine.triggered"]

class RoutineCompletedEvent(EngineEventBase):
    type: Literal["routine.completed"]

class SessionResponseEvent(EngineEventBase):
    type: Literal["session.response"]

class UnknownEvent(EngineEventBase):
    model_config = ConfigDict(extra="allow")
    type: str

EngineEvent = Union[
    RunStartedEvent,
    RunProgressEvent,
    RunCompletedEvent,
    RunFailedEvent,
    ToolCalledEvent,
    ToolResultEvent,
    ApprovalRequestedEvent,
    ApprovalResolvedEvent,
    RoutineTriggeredEvent,
    RoutineCompletedEvent,
    SessionResponseEvent,
    UnknownEvent,
]
