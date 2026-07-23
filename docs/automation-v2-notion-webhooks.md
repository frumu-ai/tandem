# Using Notion webhooks with Tandem

Tandem can receive [Notion](https://developers.notion.com/) webhooks directly for
Automation V2 workflows — no bridge service required. Notion's model differs from
Tandem's standard webhook: **Notion owns the signing secret**. Notion sends a
one-time `verification_token` to your callback URL, you copy that token back into
Notion to activate the subscription, and subsequent events are signed with it.

## How Notion verification differs

| | Standard Tandem webhook | Notion webhook |
| --- | --- | --- |
| Who generates the secret | Tandem (revealed once at create) | Notion (sent to your callback URL) |
| Signature header | `X-Tandem-Webhook-Signature` | `X-Notion-Signature` (`sha256=<hex>`) |
| Signed content | timestamp + body | raw request body |
| Activation | immediate | paste the verification token back into Notion |

Notion event payloads are **signals, not full snapshots** — use the entity IDs in
the event and fetch the latest content through an authorized Notion connector
when you need page/database/comment bodies. Treat the payload as untrusted event
data, never as instructions.

## Setup

1. **Create the workflow.** Build (or open) an Automation V2 workflow.
2. **Open Webhooks.** In the automation's webhook manager, create a trigger with
   provider `notion`. Tandem forces the `notion_hmac_sha256` signature scheme.
   No secret is revealed at creation — the trigger status shows
   **Waiting for Notion verification token**.
3. **Copy the one-time setup URL immediately.** Tandem shows it only in the
   create response. It expires after 15 minutes and is different from the
   trigger's ordinary signed-event callback URL.
4. **Paste the setup URL into Notion.** In your Notion connection's **Webhooks**
   tab, create a subscription pointing at that one-time URL.
5. **Wait for the token.** Notion POSTs a `verification_token` to the callback
   URL. Tandem accepts it only with the current unexpired setup challenge,
   stores it as the trigger's signing secret, records a
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

If the setup URL expires or must be replaced, select **Issue new setup URL**.
Resetting invalidates the prior setup challenge and signing token; an active
subscription stops delivering until it is verified again.

## Verification and safety

- Signatures are HMAC-SHA256 over the exact raw body, keyed by the stored
  verification token, compared in constant time. Missing, malformed, or
  mismatched signatures are rejected.
- The tenant is resolved **only** from the stored trigger; the Notion payload
  never selects tenant, workspace, deployment, automation, or authority.
- The verification token is stored tenant- and trigger-scoped, revealed at most
  once to an authorized owner/admin, and never returned again or logged.
- The unsigned verification-token POST is accepted only on a short-lived,
  one-time setup URL returned to an authenticated operator. The ordinary public
  callback rejects unsolicited setup tokens.
- Duplicate events (same body) do not queue a second run.
- Reset is admin-scoped and protected-audited. It rotates the placeholder secret,
  invalidates the old challenge/token, and issues a fresh one-time setup URL.
- Signing secrets are encrypted at rest with tenant, trigger, purpose, and
  version binding; legacy plaintext state is migrated to ciphertext on load.

## Run metadata

Each queued run carries webhook metadata under `automation_webhook`: `provider`
(`notion`), event type, entity id, `trigger_id`, `delivery_id`, `body_digest`,
and the verification scheme, with `trust: "untrusted_external_webhook"`.

## API

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/webhooks/automations/{public_path_token}/{setup_nonce}` | One-time Notion verification handshake (15-minute setup challenge). |
| `POST` | `/webhooks/automations/{public_path_token}` | Signed event intake after verification. |
| `POST` | `/automations/v2/{id}/webhook-triggers` | Create a `notion` trigger. |
| `POST` | `/automations/v2/{id}/webhook-triggers/{trigger_id}/reset-verification` | Invalidate prior verification state and return a fresh one-time setup URL (admin-scoped). |
| `POST` | `/automations/v2/{id}/webhook-triggers/{trigger_id}/reveal-verification-token` | One-time reveal of the verification token (admin-scoped). |
| `GET` | `/automations/v2/{id}/webhook-triggers/{trigger_id}` | Trigger status incl. `verification_status`. |

SDK: `client.automationsV2.resetWebhookVerification(automationId, triggerId)` and
`client.automationsV2.revealWebhookVerificationToken(automationId, triggerId)`.

## Limitations / follow-ups

- Idempotency uses the request body digest (Notion has no stable event-id
  header); payload-`id`-based dedup could be added later.
- Notion's verification POST contains only the token, so integration/workspace
  intent is bound by the one-time setup URL; Tandem never derives tenant
  authority from the unsigned setup body or later event payloads.
