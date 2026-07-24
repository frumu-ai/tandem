import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../src/features/automations/AutomationWebhookManager.tsx", import.meta.url),
  "utf8",
);

test("webhook delivery summaries expose rejection reasons without expansion", () => {
  assert.match(source, /Reason:\s*<code>\{reason\}<\/code>/);
});

test("webhook creation warns that org units require matching enterprise scope", () => {
  assert.match(source, /Enterprise org unit \(optional\)/);
  assert.match(source, /Enterprise-scoped workflows only\./);
  assert.match(source, /Otherwise, webhook deliveries\s+will be rejected\./);
});

test("Notion onboarding uses a one-time setup URL and protected reset path", () => {
  assert.match(source, /setup_callback_url/);
  assert.match(source, /Notion setup URL/);
  assert.match(source, /Signed event callback/);
  assert.match(source, /resetWebhookVerification/);
  assert.match(source, /Issue new setup URL/);
  assert.match(source, /invalidates the current signing token/);
});
