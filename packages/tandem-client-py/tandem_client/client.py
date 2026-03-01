"""Main TandemClient class — full parity with the Tandem engine HTTP + SSE API."""
from __future__ import annotations

import asyncio
import json as _json
from typing import Any, AsyncGenerator, Optional
from urllib.parse import quote

import httpx

from pydantic import BaseModel, TypeAdapter

from .stream import stream_sse
from .types import (
    AgentTeamApprovalsResponse,
    AgentTeamInstancesResponse,
    AgentTeamMissionsResponse,
    AgentTeamSpawnResponse,
    AgentTeamTemplatesResponse,
    AgentTeamTemplateWriteResponse,
    AutomationV2ListResponse,
    AutomationV2Record,
    AutomationV2RunListResponse,
    AutomationV2RunRecord,
    ArtifactRecord,
    ChannelsConfigResponse,
    ChannelsStatusResponse,
    DefinitionCreateResponse,
    DefinitionListResponse,
    EngineEvent,
    EngineMessage,
    MemoryAuditResponse,
    MemoryItem,
    MemoryListResponse,
    MemoryPromoteResponse,
    MemoryPutResponse,
    MemorySearchResponse,
    MissionCreateResponse,
    MissionGetResponse,
    MissionEventResponse,
    MissionListResponse,
    MissionRecord,
    PermissionSnapshotResponse,
    PromptPartInput,
    PromptAsyncResult,
    PromptTextPartInput,
    ProviderCatalog,
    ProvidersConfigResponse,
    IdentityConfig,
    IdentityConfigResponse,
    QuestionsListResponse,
    QuestionRecord,
    ResourceListResponse,
    ResourceWriteResponse,
    RoutineHistoryResponse,
    RoutineRecord,
    RunArtifactsResponse,
    RunNowResponse,
    RunRecord,
    SessionDiff,
    SessionListResponse,
    SessionRecord,
    SessionRunStateResponse,
    SessionTodo,
    SkillImportResponse,
    SkillsListResponse,
    SkillRecord,
    SkillTemplatesResponse,
    SystemHealth,
    ToolExecuteResult,
    ToolSchema,
    ToolMode,
    ContextMode,
)

_engine_event_adapter = TypeAdapter(EngineEvent)

class TandemClient:
    """
    Async HTTP client for the Tandem autonomous agent engine.

    Provides full coverage of the Tandem engine HTTP + SSE API.

    Use as an async context manager::

        async with TandemClient(base_url="http://localhost:39731", token="...") as client:
            session_id = await client.sessions.create(title="My agent")
            run = await client.sessions.prompt_async(session_id, "Summarize README.md")
            async for event in client.stream(session_id, run.run_id):
                if event.type == "session.response":
                    print(event.properties.get("delta", ""), end="", flush=True)
                if event.type in ("run.complete", "run.completed", "run.failed", "session.run.finished"):
                    break

    Or construct manually and call ``await client.aclose()`` when done.
    """

    def __init__(self, base_url: str, token: str, *, timeout: float = 20.0) -> None:
        self._base_url = base_url.rstrip("/")
        self._token = token
        self._http = httpx.AsyncClient(
            base_url=self._base_url,
            headers={"Authorization": f"Bearer {token}"},
            timeout=timeout,
        )
        self.sessions = _Sessions(self._base_url, self._token, self._http)
        self.permissions = _Permissions(self._http)
        self.questions = _Questions(self._http)
        self.providers = _Providers(self._http)
        self.identity = _Identity(self._http)
        self.channels = _Channels(self._http)
        self.mcp = _Mcp(self._http)
        self.routines = _Routines(self._http)
        self.automations = _Automations(self._http)
        self.automations_v2 = _AutomationsV2(self._http)
        self.memory = _Memory(self._http)
        self.skills = _Skills(self._http)
        self.resources = _Resources(self._http)
        self.agent_teams = _AgentTeams(self._http)
        self.missions = _Missions(self._http)

    async def __aenter__(self) -> "TandemClient":
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.aclose()

    async def aclose(self) -> None:
        """Close the underlying HTTP client."""
        await self._http.aclose()

    # ─── Health ───────────────────────────────────────────────────────────────

    async def health(self) -> SystemHealth:
        """Check engine health. Ready when ``SystemHealth.ready == True``."""
        res = await self._http.get("/global/health")
        res.raise_for_status()
        return SystemHealth.model_validate(res.json())

    # ─── Tools ────────────────────────────────────────────────────────────────

    async def list_tool_ids(self) -> list[str]:
        """List all tool IDs registered in the engine."""
        res = await self._http.get("/tool/ids")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def list_tools(self) -> list[ToolSchema]:
        """List all tools with their schemas."""
        res = await self._http.get("/tool")
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            return [ToolSchema.model_validate(t) for t in raw]
        return []

    async def execute_tool(self, tool: str, args: Optional[dict[str, Any]] = None) -> ToolExecuteResult:
        """
        Execute a built-in tool directly (without a session).

        Example::

            result = await client.execute_tool("workspace_list_files", {"path": "."})
            print(result.output)
        """
        res = await self._http.post("/tool/execute", json={"tool": tool, "args": args or {}})
        res.raise_for_status()
        return ToolExecuteResult.model_validate(res.json())

    # ─── SSE streaming ────────────────────────────────────────────────────────

    def stream(
        self,
        session_id: str,
        run_id: Optional[str] = None,
    ) -> AsyncGenerator[EngineEvent, None]:
        """
        Stream events from an active run as an async generator.

        Example::

            async for event in client.stream(session_id, run_id):
                if event.type == "session.response":
                    print(event.properties.get("delta", ""), end="", flush=True)
                if event.type in ("run.complete", "run.completed", "run.failed", "session.run.finished"):
                    break
        """
        params = f"sessionID={quote(session_id)}"
        if run_id:
            params += f"&runID={quote(run_id)}"
        url = f"{self._base_url}/event?{params}"
        return stream_sse(url, self._token, client=self._http)

    def global_stream(self) -> AsyncGenerator[EngineEvent, None]:
        """Stream all engine events across all sessions."""
        url = f"{self._base_url}/global/event"
        return stream_sse(url, self._token, client=self._http)

    async def run_events(
        self, run_id: str, *, since_seq: Optional[int] = None, tail: Optional[int] = None
    ) -> list[EngineEvent]:
        """Pull stored events for a specific run (not SSE, paginated)."""
        params: dict[str, Any] = {}
        if since_seq is not None:
            params["since_seq"] = since_seq
        if tail is not None:
            params["tail"] = tail
        res = await self._http.get(f"/run/{quote(run_id)}/events", params=params)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            return [_engine_event_adapter.validate_python(e) for e in raw]
        return []


# ─── Sessions namespace ────────────────────────────────────────────────────────


class _Sessions:
    def __init__(self, base_url: str, token: str, http: httpx.AsyncClient) -> None:
        self._base_url = base_url
        self._token = token
        self._http = http

    async def create(
        self,
        *,
        title: str = "Tandem SDK Session",
        directory: str = ".",
        provider: Optional[str] = None,
        model: Optional[str] = None,
    ) -> str:
        """Create a new session. Returns the session ID."""
        payload: dict[str, Any] = {"title": title, "directory": directory}
        if provider and model:
            payload["model"] = {"providerID": provider, "modelID": model}
            payload["provider"] = provider
        res = await self._http.post("/session", json=payload)
        res.raise_for_status()
        return str(res.json()["id"])

    async def list(
        self,
        *,
        q: Optional[str] = None,
        page: Optional[int] = None,
        page_size: Optional[int] = None,
        archived: Optional[bool] = None,
        scope: Optional[str] = None,
        workspace: Optional[str] = None,
    ) -> SessionListResponse:
        """List sessions with optional filtering."""
        params: dict[str, Any] = {}
        if q is not None: params["q"] = q
        if page is not None: params["page"] = page
        if page_size is not None: params["page_size"] = page_size
        if archived is not None: params["archived"] = str(archived).lower()
        if scope is not None: params["scope"] = scope
        if workspace is not None: params["workspace"] = workspace
        res = await self._http.get("/session", params=params)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            sessions = [SessionRecord.model_validate(s) for s in raw]
            return SessionListResponse(sessions=sessions, count=len(sessions))
        return SessionListResponse.model_validate(raw)

    async def get(self, session_id: str) -> SessionRecord:
        """Get a session by ID."""
        res = await self._http.get(f"/session/{quote(session_id)}")
        res.raise_for_status()
        return SessionRecord.model_validate(res.json())

    async def update(
        self, session_id: str, *, title: Optional[str] = None, archived: Optional[bool] = None
    ) -> SessionRecord:
        """Update session metadata (title, archive status)."""
        payload: dict[str, Any] = {}
        if title is not None: payload["title"] = title
        if archived is not None: payload["archived"] = archived
        res = await self._http.patch(f"/session/{quote(session_id)}", json=payload)
        res.raise_for_status()
        return SessionRecord.model_validate(res.json())

    async def archive(self, session_id: str) -> SessionRecord:
        """Archive a session."""
        return await self.update(session_id, archived=True)

    async def delete(self, session_id: str) -> None:
        """Delete a session."""
        res = await self._http.delete(f"/session/{quote(session_id)}")
        res.raise_for_status()

    async def messages(self, session_id: str) -> list[EngineMessage]:
        """Get message history for a session."""
        res = await self._http.get(f"/session/{quote(session_id)}/message")
        res.raise_for_status()
        return [EngineMessage.model_validate(m) for m in res.json()]

    async def todos(self, session_id: str) -> list[SessionTodo]:
        """Get pending TODOs associated with a session."""
        res = await self._http.get(f"/session/{quote(session_id)}/todo")
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            return [SessionTodo.model_validate(t) for t in raw]
        items = raw.get("todos", []) if isinstance(raw, dict) else []
        return [SessionTodo.model_validate(t) for t in items]

    async def active_run(self, session_id: str) -> SessionRunStateResponse:
        """Get the currently active run for a session."""
        res = await self._http.get(f"/session/{quote(session_id)}/run")
        res.raise_for_status()
        return SessionRunStateResponse.model_validate(res.json())

    async def prompt_async(
        self,
        session_id: str,
        prompt: str,
        *,
        tool_mode: Optional[ToolMode] = None,
        tool_allowlist: Optional[list[str]] = None,
        context_mode: Optional[ContextMode] = None,
    ) -> PromptAsyncResult:
        """
        Start an async run. Use ``client.stream()`` to receive events.

        Handles ``409 SESSION_RUN_CONFLICT`` by returning the existing run ID.
        """
        return await self.prompt_async_parts(
            session_id,
            [PromptTextPartInput(type="text", text=prompt)],
            tool_mode=tool_mode,
            tool_allowlist=tool_allowlist,
            context_mode=context_mode,
        )

    async def prompt_async_parts(
        self,
        session_id: str,
        parts: list[PromptPartInput | dict[str, Any]],
        *,
        tool_mode: Optional[ToolMode] = None,
        tool_allowlist: Optional[list[str]] = None,
        context_mode: Optional[ContextMode] = None,
    ) -> PromptAsyncResult:
        """
        Start an async run with explicit prompt parts (text and/or file parts).

        Handles ``409 SESSION_RUN_CONFLICT`` by returning the existing run ID.
        """
        payload = {
            "parts": [
                p.model_dump(exclude_none=True) if isinstance(p, BaseModel) else p
                for p in parts
            ]
        }
        if tool_mode is not None:
            payload["toolMode"] = tool_mode
        if tool_allowlist:
            payload["toolAllowlist"] = tool_allowlist
        if context_mode is not None:
            payload["contextMode"] = context_mode
        res = await self._http.post(
            f"/session/{quote(session_id)}/prompt_async",
            params={"return": "run"},
            json=payload,
        )
        if res.status_code == 409:
            conflict = res.json() or {}
            active = conflict.get("activeRun") or {}
            run_id = active.get("runID") or active.get("runId") or active.get("run_id")
            if run_id:
                return PromptAsyncResult(run_id=str(run_id))
        res.raise_for_status()
        data = res.json() or {}
        run_id = (
            data.get("id") or data.get("runID") or data.get("runId") or data.get("run_id")
            or (data.get("run") or {}).get("id")
        )
        if not run_id:
            raise ValueError(f"Run ID missing in engine response: {data}")
        return PromptAsyncResult(run_id=str(run_id))

    async def prompt_sync(self, session_id: str, prompt: str) -> str:
        """Run a prompt synchronously and return the text reply (blocking)."""
        payload = {"parts": [{"type": "text", "text": prompt}]}
        res = await self._http.post(f"/session/{quote(session_id)}/prompt_sync", json=payload)
        res.raise_for_status()
        data = res.json() or {}
        return str(data.get("reply") or data.get("text") or data.get("output") or "")

    async def abort(self, session_id: str) -> dict[str, Any]:
        """Abort the active run for a session."""
        res = await self._http.post(f"/session/{quote(session_id)}/abort", json={})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def cancel(self, session_id: str) -> dict[str, Any]:
        """Cancel the session's active run."""
        res = await self._http.post(f"/session/{quote(session_id)}/cancel", json={})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def cancel_run(self, session_id: str, run_id: str) -> dict[str, Any]:
        """Cancel a specific run within a session."""
        res = await self._http.post(
            f"/session/{quote(session_id)}/run/{quote(run_id)}/cancel", json={}
        )
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def fork(self, session_id: str) -> SessionRecord:
        """Fork a session into a divergent child session."""
        res = await self._http.post(f"/session/{quote(session_id)}/fork", json={})
        res.raise_for_status()
        return SessionRecord.model_validate(res.json())

    async def diff(self, session_id: str) -> SessionDiff:
        """Get the workspace diff produced by the session's last run."""
        res = await self._http.get(f"/session/{quote(session_id)}/diff")
        res.raise_for_status()
        return SessionDiff.model_validate(res.json())

    async def revert(self, session_id: str) -> dict[str, Any]:
        """Revert uncommitted workspace changes made by the session."""
        res = await self._http.post(f"/session/{quote(session_id)}/revert", json={})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def unrevert(self, session_id: str) -> dict[str, Any]:
        """Undo a previous revert (restore session changes)."""
        res = await self._http.post(f"/session/{quote(session_id)}/unrevert", json={})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def children(self, session_id: str) -> list[SessionRecord]:
        """Get child sessions forked from this session."""
        res = await self._http.get(f"/session/{quote(session_id)}/children")
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            return [SessionRecord.model_validate(s) for s in raw]
        items = raw.get("sessions") or raw.get("children") or []
        return [SessionRecord.model_validate(s) for s in items]

    async def summarize(self, session_id: str) -> dict[str, Any]:
        """Trigger engine-side summarization of the session's conversation history."""
        res = await self._http.post(f"/session/{quote(session_id)}/summarize", json={})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def attach(self, session_id: str, target_workspace: str) -> dict[str, Any]:
        """Attach a session to a different workspace directory."""
        res = await self._http.post(
            f"/session/{quote(session_id)}/attach",
            json={"target_workspace": target_workspace},
        )
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]


# ─── Permissions ──────────────────────────────────────────────────────────────


class _Permissions:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self) -> PermissionSnapshotResponse:
        """List pending permission requests and existing rules."""
        res = await self._http.get("/permission")
        res.raise_for_status()
        return PermissionSnapshotResponse.model_validate(res.json())

    async def reply(self, request_id: str, reply: str) -> dict[str, Any]:
        """Approve or deny a permission request."""
        res = await self._http.post(f"/permission/{quote(request_id)}/reply", json={"reply": reply})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]


# ─── Questions ────────────────────────────────────────────────────────────────


class _Questions:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self) -> QuestionsListResponse:
        """List pending AI-generated questions awaiting confirmation."""
        res = await self._http.get("/question")
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            return QuestionsListResponse(questions=[QuestionRecord.model_validate(q) for q in raw])
        return QuestionsListResponse.model_validate(raw)

    async def reply(self, question_id: str, answer: str) -> dict[str, Any]:
        """Answer a pending question."""
        res = await self._http.post(f"/question/{quote(question_id)}/reply", json={"answer": answer})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def reject(self, question_id: str) -> dict[str, Any]:
        """Reject/dismiss a pending question."""
        res = await self._http.post(f"/question/{quote(question_id)}/reject", json={})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]


# ─── Providers ────────────────────────────────────────────────────────────────


class _Providers:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def catalog(self) -> ProviderCatalog:
        """List available providers and their models."""
        res = await self._http.get("/provider")
        res.raise_for_status()
        return ProviderCatalog.model_validate(res.json())

    async def config(self) -> ProvidersConfigResponse:
        """Get current provider/model configuration."""
        res = await self._http.get("/config/providers")
        res.raise_for_status()
        return ProvidersConfigResponse.model_validate(res.json())

    async def set_defaults(self, provider_id: str, model_id: str) -> None:
        """Set the default provider and model."""
        res = await self._http.patch(
            "/config",
            json={"default_provider": provider_id, "providers": {provider_id: {"default_model": model_id}}},
        )
        res.raise_for_status()

    async def set_api_key(self, provider_id: str, api_key: str) -> None:
        """Store an API key for a provider."""
        res = await self._http.put(f"/auth/{quote(provider_id)}", json={"apiKey": api_key})
        res.raise_for_status()

    async def auth_status(self) -> dict[str, Any]:
        """Get authentication status for all providers."""
        res = await self._http.get("/provider/auth")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]


class _Identity:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def get(self) -> IdentityConfigResponse:
        """Get bot identity/personality config and personality preset catalog."""
        res = await self._http.get("/config/identity")
        res.raise_for_status()
        return IdentityConfigResponse.model_validate(res.json())

    async def patch(self, identity: IdentityConfig | dict[str, Any]) -> IdentityConfigResponse:
        """Patch bot identity/personality config."""
        payload = (
            identity.model_dump(by_alias=True, exclude_none=True)
            if isinstance(identity, IdentityConfig)
            else identity
        )
        res = await self._http.patch("/config/identity", json=payload)
        res.raise_for_status()
        return IdentityConfigResponse.model_validate(res.json())


# ─── Channels ─────────────────────────────────────────────────────────────────


class _Channels:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def config(self) -> ChannelsConfigResponse:
        res = await self._http.get("/channels/config")
        res.raise_for_status()
        return ChannelsConfigResponse.model_validate(res.json())

    async def status(self) -> ChannelsStatusResponse:
        res = await self._http.get("/channels/status")
        res.raise_for_status()
        return ChannelsStatusResponse.model_validate(res.json())

    async def put(self, channel: str, payload: dict[str, Any]) -> dict[str, Any]:
        res = await self._http.put(f"/channels/{channel}", json=payload)
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def delete(self, channel: str) -> dict[str, Any]:
        res = await self._http.delete(f"/channels/{channel}")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]


# ─── MCP ──────────────────────────────────────────────────────────────────────


class _Mcp:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self) -> dict[str, Any]:
        res = await self._http.get("/mcp")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def list_tools(self) -> list[Any]:
        res = await self._http.get("/mcp/tools")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def list_resources(self) -> list[Any]:
        res = await self._http.get("/mcp/resources")
        res.raise_for_status()
        raw = res.json()
        return raw if isinstance(raw, list) else []

    async def add(self, name: str, transport: str, *, headers: Optional[dict[str, str]] = None, enabled: bool = True) -> dict[str, Any]:
        payload: dict[str, Any] = {"name": name, "transport": transport, "enabled": enabled}
        if headers:
            payload["headers"] = headers
        res = await self._http.post("/mcp", json=payload)
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def connect(self, name: str) -> dict[str, Any]:
        res = await self._http.post(f"/mcp/{quote(name)}/connect")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def disconnect(self, name: str) -> dict[str, Any]:
        res = await self._http.post(f"/mcp/{quote(name)}/disconnect")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def refresh(self, name: str) -> dict[str, Any]:
        res = await self._http.post(f"/mcp/{quote(name)}/refresh")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def set_enabled(self, name: str, enabled: bool) -> dict[str, Any]:
        res = await self._http.patch(f"/mcp/{quote(name)}", json={"enabled": enabled})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]


# ─── Memory ───────────────────────────────────────────────────────────────────


class _Memory:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def put(self, text: str, *, tags: Optional[list[str]] = None, source: Optional[str] = None,
                  session_id: Optional[str] = None, run_id: Optional[str] = None, capability: Optional[str] = None) -> MemoryPutResponse:
        payload: dict[str, Any] = {"text": text}
        if tags: payload["tags"] = tags
        if source: payload["source"] = source
        if session_id: payload["session_id"] = session_id
        if run_id: payload["run_id"] = run_id
        if capability: payload["capability"] = capability
        res = await self._http.post("/memory/put", json=payload)
        res.raise_for_status()
        return MemoryPutResponse.model_validate(res.json())

    async def search(self, query: str, *, limit: Optional[int] = None, tags: Optional[list[str]] = None,
                     session_id: Optional[str] = None, capability: Optional[str] = None) -> MemorySearchResponse:
        payload: dict[str, Any] = {"query": query}
        if limit is not None: payload["limit"] = limit
        if tags: payload["tags"] = tags
        if session_id: payload["session_id"] = session_id
        if capability: payload["capability"] = capability
        res = await self._http.post("/memory/search", json=payload)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            from .types import MemorySearchResult
            return MemorySearchResponse(results=[MemorySearchResult.model_validate(r) for r in raw], count=len(raw))
        return MemorySearchResponse.model_validate(raw)

    async def list(self, *, q: Optional[str] = None, limit: Optional[int] = None, offset: Optional[int] = None,
                   user_id: Optional[str] = None) -> MemoryListResponse:
        params: dict[str, Any] = {}
        if q: params["q"] = q
        if limit is not None: params["limit"] = limit
        if offset is not None: params["offset"] = offset
        if user_id: params["user_id"] = user_id
        res = await self._http.get("/memory", params=params)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            return MemoryListResponse(items=[MemoryItem.model_validate(i) for i in raw], count=len(raw))
        return MemoryListResponse.model_validate(raw)

    async def delete(self, memory_id: str) -> dict[str, Any]:
        res = await self._http.delete(f"/memory/{quote(memory_id)}")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def promote(self, memory_id: str, *, capability: Optional[str] = None) -> MemoryPromoteResponse:
        payload: dict[str, Any] = {"id": memory_id}
        if capability: payload["capability"] = capability
        res = await self._http.post("/memory/promote", json=payload)
        res.raise_for_status()
        return MemoryPromoteResponse.model_validate(res.json())

    async def demote(self, memory_id: str, *, run_id: Optional[str] = None) -> dict[str, Any]:
        payload: dict[str, Any] = {"id": memory_id}
        if run_id: payload["run_id"] = run_id
        res = await self._http.post("/memory/demote", json=payload)
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def audit(self, *, run_id: Optional[str] = None, limit: Optional[int] = None) -> MemoryAuditResponse:
        params: dict[str, Any] = {}
        if run_id: params["run_id"] = run_id
        if limit is not None: params["limit"] = limit
        res = await self._http.get("/memory/audit", params=params)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            from .types import MemoryAuditEntry
            return MemoryAuditResponse(entries=[MemoryAuditEntry.model_validate(e) for e in raw], count=len(raw))
        return MemoryAuditResponse.model_validate(raw)


# ─── Skills ───────────────────────────────────────────────────────────────────


class _Skills:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self, location: Optional[str] = None) -> SkillsListResponse:
        params = {"location": location} if location else {}
        res = await self._http.get("/skills", params=params)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            return SkillsListResponse(skills=[SkillRecord.model_validate(s) for s in raw], count=len(raw))
        return SkillsListResponse.model_validate(raw)

    async def get(self, name: str) -> SkillRecord:
        res = await self._http.get(f"/skills/{quote(name)}")
        res.raise_for_status()
        return SkillRecord.model_validate(res.json())

    async def import_skill(self, location: str, *, content: Optional[str] = None,
                           file_or_path: Optional[str] = None, namespace: Optional[str] = None,
                           conflict_policy: Optional[str] = None) -> SkillImportResponse:
        payload: dict[str, Any] = {"location": location}
        if content: payload["content"] = content
        if file_or_path: payload["file_or_path"] = file_or_path
        if namespace: payload["namespace"] = namespace
        if conflict_policy: payload["conflict_policy"] = conflict_policy
        res = await self._http.post("/skills/import", json=payload)
        res.raise_for_status()
        return SkillImportResponse.model_validate(res.json())

    async def preview(self, location: str, *, content: Optional[str] = None, file_or_path: Optional[str] = None) -> SkillImportResponse:
        payload: dict[str, Any] = {"location": location}
        if content: payload["content"] = content
        if file_or_path: payload["file_or_path"] = file_or_path
        res = await self._http.post("/skills/import/preview", json=payload)
        res.raise_for_status()
        return SkillImportResponse.model_validate(res.json())

    async def templates(self) -> SkillTemplatesResponse:
        res = await self._http.get("/skills/templates")
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            from .types import SkillTemplate
            return SkillTemplatesResponse(templates=[SkillTemplate.model_validate(t) for t in raw], count=len(raw))
        return SkillTemplatesResponse.model_validate(raw)


# ─── Resources ────────────────────────────────────────────────────────────────


class _Resources:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self, *, prefix: Optional[str] = None, limit: Optional[int] = None) -> ResourceListResponse:
        params: dict[str, Any] = {}
        if prefix: params["prefix"] = prefix
        if limit is not None: params["limit"] = limit
        res = await self._http.get("/resource", params=params)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            from .types import ResourceRecord
            return ResourceListResponse(items=[ResourceRecord.model_validate(r) for r in raw], count=len(raw))
        return ResourceListResponse.model_validate(raw)

    async def write(self, key: str, value: Any, *, if_match_rev: Optional[int] = None,
                    updated_by: Optional[str] = None, ttl_ms: Optional[int] = None) -> ResourceWriteResponse:
        payload: dict[str, Any] = {"key": key, "value": value}
        if if_match_rev is not None: payload["if_match_rev"] = if_match_rev
        if updated_by: payload["updated_by"] = updated_by
        if ttl_ms is not None: payload["ttl_ms"] = ttl_ms
        res = await self._http.put("/resource", json=payload)
        res.raise_for_status()
        return ResourceWriteResponse.model_validate(res.json())

    async def delete(self, key: str, *, if_match_rev: Optional[int] = None) -> dict[str, Any]:
        payload: dict[str, Any] = {"key": key}
        if if_match_rev is not None: payload["if_match_rev"] = if_match_rev
        res = await self._http.delete("/resource", json=payload)
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]


# ─── Routines ─────────────────────────────────────────────────────────────────


class _Routines:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self) -> DefinitionListResponse:
        res = await self._http.get("/routines")
        res.raise_for_status()
        return DefinitionListResponse.model_validate(res.json())

    async def create(self, options: dict[str, Any]) -> DefinitionCreateResponse:
        if "prompt" in options and "entrypoint" not in options:
            options = {**options, "entrypoint": options["prompt"]}
        res = await self._http.post("/routines", json=options)
        res.raise_for_status()
        return DefinitionCreateResponse.model_validate(res.json())

    async def update(self, routine_id: str, patch: dict[str, Any]) -> RoutineRecord:
        res = await self._http.patch(f"/routines/{quote(routine_id)}", json=patch)
        res.raise_for_status()
        return RoutineRecord.model_validate(res.json())

    async def delete(self, routine_id: str) -> None:
        res = await self._http.delete(f"/routines/{quote(routine_id)}")
        res.raise_for_status()

    async def run_now(self, routine_id: str) -> RunNowResponse:
        res = await self._http.post(f"/routines/{quote(routine_id)}/run_now", json={})
        res.raise_for_status()
        return RunNowResponse.model_validate(res.json())

    async def list_runs(self, *, routine_id: Optional[str] = None, limit: int = 25) -> list[dict[str, Any]]:
        params: dict[str, Any] = {"limit": limit}
        if routine_id: params["routine_id"] = routine_id
        res = await self._http.get("/routines/runs", params=params)
        res.raise_for_status()
        data = res.json()
        return data.get("runs", data) if isinstance(data, dict) else data

    async def get_runs_for_routine(self, routine_id: str, limit: int = 25) -> list[dict[str, Any]]:
        res = await self._http.get(f"/routines/{quote(routine_id)}/runs", params={"limit": limit})
        res.raise_for_status()
        data = res.json()
        return data.get("runs", data) if isinstance(data, dict) else data

    async def get_run(self, run_id: str) -> RunRecord:
        res = await self._http.get(f"/routines/runs/{quote(run_id)}")
        res.raise_for_status()
        return RunRecord.model_validate(res.json())

    async def list_artifacts(self, run_id: str) -> RunArtifactsResponse:
        res = await self._http.get(f"/routines/runs/{quote(run_id)}/artifacts")
        res.raise_for_status()
        return RunArtifactsResponse.model_validate(res.json())

    async def approve_run(self, run_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(f"/routines/runs/{quote(run_id)}/approve", json={"reason": reason})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def deny_run(self, run_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(f"/routines/runs/{quote(run_id)}/deny", json={"reason": reason})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def pause_run(self, run_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(f"/routines/runs/{quote(run_id)}/pause", json={"reason": reason})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def resume_run(self, run_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(f"/routines/runs/{quote(run_id)}/resume", json={"reason": reason})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def history(self, routine_id: str, limit: Optional[int] = None) -> RoutineHistoryResponse:
        params = {"limit": limit} if limit is not None else {}
        res = await self._http.get(f"/routines/{quote(routine_id)}/history", params=params)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            from .types import RoutineHistoryEntry
            return RoutineHistoryResponse(history=[RoutineHistoryEntry.model_validate(e) for e in raw], count=len(raw))
        return RoutineHistoryResponse.model_validate(raw)


# ─── Automations ──────────────────────────────────────────────────────────────


class _Automations:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self) -> DefinitionListResponse:
        res = await self._http.get("/automations")
        res.raise_for_status()
        return DefinitionListResponse.model_validate(res.json())

    async def create(self, options: dict[str, Any]) -> DefinitionCreateResponse:
        res = await self._http.post("/automations", json=options)
        res.raise_for_status()
        return DefinitionCreateResponse.model_validate(res.json())

    async def update(self, automation_id: str, patch: dict[str, Any]) -> dict[str, Any]:
        res = await self._http.patch(f"/automations/{quote(automation_id)}", json=patch)
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def delete(self, automation_id: str) -> None:
        res = await self._http.delete(f"/automations/{quote(automation_id)}")
        res.raise_for_status()

    async def run_now(self, automation_id: str) -> RunNowResponse:
        res = await self._http.post(f"/automations/{quote(automation_id)}/run_now", json={})
        res.raise_for_status()
        return RunNowResponse.model_validate(res.json())

    async def list_runs(self, *, automation_id: Optional[str] = None, limit: int = 25) -> list[dict[str, Any]]:
        params: dict[str, Any] = {"limit": limit}
        if automation_id: params["automation_id"] = automation_id
        res = await self._http.get("/automations/runs", params=params)
        res.raise_for_status()
        data = res.json()
        return data.get("runs", data) if isinstance(data, dict) else data

    async def get_runs_for_automation(self, automation_id: str, limit: int = 25) -> list[dict[str, Any]]:
        res = await self._http.get(f"/automations/{quote(automation_id)}/runs", params={"limit": limit})
        res.raise_for_status()
        data = res.json()
        return data.get("runs", data) if isinstance(data, dict) else data

    async def get_run(self, run_id: str) -> RunRecord:
        res = await self._http.get(f"/automations/runs/{quote(run_id)}")
        res.raise_for_status()
        return RunRecord.model_validate(res.json())

    async def list_artifacts(self, run_id: str) -> RunArtifactsResponse:
        res = await self._http.get(f"/automations/runs/{quote(run_id)}/artifacts")
        res.raise_for_status()
        return RunArtifactsResponse.model_validate(res.json())

    async def approve_run(self, run_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(f"/automations/runs/{quote(run_id)}/approve", json={"reason": reason})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def deny_run(self, run_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(f"/automations/runs/{quote(run_id)}/deny", json={"reason": reason})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def pause_run(self, run_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(f"/automations/runs/{quote(run_id)}/pause", json={"reason": reason})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def resume_run(self, run_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(f"/automations/runs/{quote(run_id)}/resume", json={"reason": reason})
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def history(self, automation_id: str, limit: Optional[int] = None) -> RoutineHistoryResponse:
        params = {"limit": limit} if limit is not None else {}
        res = await self._http.get(f"/automations/{quote(automation_id)}/history", params=params)
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            from .types import RoutineHistoryEntry
            return RoutineHistoryResponse(history=[RoutineHistoryEntry.model_validate(e) for e in raw], count=len(raw))
        return RoutineHistoryResponse.model_validate(raw)


class _AutomationsV2:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self) -> AutomationV2ListResponse:
        res = await self._http.get("/automations/v2")
        res.raise_for_status()
        return AutomationV2ListResponse.model_validate(res.json())

    async def create(self, spec: dict[str, Any]) -> AutomationV2Record:
        res = await self._http.post("/automations/v2", json=spec)
        res.raise_for_status()
        return AutomationV2Record.model_validate(res.json().get("automation", {}))

    async def get(self, automation_id: str) -> AutomationV2Record:
        res = await self._http.get(f"/automations/v2/{quote(automation_id)}")
        res.raise_for_status()
        return AutomationV2Record.model_validate(res.json().get("automation", {}))

    async def update(self, automation_id: str, patch: dict[str, Any]) -> AutomationV2Record:
        res = await self._http.patch(f"/automations/v2/{quote(automation_id)}", json=patch)
        res.raise_for_status()
        return AutomationV2Record.model_validate(res.json().get("automation", {}))

    async def delete(self, automation_id: str) -> dict[str, Any]:
        res = await self._http.delete(f"/automations/v2/{quote(automation_id)}")
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def run_now(self, automation_id: str) -> AutomationV2RunRecord:
        res = await self._http.post(f"/automations/v2/{quote(automation_id)}/run_now", json={})
        res.raise_for_status()
        return AutomationV2RunRecord.model_validate(res.json().get("run", {}))

    async def pause(self, automation_id: str, reason: str = "") -> AutomationV2Record:
        res = await self._http.post(
            f"/automations/v2/{quote(automation_id)}/pause", json={"reason": reason}
        )
        res.raise_for_status()
        return AutomationV2Record.model_validate(res.json().get("automation", {}))

    async def resume(self, automation_id: str) -> AutomationV2Record:
        res = await self._http.post(f"/automations/v2/{quote(automation_id)}/resume", json={})
        res.raise_for_status()
        return AutomationV2Record.model_validate(res.json().get("automation", {}))

    async def list_runs(self, automation_id: str, limit: int = 50) -> AutomationV2RunListResponse:
        res = await self._http.get(
            f"/automations/v2/{quote(automation_id)}/runs", params={"limit": limit}
        )
        res.raise_for_status()
        return AutomationV2RunListResponse.model_validate(res.json())

    async def get_run(self, run_id: str) -> AutomationV2RunRecord:
        res = await self._http.get(f"/automations/v2/runs/{quote(run_id)}")
        res.raise_for_status()
        return AutomationV2RunRecord.model_validate(res.json().get("run", {}))

    async def pause_run(self, run_id: str, reason: str = "") -> AutomationV2RunRecord:
        res = await self._http.post(
            f"/automations/v2/runs/{quote(run_id)}/pause", json={"reason": reason}
        )
        res.raise_for_status()
        return AutomationV2RunRecord.model_validate(res.json().get("run", {}))

    async def resume_run(self, run_id: str, reason: str = "") -> AutomationV2RunRecord:
        res = await self._http.post(
            f"/automations/v2/runs/{quote(run_id)}/resume", json={"reason": reason}
        )
        res.raise_for_status()
        return AutomationV2RunRecord.model_validate(res.json().get("run", {}))

    async def cancel_run(self, run_id: str, reason: str = "") -> AutomationV2RunRecord:
        res = await self._http.post(
            f"/automations/v2/runs/{quote(run_id)}/cancel", json={"reason": reason}
        )
        res.raise_for_status()
        return AutomationV2RunRecord.model_validate(res.json().get("run", {}))


# ─── Agent Teams ──────────────────────────────────────────────────────────────


class _AgentTeams:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list_templates(self) -> AgentTeamTemplatesResponse:
        res = await self._http.get("/agent-team/templates")
        res.raise_for_status()
        return AgentTeamTemplatesResponse.model_validate(res.json())

    async def create_template(self, template: dict[str, Any]) -> AgentTeamTemplateWriteResponse:
        res = await self._http.post("/agent-team/templates", json={"template": template})
        res.raise_for_status()
        return AgentTeamTemplateWriteResponse.model_validate(res.json())

    async def update_template(
        self, template_id: str, patch: dict[str, Any]
    ) -> AgentTeamTemplateWriteResponse:
        res = await self._http.patch(f"/agent-team/templates/{quote(template_id)}", json=patch)
        res.raise_for_status()
        return AgentTeamTemplateWriteResponse.model_validate(res.json())

    async def delete_template(self, template_id: str) -> AgentTeamTemplateWriteResponse:
        res = await self._http.delete(f"/agent-team/templates/{quote(template_id)}")
        res.raise_for_status()
        return AgentTeamTemplateWriteResponse.model_validate(res.json())

    async def list_instances(self, *, mission_id: Optional[str] = None,
                              parent_instance_id: Optional[str] = None,
                              status: Optional[str] = None) -> AgentTeamInstancesResponse:
        params: dict[str, Any] = {}
        if mission_id: params["missionID"] = mission_id
        if parent_instance_id: params["parentInstanceID"] = parent_instance_id
        if status: params["status"] = status
        res = await self._http.get("/agent-team/instances", params=params)
        res.raise_for_status()
        return AgentTeamInstancesResponse.model_validate(res.json())

    async def list_missions(self) -> AgentTeamMissionsResponse:
        res = await self._http.get("/agent-team/missions")
        res.raise_for_status()
        return AgentTeamMissionsResponse.model_validate(res.json())

    async def list_approvals(self) -> AgentTeamApprovalsResponse:
        res = await self._http.get("/agent-team/approvals")
        res.raise_for_status()
        return AgentTeamApprovalsResponse.model_validate(res.json())

    async def spawn(self, role: str, justification: str, *, mission_id: Optional[str] = None,
                    parent_instance_id: Optional[str] = None, template_id: Optional[str] = None,
                    budget_override: Optional[dict[str, Any]] = None) -> AgentTeamSpawnResponse:
        payload: dict[str, Any] = {"role": role, "justification": justification}
        if mission_id: payload["missionID"] = mission_id
        if parent_instance_id: payload["parentInstanceID"] = parent_instance_id
        if template_id: payload["templateID"] = template_id
        if budget_override: payload["budget_override"] = budget_override
        res = await self._http.post("/agent-team/spawn", json=payload)
        res.raise_for_status()
        return AgentTeamSpawnResponse.model_validate(res.json())

    async def approve_spawn(self, approval_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(
            f"/agent-team/approvals/spawn/{quote(approval_id)}/approve", json={"reason": reason}
        )
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]

    async def deny_spawn(self, approval_id: str, reason: str = "") -> dict[str, Any]:
        res = await self._http.post(
            f"/agent-team/approvals/spawn/{quote(approval_id)}/deny", json={"reason": reason}
        )
        res.raise_for_status()
        return res.json()  # type: ignore[no-any-return]


# ─── Missions ─────────────────────────────────────────────────────────────────


class _Missions:
    def __init__(self, http: httpx.AsyncClient) -> None:
        self._http = http

    async def list(self) -> MissionListResponse:
        res = await self._http.get("/mission")
        res.raise_for_status()
        raw = res.json()
        if isinstance(raw, list):
            return MissionListResponse(missions=[MissionRecord.model_validate(m) for m in raw], count=len(raw))
        return MissionListResponse.model_validate(raw)

    async def create(self, title: str, goal: str, work_items: Optional[list[dict[str, Any]]] = None) -> MissionCreateResponse:
        payload: dict[str, Any] = {"title": title, "goal": goal, "work_items": work_items or []}
        res = await self._http.post("/mission", json=payload)
        res.raise_for_status()
        return MissionCreateResponse.model_validate(res.json())

    async def get(self, mission_id: str) -> MissionGetResponse:
        res = await self._http.get(f"/mission/{quote(mission_id)}")
        res.raise_for_status()
        return MissionGetResponse.model_validate(res.json())

    async def apply_event(self, mission_id: str, event: dict[str, Any]) -> MissionEventResponse:
        res = await self._http.post(f"/mission/{quote(mission_id)}/event", json={"event": event})
        res.raise_for_status()
        return MissionEventResponse.model_validate(res.json())


# ─── Sync wrapper ─────────────────────────────────────────────────────────────


class SyncTandemClient:
    """
    Synchronous wrapper around :class:`TandemClient`.

    Useful for scripts that don't use ``async``::

        from tandem_client import SyncTandemClient

        client = SyncTandemClient(base_url="http://localhost:39731", token="...")
        session_id = client.sessions.create(title="My agent")

    .. warning::
        Does not support ``stream()`` or ``global_stream()`` — use the async client for streaming.
    """

    def __init__(self, base_url: str, token: str, *, timeout: float = 20.0) -> None:
        self._async = TandemClient(base_url=base_url, token=token, timeout=timeout)

    def __getattr__(self, name: str) -> Any:
        attr = getattr(self._async, name)
        if asyncio.iscoroutinefunction(attr):
            def wrapper(*args: Any, **kwargs: Any) -> Any:
                return asyncio.run(attr(*args, **kwargs))
            return wrapper
        return _SyncNamespace(attr)

    def close(self) -> None:
        asyncio.run(self._async.aclose())


class _SyncNamespace:
    def __init__(self, ns: Any) -> None:
        self._ns = ns

    def __getattr__(self, name: str) -> Any:
        attr = getattr(self._ns, name)
        if asyncio.iscoroutinefunction(attr):
            def wrapper(*args: Any, **kwargs: Any) -> Any:
                return asyncio.run(attr(*args, **kwargs))
            return wrapper
        return attr
