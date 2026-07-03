---
title: Automation V2 Webhooks
description: Trigger Automation V2 workflows from external providers with signed, tenant-scoped webhooks, including native Notion verification-token support.
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

## Related

- [Incident Monitor Destination Router](./incident-monitor/destination-router/) — signed **outbound** webhook destinations.
- [Automation Composer Workflows](./automation-composer-workflows/)
