---
title: Automation V2 Webhooks
description: Trigger Automation V2 workflows from external providers with signed, tenant-scoped webhooks, including native Notion and Linear setup guidance.
---

Automation V2 workflows can be triggered by external webhooks. Tandem verifies
every delivery, resolves the tenant from the stored trigger (never from the
payload), records a durable delivery, and queues or wakes the workflow. Payloads
are treated as untrusted external event data, never as instructions.

## Signature schemes

A webhook trigger declares a signature scheme that decides how deliveries are
authenticated:

- `hmac_sha256_v1` — Tandem generates the signing secret and reveals it once at
  creation. Deliveries carry `X-Tandem-Webhook-Signature: t=<timestamp_ms>,v1=<hmac_sha256>`,
  an HMAC over the timestamp and raw body. (`tandem_hmac_sha256_v1` is the
  internal verifier/header identifier, not the value you set on the trigger.)
- `notion_hmac_sha256` — the provider (Notion) owns the signing secret. No secret
  is revealed at creation; the token arrives out of band and Tandem stores it.
  See [Notion webhooks](#notion-webhooks) below.
- `linear_hmac_sha256` — the provider (Linear) owns the signing secret. Import
  the Linear-generated secret into the Tandem trigger, then Tandem verifies the
  `linear-signature` header over the exact raw request body. See
  [Linear issue webhooks](#linear-issue-webhooks) below.

For every scheme, signatures are compared in constant time and missing,
malformed, or mismatched signatures are rejected. The tenant, workspace,
deployment, automation, and authority are resolved **only** from the stored
trigger.

## Durable delivery inbox

Webhook intake is decoupled from workflow execution. An accepted delivery is
written to a durable inbox first, then drained to queue or wake the target
workflow. This keeps intake fast and resilient: a delivery is not lost if the
runtime is briefly busy, and duplicate deliveries (same body) do not queue a
second run.

## Notion webhooks

Tandem can receive [Notion](https://developers.notion.com/) webhooks directly for
Automation V2 workflows — no bridge service required. Notion's model differs from
Tandem's standard webhook: **Notion owns the signing secret**. Notion sends a
one-time `verification_token` to your callback URL, you copy that token back into
Notion to activate the subscription, and subsequent events are signed with it.

### How Notion verification differs

| | Standard Tandem webhook | Notion webhook |
| --- | --- | --- |
| Who generates the secret | Tandem (revealed once at create) | Notion (sent to your callback URL) |
| Signature header | `X-Tandem-Webhook-Signature` | `X-Notion-Signature` |
| Signed content | timestamp + body | raw request body |
| Activation | immediate | paste the verification token back into Notion |

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
   `X-Notion-Signature`, records the delivery, and queues/wakes the workflow. The
   status advances to **Verified — receiving signed events**.
8. **Confirm.** The accepted delivery appears in **Recent deliveries** and links
   to the queued run.

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

Linear webhooks are team- or workspace-scoped. Treat the signed Linear payload as
trusted for origin only; project, label, and action checks still belong inside
the Tandem workflow guard before ACA receives authority to inspect or modify a
repository.

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
   digest, verification reason, and either a queued run id or a guard suppression
   reason.
7. **Rotate exposed secrets.** If the Linear signing secret was pasted into chat,
   logs, screenshots, or a public demo environment, rotate or recreate the Linear
   webhook secret and re-import the new value into Tandem.

Do not use `unsigned_dev_mode` for any public Linear callback URL. That mode is
only for explicitly enabled local/dev servers and should fail closed on hosted or
internet-facing deployments.

### Repair-loop guard

Use the first workflow node as an authority boundary. The guard should accept
only the configured Linear project, an explicit repair-ready label such as
`tandem:repair-ready`, and the allowed issue actions/states for the demo. All
other signed events should be suppressed with a visible reason instead of
starting ACA.

For a reusable template, see [Automation Examples for Teams](./automation-examples-for-teams/#linear-repair-loop-guard-template).

### Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `missing_signature` | Linear did not include `linear-signature`, or the request hit the wrong URL. | Recopy the Tandem callback URL into Linear and verify the webhook uses Linear's normal JSON delivery path. |
| `malformed_signature` | The signature header is not the expected Linear HMAC value. | Recreate the Linear webhook or remove any proxy/header rewrite between Linear and Tandem. |
| `bad_signature` | Wrong imported secret, mutated body, stale secret after rotation, or a proxy changed bytes before Tandem verified them. | Re-import the current Linear signing secret and make sure the raw body reaches Tandem unchanged. |
| `missing_secret_material` | The trigger uses `linear_hmac_sha256` but no Linear secret has been imported. | Import the Linear signing secret into the Tandem trigger; the trigger should fail closed until this is done. |
| `stale_signature_timestamp` | Linear's `webhookTimestamp` is outside the accepted clock-skew window. | Check server clock drift and avoid replaying old payloads. |
| Delivery accepted but no ACA run starts | The first workflow guard suppressed the event because project, label, action, or duplicate checks did not pass. | Inspect delivery/run metadata for the guard reason, then update the issue project/label or intentionally rerun. |
| Public test only works with `unsigned_dev_mode` | The trigger is bypassing production signature verification. | Switch to `linear_hmac_sha256`, import the Linear secret, and rotate any secret exposed during testing. |

## Run metadata

Each queued run carries webhook metadata under `automation_webhook`: `provider`,
event type, entity id, `trigger_id`, `delivery_id`, `body_digest`, and the
verification scheme, with `trust: "untrusted_external_webhook"`.

## API

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/webhooks/automations/{public_path_token}` | Public intake (verification handshake + signed events). |
| `POST` | `/automations/v2/{id}/webhook-triggers` | Create a webhook trigger (e.g. a `notion` trigger). |
| `POST` | `/automations/v2/{id}/webhook-triggers/{trigger_id}/reveal-verification-token` | One-time reveal of a provider verification token (admin-scoped). |
| `GET` | `/automations/v2/{id}/webhook-triggers/{trigger_id}` | Trigger status incl. `verification_status`. |

SDK: `client.automationsV2.revealWebhookVerificationToken(automationId, triggerId)`.

## Limitations / follow-ups

- Notion idempotency uses the request body digest (Notion has no stable event-id
  header); payload-`id`-based dedup could be added later.
- The Notion verification token is captured only while the trigger is awaiting
  one; to re-capture (e.g. after re-subscribing in Notion) recreate the trigger.
- Linear project and label scoping is intentionally handled by workflow guards,
  not by trusting provider scope alone.

## Related

- [Automation Examples for Teams](./automation-examples-for-teams/) — includes a Linear repair-loop guard template.
- [Incident Monitor Destination Router](./incident-monitor/destination-router/) — signed **outbound** webhook destinations.
- [Automation Composer Workflows](./automation-composer-workflows/)
