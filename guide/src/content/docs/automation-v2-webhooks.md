---
title: Automation V2 Webhooks
description: Trigger Automation V2 workflows from external providers with signed, tenant-scoped webhooks, including native Notion and Linear setup guidance.
---

Automation V2 workflows can be triggered by external webhooks. For publicly
authorable workflows, an accepted webhook starts a new Automation V2 run at the
eligible root nodes. A workflow can also declare a typed correlated webhook
wait; a matching delivery wakes that wait and resumes the same run. Tandem verifies every delivery,
resolves the tenant from the stored trigger (never from the payload), and records
the delivery durably. Payloads are treated as untrusted external event data,
never as instructions.

For the complete state, routing, approval, inspection, and recovery model, see
[Building Stateful Workflows in Tandem](../stateful-workflows/).

## Signature schemes

A webhook trigger declares a signature scheme that decides how deliveries are
authenticated:

- `hmac_sha256_v1` — Tandem generates the signing secret and reveals it once at
  creation. Deliveries carry `X-Tandem-Webhook-Signature: t=<timestamp_ms>,v1=<hmac_sha256>`,
  an HMAC over the timestamp and raw body. (`tandem_hmac_sha256_v1` is the
  internal verifier/header identifier, not the value you set on the trigger.)
- `github_hmac_sha256` — Tandem generates a secret that you configure on the
  GitHub webhook. Tandem verifies `X-Hub-Signature-256: sha256=<hex>`, an
  HMAC-SHA256 over the exact raw request body.
- `notion_hmac_sha256` — the provider (Notion) owns the signing secret. No secret
  is revealed at creation; the token arrives out of band and Tandem stores it.
  Tandem verifies `X-Notion-Signature: sha256=<hex>` over the exact raw request
  body. See [Notion webhooks](#notion-webhooks) below.
- `linear_hmac_sha256` — the provider (Linear) owns the signing secret. Import
  the Linear-generated secret into the Tandem trigger, then Tandem verifies the
  bare-hex `linear-signature` header over the exact raw request body. See
  [Linear issue webhooks](#linear-issue-webhooks) below.
- `shared_secret_header_v1` — Tandem compares the value of
  `X-Tandem-Webhook-Secret` with the trigger secret. This authenticates the
  request with a shared token; it does not HMAC the request body.
- `unsigned_dev_mode` — accepts a request without a signature only when the
  server has explicitly enabled unsigned development webhooks. **Use this only
  for local development. Never expose an unsigned trigger on a hosted,
  production, or otherwise internet-facing callback URL.**

For every authenticated scheme, signatures or shared tokens are compared in
constant time, and missing, malformed, or mismatched credentials are rejected.
Body-signing providers must deliver the exact bytes they signed. The tenant,
workspace, deployment, automation, and authority are resolved **only** from the
stored trigger.

## Durable delivery inbox

Webhook intake is decoupled from workflow execution. An accepted delivery is
written to a durable inbox first, then drained to start a new run or wake a
matching wait that the runtime has already registered from a public Automation
V2 `wait` node. This keeps intake fast and resilient: a delivery is not lost if
the runtime is briefly busy, and duplicate deliveries (same body) do not queue a
second run.

Webhook waits do not jump to an arbitrary node. The wait node must already be
eligible in the published DAG, and its `correlation` must match the accepted
delivery by provider event ID, idempotency key, or body digest. See
[Author durable wait nodes](../stateful-workflows/#author-durable-wait-nodes)
for the complete schema and timeout rules.

## Notion webhooks

Tandem can receive [Notion](https://developers.notion.com/) webhooks directly for
Automation V2 workflows — no bridge service required. Notion's model differs from
Tandem's standard webhook: **Notion owns the signing secret**. Notion sends a
one-time `verification_token` to your callback URL, you copy that token back into
Notion to activate the subscription, and subsequent events are signed with it.

### How Notion verification differs

|                          | Standard Tandem webhook          | Notion webhook                                |
| ------------------------ | -------------------------------- | --------------------------------------------- |
| Who generates the secret | Tandem (revealed once at create) | Notion (sent to your callback URL)            |
| Signature header         | `X-Tandem-Webhook-Signature`     | `X-Notion-Signature`                          |
| Signed content           | timestamp + body                 | raw request body                              |
| Activation               | immediate                        | paste the verification token back into Notion |

Notion event payloads are **signals, not full snapshots** — use the entity IDs in
the event and fetch the latest content through an authorized Notion connector
when you need page/database/comment bodies.

### Setup

1. **Create the workflow.** Build (or open) an Automation V2 workflow.
2. **Open Webhooks.** In the automation's webhook manager, create a trigger with
   provider `notion`. Tandem forces the `notion_hmac_sha256` signature scheme.
   No secret is revealed at creation — the trigger status shows
   **Waiting for Notion verification token**.
3. **Copy the callback URL** shown for the trigger.
4. **Paste it into Notion.** In your Notion connection's **Webhooks** tab, create
   a subscription pointing at the callback URL.
5. **Wait for the token.** Notion POSTs a `verification_token` to the callback
   URL. Tandem stores it (as the trigger's signing secret), records a
   `notion_verification_token_received` delivery, and the status advances to
   **Verification token received**. This request does **not** start a workflow run.
6. **Reveal and paste the token back.** In Tandem, click **Reveal verification
   token** (available exactly once) and paste it into Notion to verify the
   subscription. Tandem never shows the token again.
7. **Trigger an event.** Once Notion sends a signed event, Tandem verifies
   `X-Notion-Signature` and records the delivery. It starts a new Automation V2
   run when no registered wait matches, or wakes the matching typed webhook
   wait and resumes that existing run. The status advances
   to **Verified — receiving signed events**.
8. **Confirm.** The accepted delivery appears in **Recent deliveries** and links
   to either the queued run or the woken run and wait.

### Verification and safety

- Signatures are HMAC-SHA256 over the exact raw body, keyed by the stored
  verification token, compared in constant time. Missing, malformed, or
  mismatched signatures are rejected.
- The tenant is resolved **only** from the stored trigger; the Notion payload
  never selects tenant, workspace, deployment, automation, or authority.
- The verification token is stored tenant- and trigger-scoped, revealed at most
  once to an authorized owner/admin, and never returned again or logged.
- Duplicate events (same body) do not queue a second run.
- The verification token is only captured while the trigger is awaiting one; an
  unsigned request cannot overwrite a token that has already been received.

## Linear issue webhooks

Tandem can receive Linear issue events directly for Automation V2 workflows when
the trigger uses provider `linear` and the native `linear_hmac_sha256` signature
scheme. Use this for repair-loop automations where a Linear issue should trigger
an ACA workflow without a bridge service.

Linear webhooks are team- or workspace-scoped. Treat the signed Linear payload
as trusted for origin only. Before ACA receives authority to inspect or modify a
repository, use a fixed read-only guard lookup to check the current project,
label, and issue state. The webhook event action remains operator-inspectable
metadata; it is not currently a node input.

### Setup

1. **Create or select the workflow.** Build an Automation V2 workflow that starts
   with a Linear guard node before any repo, MCP, or write-capable ACA step.
2. **Create the Tandem trigger.** In the automation's webhook manager, create a
   trigger with provider `linear`, event kind such as `issues.updated`, and
   signature scheme `linear_hmac_sha256`.
3. **Copy the callback URL** shown for the trigger.
4. **Create the Linear webhook.** In Linear, open **Settings -> API -> Webhooks**,
   create a webhook, paste the Tandem callback URL, and select **Issues** data
   change events. If Linear asks for teams, select the team that owns the repair
   project.
5. **Import the Linear signing secret.** Copy the signing secret generated by
   Linear and paste/import it into the Tandem trigger. Tandem stores it as
   tenant- and trigger-scoped secret material; the secret should not be committed
   to workflow JSON, docs, screenshots, or demo notes.
6. **Verify a test delivery.** Create or update a test issue in the intended
   project. The delivery should show provider `linear`,
   scheme `linear_hmac_sha256`, a Linear delivery/event id when available, body
   digest, verification reason, delivery outcome, and queued run id when a run
   was created. Follow that run id to inspect the guard output.
7. **Rotate exposed secrets.** If the Linear signing secret was pasted into chat,
   logs, screenshots, or a public demo environment, rotate or recreate the Linear
   webhook secret and re-import the new value into Tandem.

Do not use `unsigned_dev_mode` for any public Linear callback URL. That mode is
only for explicitly enabled local/dev servers and should fail closed on hosted or
internet-facing deployments.

### Repair-loop guard

Use the first workflow node as an authority boundary. The guard should accept
only the configured Linear project, an explicit repair-ready label such as
`tandem:repair-ready`, and an eligible current issue state. A valid delivery can
start the run, but the guard returns `has_work: false` when its authoritative
lookup finds no eligible issue, suppressing the downstream ACA nodes.

For a reusable template, see [Automation Examples for Teams](../automation-examples-for-teams/#4-linear-repair-loop-guard-template).

### Troubleshooting

| Symptom                                         | Likely cause                                                                                                                                                   | Fix                                                                                                                                  |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `missing_signature`                             | Linear did not include `linear-signature`, or the request hit the wrong URL.                                                                                   | Recopy the Tandem callback URL into Linear and verify the webhook uses Linear's normal JSON delivery path.                           |
| `malformed_signature`                           | The signature header is not the expected Linear HMAC value.                                                                                                    | Recreate the Linear webhook or remove any proxy/header rewrite between Linear and Tandem.                                            |
| `bad_signature`                                 | Wrong imported secret, mutated body, stale secret after rotation, or a proxy changed bytes before Tandem verified them.                                        | Re-import the current Linear signing secret and make sure the raw body reaches Tandem unchanged.                                     |
| `missing_secret_material`                       | The trigger uses `linear_hmac_sha256` but no Linear secret has been imported.                                                                                  | Import the Linear signing secret into the Tandem trigger; the trigger should fail closed until this is done.                         |
| `stale_signature_timestamp`                     | Linear's `webhookTimestamp` is outside the accepted clock-skew window.                                                                                         | Check server clock drift and avoid replaying old payloads.                                                                           |
| Delivery accepted but no ACA work runs          | The webhook run started, but a fixed read-only guard lookup found no eligible issue, or the delivery was deduplicated/suppressed before a new run was created. | Inspect delivery/run metadata and the guard output, then update the authoritative Linear project/label state or intentionally rerun. |
| Public test only works with `unsigned_dev_mode` | The trigger is bypassing production signature verification.                                                                                                    | Switch to `linear_hmac_sha256`, import the Linear secret, and rotate any secret exposed during testing.                              |

## Run metadata

Each queued run stores webhook metadata under
`automation_snapshot.metadata.automation_webhook`: `provider`, event type,
entity id, `trigger_id`, `delivery_id`, `body_digest`, and the verification
scheme, with `trust: "untrusted_external_webhook"`. This is an operator
inspection surface. Current public Automation V2 authoring does not inject that
object into a root node prompt or expose it as an `input_refs` source; use a
fixed read-only provider lookup for node-visible guard decisions.

## API

| Method | Path                                                                           | Purpose                                                        |
| ------ | ------------------------------------------------------------------------------ | -------------------------------------------------------------- |
| `POST` | `/webhooks/automations/{public_path_token}`                                    | Public intake (verification handshake + signed events).        |
| `POST` | `/automations/v2/{id}/webhook-triggers`                                        | Create a webhook trigger (e.g. a `notion` trigger).            |
| `POST` | `/automations/v2/{id}/webhook-triggers/{trigger_id}/reveal-verification-token` | One-time reveal of a Notion verification token (admin-scoped). |
| `POST` | `/automations/v2/{id}/webhook-triggers/{trigger_id}/import-secret`             | Import/replace a Linear signing secret (admin-scoped).         |
| `GET`  | `/automations/v2/{id}/webhook-triggers/{trigger_id}`                           | Trigger status incl. `verification_status`.                    |

SDK:

- Notion: `client.automationsV2.revealWebhookVerificationToken(automationId, triggerId)`.
- Linear: `client.automationsV2.importWebhookProviderSecret(automationId, triggerId, secret)`.

For the full Linear dev reference (troubleshooting, run metadata, secret rotation), see [Using Linear Webhooks with Tandem](https://github.com/frumu-ai/tandem/blob/main/docs/automation-v2-linear-webhooks.md).

## Limitations / follow-ups

- Notion idempotency uses the request body digest (Notion has no stable event-id
  header); payload-`id`-based dedup could be added later.
- The Notion verification token is captured only while the trigger is awaiting
  one; to re-capture (e.g. after re-subscribing in Notion) recreate the trigger.
- Linear project and label scoping is intentionally handled by workflow guards,
  not by trusting provider scope alone.

## Related

- [Building Stateful Workflows in Tandem](../stateful-workflows/) — state,
  webhook routing, approvals, inspection, and recovery from end to end.
- [Automation Examples for Teams](./automation-examples-for-teams/) — includes a Linear repair-loop guard template.
- [Incident Monitor Destination Router](./incident-monitor/destination-router/) — signed **outbound** webhook destinations.
- [Automation Composer Workflows](./automation-composer-workflows/)
