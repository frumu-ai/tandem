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
    allowed_users: Optional[list[str]] = Field(None, validation_alias=AliasChoices("allowedUsers", "allowed_users"))
    mention_only: Optional[bool] = Field(None, validation_alias=AliasChoices("mentionOnly", "mention_only"))
    guild_id: Optional[str] = Field(None, validation_alias=AliasChoices("guildId", "guild_id"))
    channel_id: Optional[str] = Field(None, validation_alias=AliasChoices("channelId", "channel_id"))


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
