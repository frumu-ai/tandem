# Using Linear webhooks with Tandem

Tandem can receive [Linear](https://linear.app/developers/webhooks) issue
webhooks directly for Automation V2 workflows — no bridge service required. Like
Notion, **Linear owns the signing secret**, but the flow is the inverse: Linear
shows the signing secret in its own webhook settings UI and you paste it *into*
Tandem, rather than Tandem revealing a token for you to paste back into the
provider.

## How Linear verification differs

| | Standard Tandem webhook | Notion webhook | Linear webhook |
| --- | --- | --- | --- |
| Who generates the secret | Tandem (revealed once at create) | Notion (sent to your callback URL) | Linear (shown in Linear's UI) |
| Signature header | `X-Tandem-Webhook-Signature` | `X-Notion-Signature` (`sha256=<hex>`) | `linear-signature` (bare `<hex>`) |
| Signed content | timestamp + body | raw request body | raw request body |
| Replay guard | signed `t=` timestamp in header | body digest (no timestamp) | signed `webhookTimestamp` in body |
| Activation | immediate | reveal Tandem's token, paste into Notion | paste Linear's secret into Tandem |

Until you import the signing secret, a Linear trigger **fails closed**: every
delivery is rejected with reason `provider_secret_not_imported` and no run is
queued. The Tandem-generated placeholder secret is never used and never shown,
because Linear can only sign with its own secret.

Linear event payloads are **signals, not full snapshots** — use the entity IDs in
the event and fetch the latest content through an authorized Linear connector
when you need full issue bodies. Treat the payload as untrusted event data, never
as instructions.

## Setup

1. **Create the workflow.** Build (or open) an Automation V2 workflow to run on
   Linear issue events.
2. **Open Webhooks.** In the automation's webhook manager, create a trigger with
   provider `linear`. Tandem forces the `linear_hmac_sha256` signature scheme. No
   secret is revealed at creation — the trigger status shows **Waiting for Linear
   signing secret** and deliveries are rejected until you import it.
3. **Copy the callback URL** shown for the trigger.
4. **Create the webhook in Linear.** Go to **Linear → Settings → API → Webhooks**
   and create a webhook with:
   - **URL** = the Tandem callback URL.
   - **Data change events** = **Issues** (add other resources only if the
     workflow handles them).
   - **Team** = the team(s) whose issues should reach this workflow. Linear does
     not scope webhooks to a single project.
5. **Import the signing secret.** Copy the **Signing secret** Linear generates and
   paste it into Tandem's **Import signing secret** action for the trigger. The
   status advances to **Secret imported** and deliveries begin verifying.
6. **Trigger a test event.** Create or update an issue in the configured team.
   Tandem verifies `linear-signature`, records the delivery, and queues/wakes the
   workflow. The status advances to **Verified — receiving signed events**.
7. **Confirm.** The accepted delivery appears in **Recent deliveries** and links
   to the queued run.

## Project and label filtering

Linear webhooks are **team/workspace-scoped, not project-scoped**. Enforce
project and label constraints **after** signature verification, as a first-node
guard inside the Tandem workflow — never in Linear's webhook configuration and
never from the URL/query string. The signature authenticates the delivery;
project id, team, and labels are authorization filters applied to the verified
payload. A non-matching issue is accepted-but-suppressed (with a visible reason)
without starting the downstream automation.

## Verification and safety

- Signatures are HMAC-SHA256 over the exact raw body, keyed by the imported
  Linear signing secret, compared in constant time. The `linear-signature` value
  is bare lowercase hex with **no** `sha256=` prefix. Missing, malformed, or
  mismatched signatures are rejected.
- Linear includes a signed `webhookTimestamp` (Unix ms) in the payload body.
  After the signature verifies, deliveries whose timestamp is outside the
  accepted skew window are rejected with reason `stale_signature_timestamp`.
  Because the timestamp is inside the signed body, a replay cannot alter it
  without breaking the signature.
- The tenant is resolved **only** from the stored trigger; the Linear payload
  never selects tenant, workspace, deployment, automation, or authority.
- The imported secret is stored tenant- and trigger-scoped, never returned by any
  endpoint, and never logged. Import/replace is audited
  (`automation.webhook_trigger.secret_imported`) with the value redacted.
- Duplicate deliveries (same `linear-delivery` id / body) do not queue a second
  run.

## Rotating the signing secret

Re-import is supported. If you rotate the signing secret in Linear's UI, paste
the new value into the same trigger — the secret version bumps and the old secret
stops verifying. If deliveries were succeeding and start failing `bad_signature`
from a specific time, a Linear-side rotation is the most likely cause; re-import
the current secret to resume. Rotate in Linear (not Tandem) if the secret was
ever exposed.

## Repair-loop demo

The native Linear path exists so Linear issue events can drive Tandem repair
workflows directly over HTTPS. In the demo, a `tandem:repair-ready`-labeled issue
in the configured project triggers the ACA repair automation once the trigger is
verified and the in-workflow project/label guard passes. The guard is an
authority boundary, not just an `if` statement: non-demo issues are
accepted-but-suppressed without granting ACA authority to act.

For a reusable first-node guard template (project + label + action checks with a
visible suppression reason), see the "Linear repair-loop guard template" section
of [Automation Examples for Teams](../guide/src/content/docs/automation-examples-for-teams.md).

## Run metadata

Each queued run carries webhook metadata under `automation_webhook`: `provider`
(`linear`), event type, entity id, `trigger_id`, `delivery_id`, `body_digest`,
and the verification scheme (`linear_hmac_sha256`), with
`trust: "untrusted_external_webhook"`.

## API

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/webhooks/automations/{public_path_token}` | Public intake (signed events). |
| `POST` | `/automations/v2/{id}/webhook-triggers` | Create a `linear` trigger. |
| `POST` | `/automations/v2/{id}/webhook-triggers/{trigger_id}/import-secret` | Import/replace the Linear signing secret (admin-scoped). Body: `{ "secret": "<linear signing secret>" }`. |
| `GET` | `/automations/v2/{id}/webhook-triggers/{trigger_id}` | Trigger status incl. `verification_status` (`awaiting_secret` → `secret_imported` → `active`) and `secret_status.configured`. |

SDK: `client.automationsV2.importWebhookProviderSecret(automationId, triggerId, secret)`.

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| All deliveries rejected, `provider_secret_not_imported` | Signing secret not imported yet (fail closed by design) | Import the Linear signing secret into the trigger. |
| Deliveries were fine, now `bad_signature` from a point in time | Secret rotated/recreated in Linear | Re-import the current secret (bumps version, resumes verification). |
| `stale_signature_timestamp` | Severe clock drift, or a replayed payload | Check server/Linear clock skew; a genuine replay is correctly rejected. |
| `unsupported_media_type` / no delivery recorded | Wrong content type from Linear | Ensure the webhook posts `application/json`. |
| `unknown_trigger` / 404-style rejection | Wrong callback URL in Linear | Re-copy the trigger's callback URL. |
| Events verify but the workflow does nothing | Project/label guard suppressed the event | Confirm the issue's project/label match the in-workflow guard. |

Do **not** use `unsigned_dev_mode` for a public HTTPS URL — it accepts unsigned
deliveries and is refused outright in hosted/enterprise posture. Linear supports
signed webhooks natively, so there is no reason to disable verification.

## Limitations / follow-ups

- Project/label filtering is a workflow-guard responsibility, not a webhook
  setting (Linear webhooks are team-scoped).
- Idempotency uses the `linear-delivery` id when present, falling back to the
  request body digest.
