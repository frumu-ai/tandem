---
title: Enterprise Client Onboarding Runbook
description: Fast pilot and hardening checklist for bringing enterprise clients and end users online with Tandem while keeping hosted provisioning private.
---

Use this runbook when a forward engineer or agent needs to bring an enterprise
client online quickly. It is intentionally ordered: do not connect broad tools,
import data, or create automations until the earlier readiness and governance
checks pass.

Agent search terms: enterprise onboarding, client onboarding, end user
onboarding, enterprise MCP governance setup, tenant scoped setup, fast pilot,
go-live checklist, hosted client rollout, enterprise runbook.

## Hosted control-plane boundary

The public Tandem runtime starts after hosted provisioning has established the
customer tenant, workspace, and verified principal context. The private hosted
control plane owns:

- customer account and tenant provisioning
- workspace creation and ownership
- user invites and hosted end-user lifecycle
- SSO/OIDC, SCIM, billing, and account administration
- any hosted-only admin console flows

Do not document or invent public runtime endpoints for those features. In public
runtime docs, treat them as completed prerequisites. Agents may draft setup
plans, but they must not self-grant roles, create users, invite users, or imply
that the public runtime exposes invite or SCIM APIs.

## Track A: fast pilot

This track gets one client workspace live with one governed data source, one MCP
connector, one user-facing surface, and one safe automation.

### 1. Confirm runtime access

1. Start the engine and control panel.
2. Verify the operator has an engine token.
3. Check provider readiness before creating workflows.
4. Open the control panel enterprise admin route:

```text
http://127.0.0.1:39732/#/enterprise-admin
```

Stop if the operator cannot authenticate, provider/model readiness is unknown,
or the enterprise admin page does not show the expected tenant/principal
context.

Useful docs:

- [Control Panel](./control-panel/)
- [Engine Authentication For Agents](./engine-authentication-for-agents/)
- [Choosing Providers And Models For Agents](./choosing-providers-and-models-for-agents/)

### 2. Create the governance skeleton

Create only the minimum structure needed for the pilot:

1. One org unit, such as `pilot_team`, `finance`, or `support`.
2. One membership for the pilot user, group, service account, or agent worker.
3. One resource/data-class grant for the pilot data scope.

Example org unit:

```bash
curl -sS -X POST http://127.0.0.1:39731/enterprise/org-units \
  -H "content-type: application/json" \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN" \
  -d '{
    "unit_id": "pilot-finance",
    "taxonomy_id": "department",
    "display_name": "Pilot Finance"
  }'
```

Example grant:

```bash
curl -sS -X POST http://127.0.0.1:39731/enterprise/org-unit-access-grants \
  -H "content-type: application/json" \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN" \
  -d '{
    "grant_id": "pilot-finance-read",
    "unit_id": "pilot-finance",
    "permission": "read",
    "resource_ref": {
      "kind": "document_collection",
      "organization_id": "ORG_ID",
      "workspace_id": "WORKSPACE_ID",
      "resource_id": "finance-drive"
    },
    "data_class": "confidential"
  }'
```

Stop if the grant is broader than the pilot needs. Executive, all-company, and
admin-style access should be explicit and reviewed.

### 3. Register the governed data source

For the first pilot, prefer one read-only Google Drive binding or one equivalent
connector path that the enterprise admin page supports.

1. List supported enterprise connector providers.
2. Create an active connector record.
3. Attach a source-bound credential reference. Do not paste raw secrets.
4. Create a source binding with the right resource and data class.
5. Run preflight before import.
6. Import or reindex.
7. Review quarantine if the binding requires review.

Example provider discovery:

```bash
curl -sS http://127.0.0.1:39731/enterprise/connector-providers \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN"
```

Example connector and source binding:

```bash
curl -sS -X POST http://127.0.0.1:39731/enterprise/connectors \
  -H "content-type: application/json" \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN" \
  -d '{
    "connector_id": "google_drive",
    "provider": "google_drive",
    "display_name": "Pilot Google Drive",
    "state": "active"
  }'

curl -sS -X POST http://127.0.0.1:39731/enterprise/source-bindings \
  -H "content-type: application/json" \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN" \
  -d '{
    "binding_id": "finance-drive",
    "connector_id": "google_drive",
    "source_type": "google_drive",
    "native_source_id": "DRIVE_FOLDER_ID",
    "source_root_label": "Pilot Finance Folder",
    "resource_ref": {
      "kind": "document_collection",
      "organization_id": "ORG_ID",
      "workspace_id": "WORKSPACE_ID",
      "resource_id": "finance-drive"
    },
    "data_class": "confidential",
    "state": "enabled",
    "credential_ref_id": "readonly",
    "ingestion_policy": {
      "allow_indexing": true,
      "review_required": true
    }
  }'
```

Stop if preflight fails, the credential is not read-only for the pilot, or
quarantine output has not been reviewed.

### 4. Add the pilot MCP connector

Use MCP for external tools and system actions. Keep the tool surface narrow.

1. Call `mcp_list` first when operating as an agent.
2. If the connector is missing, ask the operator to connect it or file a
   capability request.
3. Add/connect the MCP server.
4. Verify discovered tool IDs.
5. Apply server-level and workflow-level allowlists.

Example:

```bash
curl -sS -X POST http://127.0.0.1:39731/mcp \
  -H "content-type: application/json" \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN" \
  -d '{
    "name": "pilot-slack",
    "transport": "https://your-mcp-server.example/mcp",
    "enabled": true,
    "allowed_tools": ["list_channels", "post_message"]
  }'

curl -sS -X POST http://127.0.0.1:39731/mcp/pilot-slack/connect \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN"

curl -sS http://127.0.0.1:39731/mcp/tools \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN"
```

Stop if the required tool is not listed in `/mcp/tools` or `/tool/ids`. A
catalog entry alone is not execution access.

### 5. Configure one end-user surface

Choose the smallest pilot surface:

- control panel for admin-led pilots
- Slack, Discord, or Telegram for channel pilots
- SDK/API caller for embedded customer workflows

For a knowledge bot, pair a KB-marked MCP server with channel-level strict KB
grounding so answers come from retrieved evidence instead of general model
memory.

Useful docs:

- [Channel Integrations](./channel-integrations/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [SDK](./sdk/)

### 6. Create the first safe automation

Use V2 automation for persistent pilot workflows. Separate high-impact action
stages:

1. read/research
2. draft artifact
3. human approval gate
4. post-approval external action

The post-approval node should be the only node with the concrete send, publish,
merge, delete, or update tool.

Minimal policy shape:

```json
{
  "tool_policy": {
    "allowlist": ["read", "write", "mcp.pilot_slack.post_message"],
    "denylist": []
  },
  "mcp_policy": {
    "allowed_servers": ["pilot-slack"],
    "allowed_tools": ["mcp.pilot_slack.post_message"]
  }
}
```

Stop if the automation gives send/publish tools to research or approval-gate
nodes.

### 7. Run the go-live smoke test

Before handing the pilot to the client, verify:

- provider/model readiness is green
- enterprise tenant/principal context is correct
- org unit membership and effective grants are correct
- source binding preflight passes
- import or reindex produces source-object and ingestion-job records
- quarantine is empty or reviewed
- MCP server is connected and the exact tools are listed
- first automation run creates the expected artifact
- approval gate works before external action
- audit, run, and event surfaces show the expected decisions
- connector impact report works for the pilot connector

If any smoke check fails, pause the pilot and fix the narrow failing layer before
expanding scope.

## Track B: enterprise hardening

Run this after the fast pilot is healthy.

- Expand org units and grants from pilot-only access to the customer's real
  taxonomy.
- Split data classes for public, internal, confidential, restricted, and any
  customer-specific classes.
- Add review-required source bindings for sensitive sources.
- Add more MCP connectors one at a time with explicit server-level tool
  allowlists.
- Add channel-specific KB grounding where end-user answers must cite approved
  knowledge.
- Add approval queues for external actions and lifecycle reviews.
- Document connector rotation, revoke, quarantine, re-scope, and cache
  invalidation playbooks.
- Verify audit and run-history evidence with the customer's admin/operator.
- Create a support handoff that names what is public runtime setup versus
  private hosted-control-plane setup.

## What agents should output

When an agent assists with enterprise onboarding, it should produce artifacts
that a forward engineer can review and apply. Good outputs include:

- a provisioning prerequisite checklist for private hosted-control-plane owners
- the proposed endpoint call order with sample payloads and unknown values marked
  as operator-supplied
- the org-unit, membership, resource, and data-class map for admin review
- the MCP inventory, missing capability requests, and exact allowed tool IDs
- source-binding, credential-reference, preflight, import, and quarantine notes
- proposed automation policies with research, draft, approval, and external
  action stages separated
- a smoke-test report that names the exact check that passed or failed
- client/support handoff notes that distinguish public runtime setup from
  private hosted-account setup

Agents should leave credential values, user invites, SSO/OIDC, SCIM, billing,
and hosted account ownership changes with the private control-plane owner.

## Operator handoff checklist

Before calling the onboarding complete, give the client or internal support
team:

- engine/control-panel URL and support owner
- private hosted-control-plane owner for invites, SSO/OIDC, SCIM, and billing
- provider/model policy summary
- connected MCP list and allowed tools
- source bindings and data classes
- channel surfaces and grounding settings
- first automation names, schedules, approval owners, and run history links
- incident response steps for connector compromise or data mis-scope
- known limitations and next hardening tasks

## Related docs

- [Enterprise Data Governance](./enterprise-data-governance/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [MCP Capability Discovery And Request Flow](./mcp-capability-discovery-and-request-flow/)
- [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)
- [Channel Integrations](./channel-integrations/)
- [Engine Authentication For Agents](./engine-authentication-for-agents/)
- [Control Panel](./control-panel/)
