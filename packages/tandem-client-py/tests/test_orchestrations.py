import json

import httpx
import pytest
import respx

from tandem_client import OrchestrationDraftInput, TandemClient


BASE = "http://localhost:39731"


def _tenant() -> dict[str, object]:
    return {"org_id": "local", "workspace_id": "local", "source": "local_implicit"}


def _draft(status: str = "draft", version: int = 0) -> dict[str, object]:
    return {
        "schema_version": 1,
        "orchestration_id": "goal/loop",
        "name": "Goal loop",
        "status": status,
        "version": version,
        "root_node_id": "start",
        "nodes": [
            {
                "node_id": "start",
                "name": "Start",
                "position": {"x": 0.0, "y": 0.0},
                "kind": "workflow",
                "automation_id": "automation-1",
                "pinned_definition_hash": "sha256:old",
                "allowed_transition_keys": ["complete"],
            },
            {
                "node_id": "done",
                "name": "Done",
                "position": {"x": 300.0, "y": 0.0},
                "kind": "terminal",
                "outcome": "complete",
            },
        ],
        "edges": [
            {
                "edge_id": "finish",
                "from_node_id": "start",
                "to_node_id": "done",
                "transition_key": "complete",
                "artifact_contract": {"artifact_type": "report", "required": True},
            }
        ],
        "goal_policy": {"max_hops": 5, "on_limit": "pause_for_review"},
        "tenant_context": _tenant(),
        "created_at_ms": 100,
        "updated_at_ms": 101,
        "published_at_ms": 200 if status == "published" else None,
    }


def _spec_response(spec: dict[str, object]) -> dict[str, object]:
    return {
        "orchestration": spec,
        "orchestration_id": spec["orchestration_id"],
        "version": spec["version"],
        "status": spec["status"],
        "updated_at_ms": spec["updated_at_ms"],
    }


@pytest.mark.asyncio
@respx.mock
async def test_canonical_draft_v0_routes_and_aggregate_contracts() -> None:
    draft = _draft()
    updated = _draft()
    updated["updated_at_ms"] = 150
    published = _draft("published", 1)
    refreshed = _draft()
    refreshed["updated_at_ms"] = 160
    refreshed["nodes"][0]["pinned_definition_hash"] = "sha256:new"  # type: ignore[index]
    path = f"{BASE}/orchestrations/goal%2Floop"

    list_route = respx.get(
        f"{BASE}/orchestrations", params={"status": "draft", "limit": "10"}
    ).mock(return_value=httpx.Response(200, json={
        "orchestrations": [{
            "orchestration_id": "goal/loop", "name": "Goal loop",
            "draft": {"status": "draft", "updated_at_ms": 101},
            "latest_published_version": 1,
            "published_versions": [{"version": 1, "published_at_ms": 200}],
        }], "count": 1,
    }))
    create_route = respx.post(f"{BASE}/orchestrations").mock(
        return_value=httpx.Response(201, json=_spec_response(draft))
    )
    get_route = respx.get(path).mock(return_value=httpx.Response(200, json={
        "orchestration_id": "goal/loop", "draft": draft, "latest_published": published,
    }))
    update_route = respx.put(path).mock(
        return_value=httpx.Response(200, json=_spec_response(updated))
    )
    versions_route = respx.get(f"{path}/versions").mock(return_value=httpx.Response(200, json={
        "orchestration_id": "goal/loop",
        "versions": [{"version": 1, "name": "Goal loop", "published_at_ms": 200, "metadata": None}],
        "count": 1,
    }))
    version_route = respx.get(f"{path}/versions/1").mock(
        return_value=httpx.Response(200, json=_spec_response(published))
    )
    stale = [{
        "node_id": "start", "automation_id": "automation-1", "state": "stale",
        "pinned_hash": "sha256:old", "current_hash": "sha256:new",
    }]
    stale_route = respx.get(f"{path}/stale-references").mock(
        return_value=httpx.Response(200, json={"orchestration_id": "goal/loop", "references": stale, "stale_count": 1})
    )
    validate_route = respx.post(f"{path}/validate").mock(return_value=httpx.Response(200, json={
        "orchestration_id": "goal/loop", "version": 0,
        "report": {"valid": True, "issues": []}, "stale_references": stale,
    }))
    refresh_route = respx.post(f"{path}/refresh-references").mock(
        return_value=httpx.Response(200, json={"orchestration": refreshed, "refreshed_node_ids": ["start"]})
    )
    publish_route = respx.post(f"{path}/publish").mock(
        return_value=httpx.Response(201, json=_spec_response(published))
    )
    dry_run_route = respx.post(f"{path}/dry-run").mock(return_value=httpx.Response(200, json={
        "allowed": True, "issues": [],
        "edge": {"edge_id": "finish", "transition_key": "complete", "artifact_contract": draft["edges"][0]["artifact_contract"], "approval_required": False},  # type: ignore[index]
        "target": {"node_id": "done", "name": "Done", "kind": {"kind": "terminal", "outcome": "complete"}},
    }))
    archive_route = respx.post(f"{path}/archive").mock(
        return_value=httpx.Response(200, json=_spec_response({**draft, "status": "archived"}))
    )

    write = OrchestrationDraftInput.model_validate({
        "orchestration_id": "goal/loop", "name": "Goal loop", "root_node_id": "start",
        "nodes": draft["nodes"], "edges": draft["edges"], "goal_policy": draft["goal_policy"],
    })
    async with TandemClient(base_url=BASE, token="token") as client:
        listed = await client.orchestrations.list(status="draft", limit=10)
        created = await client.orchestrations.create(write)
        aggregate = await client.orchestrations.get("goal/loop")
        changed = await client.orchestrations.update("goal/loop", write, expected_updated_at_ms=101)
        versions = await client.orchestrations.list_versions("goal/loop")
        version = await client.orchestrations.get_version("goal/loop", 1)
        refs = await client.orchestrations.stale_references("goal/loop")
        validation = await client.orchestrations.validate("goal/loop")
        refresh = await client.orchestrations.refresh_references("goal/loop", expected_updated_at_ms=150)
        release = await client.orchestrations.publish("goal/loop", expected_updated_at_ms=150)
        await client.orchestrations.publish("goal/loop")
        preview = await client.orchestrations.dry_run(
            "goal/loop", from_node_id="start", transition_key="complete", artifact_type="report", version=1
        )
        archived = await client.orchestrations.archive("goal/loop", expected_updated_at_ms=150)
        await client.orchestrations.archive("goal/loop")

    assert listed.orchestrations[0].draft.status == "draft"  # type: ignore[union-attr]
    assert created.version == 0
    assert aggregate.latest_published.version == 1  # type: ignore[union-attr]
    assert changed.updated_at_ms == 150
    assert versions.versions[0].version == 1
    assert version.orchestration.status == "published"
    assert refs.references[0].state == "stale"
    assert validation.report.valid is True
    assert refresh.refreshed_node_ids == ["start"]
    assert release.version == 1
    assert preview.target.kind["kind"] == "terminal"  # type: ignore[union-attr]
    assert archived.status == "archived"
    assert publish_route.calls[0].request.content == b'{"expected_updated_at_ms":150}'
    assert publish_route.calls[1].request.content == b'{}'
    assert archive_route.calls[0].request.content == b'{"expected_updated_at_ms":150}'
    assert archive_route.calls[1].request.content == b'{}'

    create_body = json.loads(create_route.calls[0].request.content)
    assert "status" not in create_body and "version" not in create_body
    assert json.loads(update_route.calls[0].request.content)["expected_updated_at_ms"] == 101
    assert json.loads(refresh_route.calls[0].request.content) == {"expected_updated_at_ms": 150}
    assert json.loads(dry_run_route.calls[0].request.content) == {
        "from_node_id": "start", "transition_key": "complete", "artifact_type": "report", "version": 1,
    }
    assert all(route.called for route in [list_route, get_route, versions_route, version_route, stale_route, validate_route, publish_route, archive_route])
