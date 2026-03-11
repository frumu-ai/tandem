import json
from pathlib import Path

import httpx
import pytest
import respx
from pydantic import TypeAdapter
from tandem_client import TandemClient
from tandem_client.types import EngineEvent

CONTRACT_PATH = Path(__file__).parent.parent.parent.parent / "contracts" / "events.json"

_engine_event_adapter = TypeAdapter(EngineEvent)

def test_events_contract():
    assert CONTRACT_PATH.exists(), f"Could not find events.json at {CONTRACT_PATH}"
    
    events_contract = json.loads(CONTRACT_PATH.read_text())
    assert len(events_contract) > 0

    for event_def in events_contract:
        event_type = event_def["type"]
        required_fields = event_def["required"]
        
        # Mock tolerant wire format payload
        mock_wire_payload = {
            "type": event_type,
            "timestamp": "2024-01-01T00:00:00Z",
            "properties": {"custom": "data"}
        }
        
        # Populate varying wire forms
        if "sessionId" in required_fields:
            mock_wire_payload["sessionID"] = "s_123"
        if "runId" in required_fields:
            mock_wire_payload["run_id"] = "r_456"

        # Validate with TypeAdapter
        event = _engine_event_adapter.validate_python(mock_wire_payload)

        # Assert Canonical properties
        assert event.type == event_type
        assert event.properties == {"custom": "data"}
        assert event.timestamp == "2024-01-01T00:00:00Z"
        
        if "sessionId" in required_fields:
            assert event.session_id == "s_123"
        if "runId" in required_fields:
            assert event.run_id == "r_456"

        print(f"Passed: {event_type}")


@pytest.mark.asyncio
@respx.mock
async def test_coder_list_runs_and_approve_route() -> None:
    respx.get("http://localhost:39731/coder/runs").mock(
        return_value=httpx.Response(200, json={"runs": [{"coder_run_id": "coder-1"}]})
    )
    approve_route = respx.post("http://localhost:39731/coder/runs/coder-1/approve").mock(
        return_value=httpx.Response(200, json={"ok": True})
    )

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        runs = await client.coder.list_runs(
            limit=5, workflow_mode="issue_triage", repo_slug="evan/tandem"
        )
        result = await client.coder.approve_run("coder-1", "looks good")

    assert runs.runs[0].coder_run_id == "coder-1"
    assert runs.count == 1
    assert result["ok"] is True
    assert approve_route.called
    payload = approve_route.calls[0].request.content.decode("utf-8")
    assert "looks good" in payload


@pytest.mark.asyncio
@respx.mock
async def test_high_value_sdk_parity_routes() -> None:
    respx.get("http://localhost:39731/browser/status").mock(
        return_value=httpx.Response(200, json={"runnable": True})
    )
    respx.post("http://localhost:39731/browser/install").mock(
        return_value=httpx.Response(200, json={"ok": True})
    )
    respx.post("http://localhost:39731/browser/smoke-test").mock(
        return_value=httpx.Response(200, json={"ok": True, "url": "https://example.com"})
    )
    respx.get("http://localhost:39731/workflows/runs").mock(
        return_value=httpx.Response(200, json={"runs": [], "count": 0})
    )
    workflow_run_route = respx.post("http://localhost:39731/workflows/wf-1/run").mock(
        return_value=httpx.Response(200, json={"run": {"id": "run-1"}})
    )
    respx.get("http://localhost:39731/bug-monitor/drafts").mock(
        return_value=httpx.Response(200, json={"drafts": [], "count": 0})
    )
    approve_draft_route = respx.post("http://localhost:39731/bug-monitor/drafts/d-1/approve").mock(
        return_value=httpx.Response(200, json={"ok": True})
    )
    respx.get("http://localhost:39731/mcp/catalog/demo/toml").mock(
        return_value=httpx.Response(200, text="name = 'demo'\n")
    )
    respx.get("http://localhost:39731/resource/a/b").mock(
        return_value=httpx.Response(200, json={"key": "a/b", "value": {}})
    )
    patch_resource_route = respx.patch("http://localhost:39731/resource/a/b").mock(
        return_value=httpx.Response(200, json={"ok": True, "rev": 2})
    )
    add_artifact_route = respx.post("http://localhost:39731/routines/runs/run-r/artifacts").mock(
        return_value=httpx.Response(200, json={"ok": True})
    )

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        status = await client.browser.status()
        install = await client.browser.install()
        smoke = await client.browser.smoke_test("https://example.com")
        workflow_runs = await client.workflows.list_runs(limit=5)
        await client.workflows.run("wf-1")
        drafts = await client.bug_monitor.list_drafts(limit=5)
        await client.bug_monitor.approve_draft("d-1", "ship it")
        toml = await client.mcp.catalog_toml("demo")
        resource = await client.resources.get("a/b")
        patched = await client.resources.patch_key("a/b", {"value": {"ok": True}})
        artifact = await client.routines.add_artifact("run-r", {"uri": "file://x", "kind": "report"})

    assert status.runnable is True
    assert install.ok is True
    assert smoke.ok is True
    assert workflow_runs.count == 0
    assert workflow_run_route.called
    assert drafts.count == 0
    assert approve_draft_route.called
    assert "ship it" in approve_draft_route.calls[0].request.content.decode("utf-8")
    assert "name = 'demo'" in toml
    assert resource.key == "a/b"
    assert patched.ok is True
    assert patch_resource_route.called
    assert artifact["ok"] is True
    assert add_artifact_route.called


@pytest.mark.asyncio
@respx.mock
async def test_workflow_plans_namespace_routes() -> None:
    preview_route = respx.post("http://localhost:39731/workflow-plans/preview").mock(
        return_value=httpx.Response(
            200,
            json={
                "plan": {
                    "plan_id": "plan-1",
                    "title": "Release checklist",
                    "schedule": {"type": "manual"},
                    "steps": [{"step_id": "step-1", "kind": "task", "objective": "Review changelog"}],
                }
            },
        )
    )
    chat_start_route = respx.post("http://localhost:39731/workflow-plans/chat/start").mock(
        return_value=httpx.Response(
            200,
            json={
                "plan": {
                    "plan_id": "plan-1",
                    "title": "Release checklist",
                    "schedule": {"type": "manual"},
                    "steps": [{"step_id": "step-1", "kind": "task", "objective": "Review changelog"}],
                },
                "conversation": {
                    "conversation_id": "conv-1",
                    "plan_id": "plan-1",
                    "messages": [{"role": "assistant", "text": "Drafted plan."}],
                },
            },
        )
    )
    chat_message_route = respx.post("http://localhost:39731/workflow-plans/chat/message").mock(
        return_value=httpx.Response(
            200,
            json={
                "plan": {
                    "plan_id": "plan-1",
                    "title": "Release checklist",
                    "schedule": {"type": "manual"},
                    "steps": [{"step_id": "step-1", "kind": "task", "objective": "Review changelog"}],
                },
                "conversation": {
                    "conversation_id": "conv-1",
                    "plan_id": "plan-1",
                    "messages": [{"role": "user", "text": "Add smoke tests."}],
                },
                "change_summary": ["Added smoke-test step."],
            },
        )
    )

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        preview = await client.workflow_plans.preview(prompt="Create a release checklist")
        started = await client.workflow_plans.chat_start(prompt="Create a release checklist")
        messaged = await client.workflow_plans.chat_message(
            plan_id="plan-1", message="Add smoke tests."
        )

    assert preview.plan.plan_id == "plan-1"
    assert preview.plan.steps[0].objective == "Review changelog"
    assert started.conversation.conversation_id == "conv-1"
    assert messaged.change_summary == ["Added smoke-test step."]
    assert preview_route.called
    assert chat_start_route.called
    assert chat_message_route.called


@respx.mock
def test_sync_wrapper_supports_browser_namespace() -> None:
    from tandem_client import SyncTandemClient

    respx.get("http://localhost:39731/browser/status").mock(
        return_value=httpx.Response(200, json={"runnable": True})
    )
    client = SyncTandemClient(base_url="http://localhost:39731", token="token")
    try:
        status = client.browser.status()
        assert status.runnable is True
    finally:
        client.close()


@respx.mock
def test_sync_wrapper_supports_workflow_plans_namespace() -> None:
    from tandem_client import SyncTandemClient

    respx.post("http://localhost:39731/workflow-plans/preview").mock(
        return_value=httpx.Response(
            200,
            json={
                "plan": {
                    "plan_id": "plan-1",
                    "title": "Release checklist",
                    "schedule": {"type": "manual"},
                    "steps": [{"step_id": "step-1", "kind": "task", "objective": "Review changelog"}],
                }
            },
        )
    )
    client = SyncTandemClient(base_url="http://localhost:39731", token="token")
    try:
        preview = client.workflow_plans.preview(prompt="Create a release checklist")
        assert preview.plan.plan_id == "plan-1"
    finally:
        client.close()
