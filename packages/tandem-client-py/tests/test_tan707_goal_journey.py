from __future__ import annotations

from datetime import datetime, timezone

import httpx
import pytest
import respx

from tandem_client import TandemClient


BASE = "http://localhost:39731"
GOAL_ID = "goal/tan-707"
GOAL_PATH = f"{BASE}/goals/goal%2Ftan-707"
DAY_MS = 24 * 60 * 60 * 1000
START_MS = int(datetime(2100, 1, 1, tzinfo=timezone.utc).timestamp() * 1000)


def day(value: int) -> int:
    return START_MS + value * DAY_MS


def tenant() -> dict[str, object]:
    return {"org_id": "local", "workspace_id": "local", "source": "local_implicit"}


def scope() -> dict[str, object]:
    return {"schema_version": 1, "tenant_context": tenant()}


def goal() -> dict[str, object]:
    return {
        "schema_version": 1,
        "goal_id": GOAL_ID,
        "orchestration_id": "tan-707-goal-loop",
        "orchestration_version": 1,
        "objective": "Deliver and verify the 180-day program",
        "status": "active",
        "tenant_context": tenant(),
        "policy": {
            "max_hops": 6,
            "deadline_at_ms": day(180),
            "max_total_tokens": 20_000,
            "max_total_cost_usd": 5,
            "on_limit": "pause_for_review",
        },
        "active_run_id": "run-verify",
        "current_node_id": "verify",
        "hop_count": 2,
        "total_tokens": 8_400,
        "total_cost_usd": 1.75,
        "created_at_ms": day(0),
        "updated_at_ms": day(179),
    }


def wait_record(kind: str, index: int) -> dict[str, object]:
    return {
        "schema_version": 1,
        "wait_id": f"wait/{kind}",
        "run_id": "run-execute",
        "wait_kind": kind,
        "status": "waiting" if kind == "external_condition" else "woken",
        "scope": scope(),
        "created_at_ms": day(30 * (index + 1)),
        "updated_at_ms": day(30 * (index + 1)),
    }


@pytest.mark.asyncio
@respx.mock
async def test_tan707_sdk_inspects_180_day_waits_lineage_budgets_and_duplicate_commit() -> None:
    waits = [
        wait_record(kind, index)
        for index, kind in enumerate(["timer", "approval", "webhook", "external_condition"])
    ]
    budgets = {
        "policy": goal()["policy"],
        "consumed": {"hops": 2, "total_tokens": 8_400, "total_cost_usd": 1.75},
        "remaining": {"hops": 4, "tokens": 11_600, "cost_usd": 3.25, "deadline_ms": DAY_MS},
    }
    links = [
        {"goal_id": GOAL_ID, "run_id": run_id, "orchestration_node_id": node, "orchestration_version": 1, "hop_index": index, "created_at_ms": day(created)}
        for index, (run_id, node, created) in enumerate(
            [("run-plan", "plan", 0), ("run-execute", "execute", 1), ("run-verify", "verify", 179)]
        )
    ]
    runs = [
        {
            "link": link,
            "run": {
                "run_id": link["run_id"],
                "automation_id": f"tan-707-{link['orchestration_node_id']}",
                "tenant_context": tenant(),
                "trigger_type": "orchestration",
                "status": "running" if link["run_id"] == "run-verify" else "completed",
                "created_at_ms": link["created_at_ms"],
                "updated_at_ms": link["created_at_ms"],
            },
        }
        for link in links
    ]
    handoff = {
        "schema_version": 1,
        "handoff_id": "handoff-planned",
        "idempotency_key": "day-1-planned",
        "goal_id": GOAL_ID,
        "orchestration_id": "tan-707-goal-loop",
        "orchestration_version": 1,
        "tenant_context": tenant(),
        "edge_id": "plan-execute",
        "transition_key": "planned",
        "source_automation_id": "tan-707-plan",
        "source_run_id": "run-plan",
        "source_node_id": "plan",
        "target_automation_id": "tan-707-execute",
        "target_node_id": "execute",
        "artifact": {"artifact_type": "plan", "value": {"proof": "TAN-707"}},
        "status": "consumed",
        "created_at_ms": day(1),
        "updated_at_ms": day(1),
    }

    respx.get(f"{GOAL_PATH}/runs").mock(
        return_value=httpx.Response(200, json={"goal_id": GOAL_ID, "active_run_id": "run-verify", "runs": runs, "count": 3})
    )
    respx.get(f"{GOAL_PATH}/waits").mock(
        return_value=httpx.Response(200, json={"goal_id": GOAL_ID, "waits": waits, "count": 4})
    )
    respx.get(f"{GOAL_PATH}/budgets").mock(
        return_value=httpx.Response(200, json={"goal_id": GOAL_ID, "status": "active", "budgets": budgets})
    )
    transition_route = respx.post(f"{GOAL_PATH}/transitions").mock(
        return_value=httpx.Response(200, json={
            "outcome": "committed",
            "commit": "AlreadyCommitted",
            "handoff": handoff,
            "downstream_run_id": "run-execute",
            "link": links[1],
            "goal": goal(),
        })
    )

    async with TandemClient(base_url=BASE, token="tan-707-test-token") as client:
        lineage = await client.stateful_runtime.list_goal_runs(GOAL_ID)
        durable_waits = await client.stateful_runtime.list_goal_waits(GOAL_ID)
        budget_view = await client.stateful_runtime.get_goal_budgets(GOAL_ID)
        duplicate = await client.stateful_runtime.emit_goal_handoff(
            GOAL_ID,
            transition_key="planned",
            artifact={"artifact_type": "plan", "value": {"proof": "TAN-707"}},
            idempotency_key="day-1-planned",
        )

    assert [item.link.hop_index for item in lineage.runs] == [0, 1, 2]
    assert {item.wait_kind for item in durable_waits.waits} == {
        "timer", "approval", "webhook", "external_condition"
    }
    assert budget_view.budgets.remaining.hops == 4
    assert goal()["policy"]["deadline_at_ms"] - goal()["created_at_ms"] == 180 * DAY_MS  # type: ignore[operator,index]
    assert duplicate.commit == "AlreadyCommitted" and duplicate.downstream_run_id == "run-execute"
    assert transition_route.called
