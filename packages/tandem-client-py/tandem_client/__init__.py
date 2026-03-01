"""
tandem_client — Python client for the Tandem autonomous agent engine.

Full coverage of the Tandem HTTP + SSE API.

Async (recommended)::

    from tandem_client import TandemClient

    async with TandemClient(base_url="http://localhost:39731", token="...") as client:
        session_id = await client.sessions.create(title="My agent")
        run = await client.sessions.prompt_async(session_id, "Summarize README.md")
        async for event in client.stream(session_id, run.run_id):
            if event.type == "session.response":
                print(event.properties.get("delta", ""), end="", flush=True)
            if event.type in ("run.complete", "run.completed", "run.failed", "session.run.finished"):
                break

Sync (scripts)::

    from tandem_client import SyncTandemClient

    client = SyncTandemClient(base_url="http://localhost:39731", token="...")
    session_id = client.sessions.create(title="My agent")
"""

from .client import PromptAsyncResult, SyncTandemClient, TandemClient
from .stream import is_run_terminal_event
from .types import (
    AgentTeamApprovalsResponse,
    AgentTeamInstance,
    AgentTeamInstancesResponse,
    AgentTeamMissionsResponse,
    AgentTeamSpawnApproval,
    AgentTeamSpawnResponse,
    AgentTeamTemplate,
    AgentTeamTemplatesResponse,
    AgentTeamTemplateWriteResponse,
    ArtifactRecord,
    AutomationV2ListResponse,
    AutomationV2Record,
    AutomationV2RunListResponse,
    AutomationV2RunRecord,
    ChannelConfigEntry,
    ChannelStatusEntry,
    ChannelsConfigResponse,
    ChannelsStatusResponse,
    DefinitionCreateResponse,
    DefinitionListResponse,
    EngineEvent,
    EngineMessage,
    MemoryAuditEntry,
    MemoryAuditResponse,
    MemoryItem,
    MemoryListResponse,
    MemoryPromoteResponse,
    MemoryPutResponse,
    MemorySearchResponse,
    MemorySearchResult,
    MessagePart,
    MissionCreateResponse,
    MissionEventResponse,
    MissionGetResponse,
    MissionListResponse,
    MissionRecord,
    PermissionRequestRecord,
    PermissionRule,
    PermissionSnapshotResponse,
    ProviderCatalog,
    ProviderConfigEntry,
    ProviderEntry,
    ProviderModelEntry,
    ProvidersConfigResponse,
    PromptFilePartInput,
    PromptPartInput,
    PromptTextPartInput,
    QuestionRecord,
    QuestionsListResponse,
    ResourceListResponse,
    ResourceRecord,
    ResourceWriteResponse,
    RoutineHistoryEntry,
    RoutineHistoryResponse,
    RoutineRecord,
    RunArtifactsResponse,
    RunNowResponse,
    RunRecord,
    SessionDiff,
    SessionListResponse,
    SessionRecord,
    SessionRunState,
    SessionRunStateResponse,
    SessionTodo,
    SkillImportResponse,
    SkillRecord,
    SkillsListResponse,
    SkillTemplate,
    SkillTemplatesResponse,
    SystemHealth,
    ToolExecuteResult,
    ToolSchema,
)

__all__ = [
    # Clients
    "TandemClient",
    "SyncTandemClient",
    "PromptAsyncResult",
    "PromptPartInput",
    "PromptTextPartInput",
    "PromptFilePartInput",
    # Health
    "SystemHealth",
    # Sessions
    "SessionRecord",
    "SessionListResponse",
    "SessionRunState",
    "SessionRunStateResponse",
    "SessionDiff",
    "SessionTodo",
    # Messages
    "EngineMessage",
    "MessagePart",
    # Permissions
    "PermissionRule",
    "PermissionRequestRecord",
    "PermissionSnapshotResponse",
    # Questions
    "QuestionRecord",
    "QuestionsListResponse",
    # Providers
    "ProviderEntry",
    "ProviderModelEntry",
    "ProviderCatalog",
    "ProviderConfigEntry",
    "ProvidersConfigResponse",
    # Channels
    "ChannelConfigEntry",
    "ChannelStatusEntry",
    "ChannelsConfigResponse",
    "ChannelsStatusResponse",
    # Memory
    "MemoryItem",
    "MemoryPutResponse",
    "MemorySearchResult",
    "MemorySearchResponse",
    "MemoryListResponse",
    "MemoryPromoteResponse",
    "MemoryAuditEntry",
    "MemoryAuditResponse",
    # Skills
    "SkillRecord",
    "SkillsListResponse",
    "SkillImportResponse",
    "SkillTemplate",
    "SkillTemplatesResponse",
    # Resources
    "ResourceRecord",
    "ResourceListResponse",
    "ResourceWriteResponse",
    # Routines & Automations
    "RoutineRecord",
    "DefinitionListResponse",
    "DefinitionCreateResponse",
    "RunNowResponse",
    "RunRecord",
    "RunArtifactsResponse",
    "RoutineHistoryEntry",
    "RoutineHistoryResponse",
    # Agent Teams
    "AgentTeamTemplate",
    "AgentTeamTemplatesResponse",
    "AgentTeamTemplateWriteResponse",
    "AgentTeamInstance",
    "AgentTeamInstancesResponse",
    "AgentTeamMissionsResponse",
    "AgentTeamSpawnApproval",
    "AgentTeamApprovalsResponse",
    "AgentTeamSpawnResponse",
    # Automations V2
    "AutomationV2Record",
    "AutomationV2ListResponse",
    "AutomationV2RunRecord",
    "AutomationV2RunListResponse",
    # Missions
    "MissionRecord",
    "MissionCreateResponse",
    "MissionListResponse",
    "MissionGetResponse",
    "MissionEventResponse",
    # Tools
    "ToolSchema",
    "ToolExecuteResult",
    # Events
    "EngineEvent",
    "is_run_terminal_event",
    "ArtifactRecord",
]
