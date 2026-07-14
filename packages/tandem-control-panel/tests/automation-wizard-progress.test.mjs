import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const here = dirname(fileURLToPath(import.meta.url));
const sourceDir = join(here, "..", "src");

test("automation wizard shows content-free live planner progress", () => {
  const wizard = readFileSync(
    join(sourceDir, "features", "automations", "create", "CreateWizard.tsx"),
    "utf8"
  );
  const shell = readFileSync(join(sourceDir, "app", "AppShell.tsx"), "utf8");

  assert.match(wizard, /useEngineStream/);
  assert.match(wizard, /workflow_planner\.progress/);
  assert.match(wizard, /createPlannerProgressID/);
  assert.match(wizard, /progress_id: progressID/);
  assert.match(wizard, /workflow-plan-build:\$\{plannerProgressIDRef\.current\}/);
  assert.match(wizard, /responseChars/);
  assert.match(wizard, /elapsedSeconds/);
  assert.match(wizard, /The model is working on the plan/);
  assert.match(wizard, /Receiving the model response/);
  assert.match(shell, /navigationLock\.progress/);
  assert.match(shell, /response\s+characters received/);
  assert.match(shell, /elapsed/);
  assert.doesNotMatch(shell, /ReasoningDelta|chain.of.thought|properties\.delta/);
});
