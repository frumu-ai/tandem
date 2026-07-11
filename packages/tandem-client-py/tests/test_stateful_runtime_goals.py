import json

import httpx
import pytest
import respx

from tandem_client import StatefulGoalAutomationRunRecord, StatefulGoalEventRecord, TandemClient


BASE = "http://localhost:39731"


def _tenant() -> dict[str, object]:
    return {"org_id": "local", "workspace_id": "local", "source": "local_implicit"}


def _scope() -> dict[str, object]:
    return {"schema_version": 1, "tenant_context": _tenant(), "data_classes": [], "delegation_grant_ids": []}


def _artifact() -> dict[str, object]:
    return {"artifact_type": "report", "value": {"result": "ok"}}


def _goal(status: str = "active") -> dict[str, object]:
    return {
        "schema_version": 1, "goal_id": "goal/1", "orchestration_id": "goal-loop",
        "orchestration_version": 3, "objective": "Produce report", "status": status,
        "tenant_context": _tenant(),
        "policy": {"max_hops": 5, "max_total_tokens": 10000, "max_total_cost_usd": 2.5, "on_limit": "pause_for_review"},
        "active_run_id": "run-1" if status == "active" else None, "current_node_id": "start",
        "hop_count": 1, "total_tokens": 120, "total_cost_usd": 0.04,
        "created_at_ms": 100, "updated_at_ms": 110,
    }


def _budgets() -> dict[str, object]:
    return {
        "policy": _goal()["policy"],
        "consumed": {"hops": 1, "total_tokens": 120, "total_cost_usd": 0.04},
        "remaining": {"hops": 4, "tokens": 9880, "cost_usd": 2.46, "deadline_ms": None},
    }


@pytest.mark.parametrize("status", ["blocked", "awaiting_approval"])
def test_goal_run_accepts_all_governed_automation_statuses(status: str) -> None:
    run = StatefulGoalAutomationRunRecord.model_validate({
        "run_id": "run-1",
        "automation_id": "automation-1",
        "tenant_context": _tenant(),
        "trigger_type": "orchestration",
        "status": status,
        "created_at_ms": 100,
        "updated_at_ms": 101,
    })

    assert run.status == status


def _event() -> dict[str, object]:
    return {
        "goal_seq": 1, "schema_version": 1, "event_id": "event-1", "run_id": "run-1",
        "seq": 1, "event_type": "orchestration.goal.started", "occurred_at_ms": 102,
        "scope": _scope(), "payload": {"goal_id": "goal/1"},
    }


def _handoff(status: str = "consumed") -> dict[str, object]:
    return {
        "schema_version": 1, "handoff_id": "handoff-1", "idempotency_key": "emit-1",
        "goal_id": "goal/1", "orchestration_id": "goal-loop", "orchestration_version": 3,
        "tenant_context": _tenant(), "edge_id": "edge-1", "transition_key": "continue",
        "source_automation_id": "automation-1", "source_run_id": "run-1", "source_node_id": "start",
        "target_automation_id": "automation-2", "target_node_id": "finish", "artifact": _artifact(),
        "status": status, "created_at_ms": 105, "updated_at_ms": 106,
    }


@pytest.mark.asyncio
@respx.mock
async def test_canonical_goal_routes_completion_and_cursor_contracts() -> None:
    goal = _goal()
    path = f"{BASE}/goals/goal%2F1"
    link = {"goal_id": "goal/1", "run_id": "run-1", "orchestration_node_id": "start", "orchestration_version": 3, "hop_index": 0, "created_at_ms": 101}
    run = {
        "run_id": "run-1",
        "automation_id": "automation-1",
        "tenant_context": _tenant(),
        "trigger_type": "orchestration",
        "status": "running",
        "created_at_ms": 100,
        "updated_at_ms": 101,
    }
    event = _event()
    wait = {"schema_version": 1, "wait_id": "wait/1", "run_id": "run-1", "wait_kind": "timer", "status": "waiting", "scope": _scope(), "created_at_ms": 103, "updated_at_ms": 104}
    handoff = _handoff()

    list_route = respx.get(f"{BASE}/goals", params={"limit": "25", "status": "active", "orchestration_id": "goal-loop"}).mock(
        return_value=httpx.Response(200, json={"goals": [goal], "count": 1})
    )
    start_route = respx.post(f"{BASE}/goals").mock(return_value=httpx.Response(201, json={"goal": goal, "root_run_id": "run-1", "replayed": False}))
    get_route = respx.get(path).mock(return_value=httpx.Response(200, json={"goal": goal, "goal_id": "goal/1", "status": "active", "budgets": _budgets()}))
    graph_route = respx.get(f"{path}/graph").mock(return_value=httpx.Response(200, json={
        "goal_id": "goal/1", "status": "active", "orchestration_id": "goal-loop", "orchestration_version": 3,
        "current_node_id": "start", "current_workflow": {"run_id": "run-1", "status": "running"},
        "nodes": [{"node_id": "start", "name": "Start", "kind": {"kind": "workflow", "automation_id": "automation-1"}, "state": "current", "runs": []}],
        "edges": [], "budgets": _budgets(),
    }))
    runs_route = respx.get(f"{path}/runs").mock(return_value=httpx.Response(200, json={"goal_id": "goal/1", "active_run_id": "run-1", "runs": [{"link": link, "run": run}], "count": 1}))
    events_route = respx.get(f"{path}/events", params={"cursor": "7", "limit": "10"}).mock(return_value=httpx.Response(200, json={
        "goal_id": "goal/1", "events": [{"cursor": 8, "event": event}], "count": 1,
        "last_cursor": 8, "event_source": "stateful_runtime",
    }))
    handoffs_route = respx.get(f"{path}/handoffs").mock(return_value=httpx.Response(200, json={"goal_id": "goal/1", "handoffs": [handoff], "count": 1}))
    waits_route = respx.get(f"{path}/waits").mock(return_value=httpx.Response(200, json={"goal_id": "goal/1", "waits": [wait], "count": 1}))
    inspect_route = respx.get(f"{path}/waits/wait%2F1").mock(return_value=httpx.Response(200, json={"goal_id": "goal/1", "wait": wait}))
    resolved_wait = {**wait, "status": "woken", "completed_at_ms": 120}
    resolve_route = respx.post(f"{path}/waits/wait%2F1/resolve").mock(return_value=httpx.Response(200, json={"goal_id": "goal/1", "wait": resolved_wait}))
    artifacts_route = respx.get(f"{path}/artifacts").mock(return_value=httpx.Response(200, json={
        "goal_id": "goal/1", "artifacts": [{"artifact": _artifact(), "handoff_id": "handoff-1", "transition_key": "continue", "source_run_id": "run-1", "consumed_by_run_id": None, "created_at_ms": 105}],
        "final_artifact": None, "count": 1,
    }))
    budgets_route = respx.get(f"{path}/budgets").mock(return_value=httpx.Response(200, json={"goal_id": "goal/1", "status": "active", "budgets": _budgets()}))
    pause_route = respx.post(f"{path}/pause").mock(return_value=httpx.Response(200, json={"goal": _goal("paused"), "outcome": "paused"}))
    resume_route = respx.post(f"{path}/resume").mock(return_value=httpx.Response(200, json={"goal": goal, "outcome": "resumed"}))
    cancelled = _goal("cancelled")
    cancel_route = respx.post(f"{path}/cancel").mock(return_value=httpx.Response(200, json={"goal": cancelled, "outcome": "Applied", "cancelled_run_id": "run-1", "cancelled_wait_ids": ["wait/1"], "dead_lettered_handoff_ids": []}))
    transition_route = respx.post(f"{path}/transitions").mock(return_value=httpx.Response(200, json={"outcome": "committed", "commit": "Committed", "handoff": handoff, "downstream_run_id": "run-2", "link": link, "goal": goal}))
    decision_route = respx.post(f"{path}/handoffs/handoff%2F1/decision").mock(return_value=httpx.Response(200, json={"handoff": _handoff("rejected")}))
    completion_route = respx.post(f"{path}/completion").mock(return_value=httpx.Response(200, json={"outcome": "terminal", "goal": _goal("completed")}))
    stream_route = respx.get(f"{path}/events/stream", params={"cursor": "7"}).mock(return_value=httpx.Response(
        200, headers={"Content-Type": "text/event-stream"},
        content=("event: ready\ndata: {\"goal_id\":\"goal/1\",\"cursor\":7,\"timestamp_ms\":100}\n\n"
                 "id: 8\nevent: orchestration.goal.started\ndata: " + json.dumps({"cursor": 8, "event": event}) + "\n\n"),
    ))

    async with TandemClient(base_url=BASE, token="token") as client:
        listed = await client.stateful_runtime.list_goals(limit=25, status="active", orchestration_id="goal-loop")
        started = await client.stateful_runtime.start_goal(orchestration_id="goal-loop", objective="Produce report", idempotency_key="start-1")
        fetched = await client.stateful_runtime.get_goal("goal/1")
        graph = await client.stateful_runtime.get_goal_graph("goal/1")
        runs = await client.stateful_runtime.list_goal_runs("goal/1")
        events = await client.stateful_runtime.list_goal_events("goal/1", cursor=7, limit=10)
        handoffs = await client.stateful_runtime.list_goal_handoffs("goal/1")
        waits = await client.stateful_runtime.list_goal_waits("goal/1")
        inspected = await client.stateful_runtime.get_goal_wait("goal/1", "wait/1")
        resolved = await client.stateful_runtime.resolve_goal_wait("goal/1", "wait/1", idempotency_key="resolve-1", payload={"ok": True})
        artifacts = await client.stateful_runtime.list_goal_artifacts("goal/1")
        budgets = await client.stateful_runtime.get_goal_budgets("goal/1")
        paused = await client.stateful_runtime.pause_goal("goal/1", reason="review")
        resumed = await client.stateful_runtime.resume_goal("goal/1")
        canceled = await client.stateful_runtime.cancel_goal("goal/1", reason="stop")
        transition = await client.stateful_runtime.emit_goal_handoff("goal/1", transition_key="continue", artifact=_artifact(), idempotency_key="emit-1")
        decision = await client.stateful_runtime.decide_goal_handoff("goal/1", "handoff/1", approve=False, reason="not ready")
        completion = await client.stateful_runtime.settle_goal_completion("goal/1", transition_key="complete", final_artifact=_artifact())
        streamed = [item async for item in client.stateful_runtime.events("goal/1", cursor=7)]

    assert listed.count == 1 and started.root_run_id == "run-1"
    assert fetched.budgets.remaining.hops == 4  # type: ignore[union-attr]
    assert graph.current_workflow["run_id"] == "run-1"  # type: ignore[index]
    assert runs.goal_id == "goal/1"
    assert isinstance(events.events[0].event, StatefulGoalEventRecord)
    assert events.events[0].cursor == events.last_cursor == 8
    assert handoffs.goal_id == waits.goal_id == "goal/1"
    assert inspected.wait.wait_id == "wait/1" and resolved.wait.status == "woken"
    assert artifacts.artifacts[0].transition_key == "continue"
    assert budgets.budgets.consumed.total_tokens == 120
    assert paused.outcome == "paused" and resumed.outcome == "resumed"
    assert canceled.cancelled_run_id == "run-1"
    assert transition.downstream_run_id == "run-2"
    assert decision.handoff.status == "rejected"
    assert completion.outcome == "terminal"
    assert [(item.type, item.cursor) for item in streamed] == [("orchestration.goal.started", 8)]

    assert json.loads(start_route.calls[0].request.content) == {"orchestration_id": "goal-loop", "objective": "Produce report", "idempotency_key": "start-1"}
    assert "Idempotency-Key" not in start_route.calls[0].request.headers
    assert json.loads(transition_route.calls[0].request.content)["idempotency_key"] == "emit-1"
    assert json.loads(decision_route.calls[0].request.content) == {"decision": "reject", "reason": "not ready"}
    assert json.loads(completion_route.calls[0].request.content)["transition_key"] == "complete"
    assert all(route.called for route in [list_route, get_route, graph_route, runs_route, events_route, handoffs_route, waits_route, inspect_route, resolve_route, artifacts_route, budgets_route, pause_route, resume_route, cancel_route, stream_route])
