import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const here = dirname(fileURLToPath(import.meta.url));
const createDir = join(here, "..", "src", "features", "automations", "create");

test("automation wizard reports planner fallbacks as blocking errors", () => {
  const wizard = readFileSync(join(createDir, "CreateWizard.tsx"), "utf8");
  const review = readFileSync(join(createDir, "Step4Review.tsx"), "utf8");

  assert.match(wizard, /describePlannerFallback/);
  assert.match(wizard, /toast\("err", fallbackError\)/);
  assert.match(wizard, /automationApplyIdempotencyKey/);
  assert.match(wizard, /\/api\/engine\/workflow-plans\/apply/);
  assert.match(wizard, /idempotency_key/);
  assert.match(wizard, /PROTECTED_AUDIT_PERSISTENCE_FAILED/);
  assert.match(wizard, /Do not click Create again while audit storage is unhealthy/);
  assert.match(wizard, /invalidateQueries\(\{ queryKey: \["automations"\] \}\)/);
  assert.match(review, /Plan validation failed — creation is blocked/);
  assert.match(review, /role="alert"/);
  assert.match(review, /No automation was created/);
  assert.match(review, /Creation blocked — fix plan error/);
  assert.match(review, /aria-describedby=.*automation-create-blocked/);
});
