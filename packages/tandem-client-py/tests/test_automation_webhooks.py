import json

import httpx
import pytest
import respx
from tandem_client import TandemClient
from tandem_client.types import AutomationWebhookTriggerUpdateInput

BASE = "http://localhost:39731"


def _trigger_payload() -> dict[str, object]:
    return {
        "trigger_id": "trigger-1",
        "automation_id": "automation-1",
        "name": "GitHub issues",
        "provider": "github",
        "provider_event_kind": "issues.opened",
        "enabled": True,
        "callback_url": "https://example.com/webhooks/automations/whpub_test",
        "provider_metadata": {
            "canonical_provider": "github",
            "event_id_headers": ["x-github-delivery", "x-tandem-webhook-event-id"],
            "verification": {"signature_scheme": "hmac_sha256_v1", "provider_specific": False},
            "polling": {"supported": False, "reconciliation_supported": False},
        },
        "secret_status": {"configured": True, "secret_version": 1},
        "delivery_counts": {"total": 1, "accepted": 1},
    }


@pytest.mark.asyncio
@respx.mock
async def test_automation_v2_webhook_trigger_management_routes() -> None:
    list_route = respx.get(f"{BASE}/automations/v2/automation-1/webhook-triggers").mock(
        return_value=httpx.Response(200, json={"triggers": [_trigger_payload()], "count": 1})
    )
    create_route = respx.post(f"{BASE}/automations/v2/automation-1/webhook-triggers").mock(
        return_value=httpx.Response(
            200,
            json={
                "trigger": _trigger_payload(),
                "new_secret": "whsec_new",
                "secret_one_time": True,
            },
        )
    )
    get_route = respx.get(
        f"{BASE}/automations/v2/automation-1/webhook-triggers/trigger-1"
    ).mock(return_value=httpx.Response(200, json={"trigger": _trigger_payload()}))
    update_route = respx.patch(
        f"{BASE}/automations/v2/automation-1/webhook-triggers/trigger-1"
    ).mock(return_value=httpx.Response(200, json={"trigger": _trigger_payload()}))
    disable_route = respx.post(
        f"{BASE}/automations/v2/automation-1/webhook-triggers/trigger-1/disable"
    ).mock(return_value=httpx.Response(200, json={"ok": True, "trigger": _trigger_payload()}))
    rotate_route = respx.post(
        f"{BASE}/automations/v2/automation-1/webhook-triggers/trigger-1/rotate-secret"
    ).mock(
        return_value=httpx.Response(
            200,
            json={
                "trigger": _trigger_payload(),
                "newSecret": "whsec_rotated",
                "secretOneTime": True,
            },
        )
    )
    deliveries_route = respx.get(
        f"{BASE}/automations/v2/automation-1/webhook-triggers/trigger-1/deliveries?limit=10"
    ).mock(
        return_value=httpx.Response(
            200,
            json={
                "deliveries": [
                    {
                        "delivery_id": "delivery-1",
                        "trigger_id": "trigger-1",
                        "automation_id": "automation-1",
                        "provider_event_id": "evt-1",
                        "status": "accepted",
                        "verification_scheme": "hmac_sha256_v1",
                        "verification_provider": "github",
                        "verification_reason_code": "verified",
                        "queued_run_id": "run-1",
                        "sanitized_preview": {"action": "opened"},
                    }
                ],
                "count": 1,
                "limit": 10,
            },
        )
    )
    delivery_route = respx.get(
        f"{BASE}/automations/v2/automation-1/webhook-triggers/trigger-1/deliveries/delivery-1"
    ).mock(
        return_value=httpx.Response(
            200,
            json={
                "delivery": {
                    "deliveryID": "delivery-1",
                    "triggerID": "trigger-1",
                    "automationID": "automation-1",
                    "providerEventID": "evt-1",
                    "status": "accepted",
                    "verificationScheme": "hmac_sha256_v1",
                    "verificationProvider": "github",
                    "verificationReasonCode": "verified",
                    "queuedRunID": "run-1",
                    "sanitizedPreview": {"action": "opened"},
                }
            },
        )
    )
    delete_route = respx.delete(
        f"{BASE}/automations/v2/automation-1/webhook-triggers/trigger-1"
    ).mock(
        return_value=httpx.Response(
            200, json={"ok": True, "deleted": True, "trigger_id": "trigger-1"}
        )
    )

    async with TandemClient(base_url=BASE, token="token") as client:
        listed = await client.automations_v2.list_webhook_triggers("automation-1")
        created = await client.automations_v2.create_webhook_trigger(
            "automation-1",
            {
                "provider": "github",
                "provider_event_kind": "issues.assigned",
                "providerEventKind": "issues.opened",
                "defaultDataClass": "customer_data",
                "defaultRiskTier": "internal_write",
                "owningOrgUnitId": "support",
                "resourceScope": {"root": {"resource_id": "automation-project"}},
            },
        )
        fetched = await client.automations_v2.get_webhook_trigger("automation-1", "trigger-1")
        updated = await client.automations_v2.update_webhook_trigger(
            "automation-1",
            "trigger-1",
            AutomationWebhookTriggerUpdateInput(
                providerEventKind=None,
                defaultDataClass="internal",
                defaultRiskTier=None,
            ),
        )
        disabled = await client.automations_v2.disable_webhook_trigger(
            "automation-1", "trigger-1"
        )
        rotated = await client.automations_v2.rotate_webhook_secret(
            "automation-1", "trigger-1"
        )
        deliveries = await client.automations_v2.list_webhook_deliveries(
            "automation-1", "trigger-1", limit=10
        )
        delivery = await client.automations_v2.get_webhook_delivery(
            "automation-1", "trigger-1", "delivery-1"
        )
        deleted = await client.automations_v2.delete_webhook_trigger(
            "automation-1", "trigger-1"
        )

    assert listed.count == 1
    assert listed.triggers[0].trigger_id == "trigger-1"
    assert listed.triggers[0].provider_metadata is not None
    assert listed.triggers[0].provider_metadata.canonical_provider == "github"
    assert listed.triggers[0].provider_metadata.event_id_headers[0] == "x-github-delivery"
    assert listed.triggers[0].delivery_counts is not None
    assert listed.triggers[0].delivery_counts.accepted == 1
    assert created.new_secret == "whsec_new"
    assert created.secret_one_time is True
    assert created.trigger.callback_url == "https://example.com/webhooks/automations/whpub_test"
    assert fetched.trigger.provider_event_kind == "issues.opened"
    assert updated.trigger.trigger_id == "trigger-1"
    assert disabled.trigger.enabled is True
    assert rotated.new_secret == "whsec_rotated"
    assert deliveries.limit == 10
    assert deliveries.deliveries[0].provider_event_id == "evt-1"
    assert deliveries.deliveries[0].verification_scheme == "hmac_sha256_v1"
    assert deliveries.deliveries[0].verification_provider == "github"
    assert deliveries.deliveries[0].verification_reason_code == "verified"
    assert delivery.delivery.delivery_id == "delivery-1"
    assert delivery.delivery.queued_run_id == "run-1"
    assert delivery.delivery.verification_reason_code == "verified"
    assert deleted.ok is True
    assert deleted.trigger_id == "trigger-1"

    create_body = json.loads(create_route.calls[0].request.content.decode("utf-8"))
    assert create_body == {
        "provider": "github",
        "provider_event_kind": "issues.assigned",
        "default_data_class": "customer_data",
        "default_risk_tier": "internal_write",
        "owning_org_unit_id": "support",
        "resource_scope": {"root": {"resource_id": "automation-project"}},
    }

    update_body = json.loads(update_route.calls[0].request.content.decode("utf-8"))
    assert update_body == {
        "provider_event_kind": None,
        "default_data_class": "internal",
        "default_risk_tier": None,
    }
    assert disable_route.calls[0].request.content == b"{}"
    assert list_route.called
    assert get_route.called
    assert rotate_route.called
    assert deliveries_route.called
    assert delivery_route.called
    assert delete_route.called
