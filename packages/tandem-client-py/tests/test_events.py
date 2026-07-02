import json
from pathlib import Path

import httpx
import pytest
import respx
from pydantic import TypeAdapter
from tandem_client import TandemClient
from tandem_client.types import (
    IncidentMonitorAuthorityInventoryResponse,
    IncidentMonitorAssessmentReportResponse,
    IncidentMonitorAssessmentProbeRunResponse,
    IncidentMonitorConfigResponse,
    IncidentMonitorDeploymentCardsResponse,
    IncidentMonitorDraftRecord,
    IncidentMonitorIncidentRecord,
    IncidentMonitorPostRecord,
    IncidentMonitorPostureChecksResponse,
    IncidentMonitorRoutePreviewResponse,
    EngineEvent,
)

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


def test_incident_monitor_destination_router_types_accept_payloads() -> None:
    config = IncidentMonitorConfigResponse.model_validate(
        {
            "incident_monitor": {
                "enabled": True,
                "repo": "frumu-ai/tandem",
                "destinations": [
                    {
                        "destination_id": "legacy-github",
                        "name": "GitHub (legacy Incident Monitor)",
                        "kind": "github_issue",
                        "repo": "frumu-ai/tandem",
                        "mcp_server": "github",
                        "route_tags": ["legacy_github"],
                    }
                ],
                "routes": [
                    {
                        "route_id": "default",
                        "name": "Default route",
                        "destination_ids": ["legacy-github"],
                        "approval_policy": "inherit",
                        "match_source_kinds": ["ci"],
                        "match_risk_categories": ["data_exfiltration"],
                        "match_route_tags": ["payments"],
                    }
                ],
                "default_destination_ids": ["legacy-github"],
                "safety_defaults": {
                    "require_approval_for_high_risk": True,
                    "redact_secrets": True,
                },
                "monitored_projects": [
                    {
                        "project_id": "payments",
                        "name": "Payments",
                        "repo": "acme/payments",
                        "workspace_root": "/tmp/payments",
                        "source_kind": "external_app",
                        "allowed_destination_ids": ["legacy-github"],
                        "default_route_tags": ["payments"],
                        "tenant_id": "tenant-a",
                        "log_sources": [
                            {
                                "source_id": "ci",
                                "path": "logs/ci.jsonl",
                                "source_kind": "ci",
                                "default_destination_ids": ["legacy-github"],
                                "default_route_tags": ["ci"],
                                "workspace_id": "workspace-a",
                            }
                        ],
                    }
                ],
            }
        }
    )
    preview = IncidentMonitorRoutePreviewResponse.model_validate(
        {
            "matches": [{"destination_ids": ["legacy-github"], "approval_required": False}],
            "destinations": config.incident_monitor.destinations,
            "source_readiness": [
                {
                    "project_id": "payments",
                    "source_id": "ci",
                    "source_kind": "ci",
                    "enabled": True,
                    "ready": False,
                    "missing": ["redaction_profile"],
                    "warnings": [
                        "high: Source `payments/ci` is missing redaction profile coverage"
                    ],
                    "findings": [
                        {
                            "finding_id": "srf_example",
                            "rule_id": "source_redaction_profile_missing",
                            "category": "source_protection",
                            "severity": "high",
                            "title": "Source `payments/ci` is missing redaction profile coverage",
                            "detail": "Production source readiness requires a redaction profile.",
                            "evidence_refs": [
                                "incident_monitor.config.monitored_projects[].log_sources[ci].redaction_profile"
                            ],
                            "recommendation": "Attach a redaction_profile to the source binding.",
                        }
                    ],
                }
            ],
            "source_readiness_warnings": [
                "high: Source `payments/ci` is missing redaction profile coverage"
            ],
            "default_destination_ids": ["legacy-github"],
            "effective_destination_ids": ["legacy-github"],
        }
    )
    post = IncidentMonitorPostRecord.model_validate(
        {
            "post_id": "post-1",
            "draft_id": "draft-1",
            "repo": "frumu-ai/tandem",
            "operation": "create_issue",
            "status": "posted",
            "destination_id": "legacy-github",
            "destination_kind": "github_issue",
            "external_url": "https://github.com/frumu-ai/tandem/issues/42",
            "receipt": {"provider": "github", "issue_number": 42},
        }
    )
    incident = IncidentMonitorIncidentRecord.model_validate(
        {
            "incident_id": "incident-1",
            "risk_category": "data_exfiltration",
            "actor": "agent-release",
            "model": "gpt-5",
            "tool_name": "slack.post_message",
            "action": "send_message",
            "policy": "approval.high_risk",
            "approval_state": "denied",
            "blast_radius": "customer channel",
            "external_correlation_ids": ["case-123"],
        }
    )
    draft = IncidentMonitorDraftRecord.model_validate(
        {
            "draft_id": "draft-1",
            "risk_category": "data_exfiltration",
            "actor": "agent-release",
            "external_correlation_ids": ["case-123"],
        }
    )

    assert config.incident_monitor.destinations[0].destination_id == "legacy-github"
    assert config.incident_monitor.routes[0].match_risk_categories == ["data_exfiltration"]
    assert config.incident_monitor.monitored_projects[0].source_kind == "external_app"
    assert config.incident_monitor.monitored_projects[0].log_sources[0].source_kind == "ci"
    assert preview.effective_destination_ids == ["legacy-github"]
    assert preview.source_readiness[0].findings[0].rule_id == "source_redaction_profile_missing"
    assert "redaction profile" in preview.source_readiness_warnings[0]
    assert post.receipt["issue_number"] == 42
    assert incident.risk_category == "data_exfiltration"
    assert draft.external_correlation_ids == ["case-123"]


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
            limit=5, workflow_mode="issue_triage", repo_slug="user123/tandem"
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
    respx.get("http://localhost:39731/incident-monitor/drafts").mock(
        return_value=httpx.Response(200, json={"drafts": [], "count": 0})
    )
    route_preview_route = respx.post("http://localhost:39731/incident-monitor/route-preview").mock(
        return_value=httpx.Response(
            200,
            json={
                "matches": [{"destination_ids": ["legacy-github"]}],
                "effective_destination_ids": ["legacy-github"],
            },
        )
    )
    posts_route = respx.get(
        "http://localhost:39731/incident-monitor/posts?limit=25&destination_id=legacy-github"
    ).mock(return_value=httpx.Response(200, json={"posts": [], "count": 0}))
    approve_draft_route = respx.post("http://localhost:39731/incident-monitor/drafts/d-1/approve").mock(
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
        drafts = await client.incident_monitor.list_drafts(limit=5)
        preview = await client.incident_monitor.preview_route({"source": "desktop_logs"})
        posts = await client.incident_monitor.list_posts(limit=25, destination_id="legacy-github")
        await client.incident_monitor.approve_draft("d-1", "ship it")
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
    assert preview.effective_destination_ids == ["legacy-github"]
    assert route_preview_route.called
    assert posts.count == 0
    assert posts_route.called
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
async def test_incident_monitor_destination_router_sdk_helpers() -> None:
    base_config = {
        "incident_monitor": {
            "enabled": True,
            "destinations": [
                {
                    "destination_id": "legacy-github",
                    "name": "GitHub",
                    "kind": "github_issue",
                    "enabled": True,
                },
                {
                    "destination_id": "linear-primary",
                    "name": "Linear",
                    "kind": "linear_issue",
                    "enabled": True,
                }
            ],
            "routes": [
                {
                    "route_id": "default",
                    "name": "Default",
                    "destination_ids": ["legacy-github"],
                },
                {
                    "route_id": "high-risk",
                    "name": "High risk",
                    "destination_ids": ["linear-primary"],
                    "match_risk_levels": ["high"],
                }
            ],
            "default_destination_ids": ["legacy-github"],
        }
    }
    get_config_route = respx.get("http://localhost:39731/config/incident-monitor").mock(
        return_value=httpx.Response(200, json=base_config)
    )
    patch_config_route = respx.patch("http://localhost:39731/config/incident-monitor").mock(
        return_value=httpx.Response(200, json=base_config)
    )
    publish_route = respx.post("http://localhost:39731/incident-monitor/drafts/draft-1/publish").mock(
        return_value=httpx.Response(200, json={"ok": True})
    )

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        destinations = await client.incident_monitor.list_destinations()
        await client.incident_monitor.upsert_destination(
            {
                "destination_id": "linear-primary",
                "name": "Linear",
                "kind": "linear_issue",
                "enabled": True,
            }
        )
        await client.incident_monitor.upsert_route(
            {
                "route_id": "high-risk",
                "name": "High risk",
                "destination_ids": ["linear-primary"],
                "match_risk_levels": ["high"],
            }
        )
        await client.incident_monitor.remove_destination("linear-primary")
        await client.incident_monitor.publish_draft_to_destinations(
            "draft-1",
            ["legacy-github"],
            {"reason": "ship it"},
        )

    assert destinations[0].destination_id == "legacy-github"
    assert get_config_route.call_count == 4
    assert patch_config_route.call_count == 3
    upsert_destination_payload = json.loads(
        patch_config_route.calls[0].request.content.decode("utf-8")
    )
    assert (
        upsert_destination_payload["incident_monitor"]["destinations"][1]["destination_id"]
        == "linear-primary"
    )
    upsert_route_payload = json.loads(patch_config_route.calls[1].request.content.decode("utf-8"))
    assert upsert_route_payload["incident_monitor"]["routes"][1]["route_id"] == "high-risk"
    remove_destination_payload = json.loads(
        patch_config_route.calls[2].request.content.decode("utf-8")
    )
    remove_routes = remove_destination_payload["incident_monitor"]["routes"]
    assert [route["route_id"] for route in remove_routes] == ["default"]
    assert remove_routes[0]["destination_ids"] == ["legacy-github"]
    publish_payload = json.loads(publish_route.calls[0].request.content.decode("utf-8"))
    assert publish_payload == {
        "reason": "ship it",
        "destination_ids": ["legacy-github"],
    }


@pytest.mark.asyncio
@respx.mock
async def test_incident_monitor_authority_inventory_sdk_helper() -> None:
    inventory_payload = {
        "schema_version": 1,
        "scope": {
            "source": "incident_monitor_authority_inventory",
            "read_only": True,
        },
        "inventory": {
            "workflows": [{"workflow_id": "wf-1", "enabled": True}],
            "automation_specs": [{"automation_id": "auto-1"}],
            "mcp": {"servers": [{"server": "github", "tool_count": 1}]},
            "destinations": [
                {
                    "destination_id": "linear-prod",
                    "kind": "linear_issue",
                    "require_approval": True,
                }
            ],
            "routes": [{"route_id": "high-risk", "destination_ids": ["linear-prod"]}],
            "monitored_sources": [{"project_id": "payments", "source_kind": "ci"}],
            "scoped_intake_keys": [
                {
                    "key_id": "key-1",
                    "project_id": "payments",
                    "key_hash_present": True,
                }
            ],
            "approval_rules": [{"rule_id": "destination:linear-prod"}],
            "external_publish_surfaces": {
                "configured_destinations": [{"surface_id": "linear-prod"}]
            },
        },
        "counts": {"workflows": 1, "automation_specs": 1, "destinations": 1},
        "sensitive_values": {"policy": "redacted_or_summarized"},
    }
    inventory_route = respx.get(
        "http://localhost:39731/incident-monitor/security/authority-inventory"
    ).mock(return_value=httpx.Response(200, json=inventory_payload))

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        inventory = await client.incident_monitor.get_authority_inventory()

    typed_inventory = IncidentMonitorAuthorityInventoryResponse.model_validate(inventory_payload)
    assert inventory.schema_version == typed_inventory.schema_version
    assert inventory.inventory.scoped_intake_keys[0]["key_hash_present"] is True
    assert inventory.inventory.destinations[0]["require_approval"] is True
    assert inventory_route.called


@pytest.mark.asyncio
@respx.mock
async def test_incident_monitor_posture_checks_sdk_helper() -> None:
    posture_payload = {
        "schema_version": 1,
        "scope": {
            "source": "incident_monitor_security_posture_checks",
            "read_only": True,
            "dry_run": True,
        },
        "baseline_policy": {"mode": "dry_run"},
        "findings": [
            {
                "finding_id": "bpf_123",
                "fingerprint": "sha256:abc",
                "rule_id": "mcp_server_without_tool_allowlist",
                "category": "mcp_allowlist_gap",
                "severity": "high",
                "title": "MCP server missing tool allowlist",
                "incident_draft_suggestion": {
                    "source": "security_posture",
                    "event_type": "security.posture.finding",
                },
            }
        ],
        "counts": {
            "findings": 1,
            "by_severity": {"high": 1},
            "by_category": {"mcp_allowlist_gap": 1},
        },
    }
    posture_route = respx.get(
        "http://localhost:39731/incident-monitor/security/posture-checks",
        params={
            "rules": "mcp_server_without_tool_allowlist",
            "min_severity": "medium",
        },
    ).mock(return_value=httpx.Response(200, json=posture_payload))

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        posture = await client.incident_monitor.get_posture_checks(
            rules=["mcp_server_without_tool_allowlist"],
            min_severity="medium",
        )

    typed_posture = IncidentMonitorPostureChecksResponse.model_validate(posture_payload)
    assert posture.schema_version == typed_posture.schema_version
    assert posture.findings[0].category == "mcp_allowlist_gap"
    assert posture.counts is not None
    assert posture.counts.by_severity["high"] == 1
    assert posture_route.called


@pytest.mark.asyncio
@respx.mock
async def test_incident_monitor_assessment_probes_sdk_helper() -> None:
    probes_payload = {
        "schema_version": 1,
        "scope": {
            "source": "incident_monitor_security_assessment_probes",
            "read_only": True,
            "dry_run": True,
        },
        "probe_policy": {
            "mode": "dry_run",
            "selected_probe_ids": ["webhook_url_policy"],
        },
        "results": [
            {
                "probe_id": "webhook_url_policy",
                "status": "fail",
                "expected_behavior": "Webhook destinations must use public HTTPS URLs.",
                "observed_behavior": "Webhook URL points to localhost/private network",
                "incident_draft_suggestion": {
                    "source": "security_assessment_probe",
                },
            }
        ],
        "counts": {
            "results": 1,
            "fail": 1,
            "by_status": {"fail": 1},
            "draft_suggestions": 1,
        },
        "evidence_pack": {
            "persisted": True,
            "context_run_id": "incident-monitor-assessment-probes-1",
        },
    }
    probes_route = respx.post(
        "http://localhost:39731/incident-monitor/security/assessment-probes",
        json={"probes": ["webhook_url_policy"]},
    ).mock(return_value=httpx.Response(200, json=probes_payload))

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        probes = await client.incident_monitor.run_assessment_probes(
            probes=["webhook_url_policy"],
        )

    typed_probes = IncidentMonitorAssessmentProbeRunResponse.model_validate(probes_payload)
    assert probes.schema_version == typed_probes.schema_version
    assert probes.results[0].probe_id == "webhook_url_policy"
    assert probes.counts is not None
    assert probes.counts.fail == 1
    assert probes_route.called


@pytest.mark.asyncio
@respx.mock
async def test_incident_monitor_assessment_report_sdk_helper() -> None:
    report_payload = {
        "schema_version": 1,
        "scope": {
            "source": "incident_monitor_security_gap_assessment_report",
            "read_only": True,
        },
        "counts": {
            "findings": 1,
            "protected_audit_events": 1,
        },
        "sections": {
            "self_monitoring_boundary": {
                "source_kinds": ["tandem_runtime", "tandem_monitor"],
                "external_export_required_for_high_assurance": True,
            },
            "external_audit_export": {
                "existing_ndjson_endpoint": "/audit/export",
                "records": [{"event_type": "incident_monitor.publish.failed"}],
            },
        },
        "markdown_summary": "# Incident Monitor Security Gap Assessment",
        "evidence_pack": {
            "persisted": True,
            "context_run_id": "incident-monitor-assessment-report-1",
        },
    }
    report_route = respx.post(
        "http://localhost:39731/incident-monitor/security/assessment-report",
        json={
            "source_kind": "tandem_monitor",
            "include_probe_results": True,
            "persist_artifact": True,
            "route_destination_ids": ["audit-webhook"],
        },
    ).mock(return_value=httpx.Response(200, json=report_payload))

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        report = await client.incident_monitor.generate_assessment_report(
            source_kind="tandem_monitor",
            include_probe_results=True,
            persist_artifact=True,
            route_destination_ids=["audit-webhook"],
        )

    typed_report = IncidentMonitorAssessmentReportResponse.model_validate(report_payload)
    assert report.schema_version == typed_report.schema_version
    assert report.sections is not None
    assert report.sections["self_monitoring_boundary"]["source_kinds"][1] == "tandem_monitor"
    assert report.markdown_summary is not None
    assert "Security Gap Assessment" in report.markdown_summary
    assert report_route.called


@pytest.mark.asyncio
@respx.mock
async def test_incident_monitor_deployment_cards_sdk_helper() -> None:
    cards_payload = {
        "schema_version": 1,
        "scope": {
            "source": "incident_monitor_deployment_cards",
            "read_only": True,
        },
        "cards": [
            {
                "card_id": "automation:auto-1",
                "card_kind": "automation",
                "business_owner": "Security Ops",
                "linked_evidence": {"operator_refs": ["runbook:auto-1"]},
            }
        ],
        "findings": [],
        "markdown_export": "# Incident Monitor Deployment Cards",
    }
    cards_route = respx.post(
        "http://localhost:39731/incident-monitor/security/deployment-cards",
        json={
            "defaults": {
                "business_owner": "Security Ops",
                "review_cadence_days": 30,
            },
            "metadata": {
                "automation:auto-1": {
                    "intended_purpose": "Govern payment incident follow-up",
                    "evidence_refs": ["runbook:auto-1"],
                }
            },
            "include_markdown": True,
            "include_raw_inventory": True,
        },
    ).mock(return_value=httpx.Response(200, json=cards_payload))

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        cards = await client.incident_monitor.generate_deployment_cards(
            defaults={"business_owner": "Security Ops", "review_cadence_days": 30},
            metadata={
                "automation:auto-1": {
                    "intended_purpose": "Govern payment incident follow-up",
                    "evidence_refs": ["runbook:auto-1"],
                }
            },
            include_markdown=True,
            include_raw_inventory=True,
        )

    typed_cards = IncidentMonitorDeploymentCardsResponse.model_validate(cards_payload)
    assert cards.schema_version == typed_cards.schema_version
    assert cards.cards[0]["card_id"] == "automation:auto-1"
    assert cards.markdown_export is not None
    assert "Deployment Cards" in cards.markdown_export
    assert cards_route.called


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
                ,
                "plan_package_bundle": {"bundle": "preview"},
                "plan_package_validation": {"compatible": True},
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
                "plan_package_bundle": {"bundle": "chat"},
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
                "plan_package_bundle": {"bundle": "message"},
            },
        )
    )
    import_preview_route = respx.post("http://localhost:39731/workflow-plans/import/preview").mock(
        return_value=httpx.Response(
            200,
            json={
                "ok": True,
                "bundle": {"bundle": "import"},
                "import_validation": {"compatible": True},
                "plan_package_preview": {"plan_id": "plan-1"},
                "derived_scope_snapshot": {"plan_id": "plan-1"},
                "summary": {"plan_id": "plan-1"},
            },
        )
    )
    import_route = respx.post("http://localhost:39731/workflow-plans/import").mock(
        return_value=httpx.Response(
            200,
            json={
                "ok": True,
                "bundle": {"bundle": "import"},
                "import_validation": {"compatible": True},
                "plan_package_preview": {"plan_id": "plan-1"},
                "derived_scope_snapshot": {"plan_id": "plan-1"},
                "summary": {"plan_id": "plan-1"},
            },
        )
    )

    async with TandemClient(base_url="http://localhost:39731", token="token") as client:
        preview = await client.workflow_plans.preview(prompt="Create a release checklist")
        started = await client.workflow_plans.chat_start(prompt="Create a release checklist")
        messaged = await client.workflow_plans.chat_message(
            plan_id="plan-1", message="Add smoke tests."
        )
        imported_preview = await client.workflow_plans.import_preview(bundle={"bundle": "import"})
        imported = await client.workflow_plans.import_plan(bundle={"bundle": "import"})

    assert preview.plan.plan_id == "plan-1"
    assert preview.plan.steps[0].objective == "Review changelog"
    assert started.conversation.conversation_id == "conv-1"
    assert messaged.change_summary == ["Added smoke-test step."]
    assert imported_preview.import_validation == {"compatible": True}
    assert imported.plan_package_preview == {"plan_id": "plan-1"}
    assert preview_route.called
    assert chat_start_route.called
    assert chat_message_route.called
    assert import_preview_route.called
    assert import_route.called


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
def test_sync_wrapper_supports_storage_namespace() -> None:
    from tandem_client import SyncTandemClient

    files_route = respx.get("http://localhost:39731/global/storage/files").mock(
        return_value=httpx.Response(
            200,
            json={
                "root": "/tmp/tandem",
                "base": "/tmp/tandem/data/context-runs",
                "files": [],
                "count": 0,
                "limit": 25,
            },
        )
    )
    repair_route = respx.post("http://localhost:39731/global/storage/repair").mock(
        return_value=httpx.Response(200, json={"status": "ok", "marker_updated": False})
    )
    client = SyncTandemClient(base_url="http://localhost:39731", token="token")
    try:
        listed = client.storage.list_files(path="data/context-runs", limit=25)
        repaired = client.storage.repair(force=True)
        assert listed.count == 0
        assert repaired.status == "ok"
        assert files_route.called
        assert repair_route.called
        assert repair_route.calls[0].request.content == b'{"force":true}'
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
