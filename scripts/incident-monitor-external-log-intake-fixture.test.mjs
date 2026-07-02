import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixturePath = path.join(
  repoRoot,
  "docs/fixtures/incident-monitor-external-log-intake/service.log.jsonl"
);
const smokeScriptPath = path.join(
  repoRoot,
  "scripts/incident-monitor-external-log-intake-smoke.mjs"
);

function readFixtureLines() {
  return fs
    .readFileSync(fixturePath, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

test("external log fixture contains a replayable JSONL error candidate", () => {
  const rows = readFixtureLines();
  assert.ok(rows.length >= 2, "fixture should include context and error rows");

  const error = rows.find((row) => row.level === "error");
  assert.ok(error, "fixture should include an error row");
  assert.equal(error.event, "external_service_crash");
  assert.equal(error.fingerprint, "external-demo-issue-sync-workflow-id");
  assert.match(error.stack, /workflow_id/);
});

test("external log smoke script dry-run is CI safe", () => {
  const raw = execFileSync(process.execPath, [smokeScriptPath, "--dry-run"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      TANDEM_BASE_URL: "http://127.0.0.1:39731/api/engine",
      TANDEM_TOKEN: "test-token",
    },
  });
  const payload = JSON.parse(raw);

  assert.equal(payload.ok, true);
  assert.equal(payload.dry_run, true);
  assert.equal(payload.project_id, "external-demo");
  assert.equal(payload.source_id, "service-jsonl");
  assert.equal(payload.fixture_path, fixturePath);
  assert.equal(payload.line.level, "error");
  assert.match(payload.line.fingerprint, /^external-demo-smoke-/);
});
