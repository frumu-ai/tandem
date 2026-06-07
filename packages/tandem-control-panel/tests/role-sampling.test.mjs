import assert from "node:assert/strict";
import test from "node:test";

import {
  SWARM_ROLE_KEYS,
  applyRoleSampling,
  coerceSamplingValue,
  readRoleSampling,
} from "../src/pages/roleSampling.js";

const baseConfig = () =>
  JSON.stringify(
    {
      version: 1,
      swarm: {
        enabled: true,
        manager: { provider: "openai", model: "gpt-4.1-mini" },
        worker: { provider: "", model: "" },
        reviewer: { provider: "", model: "" },
        tester: { provider: "", model: "" },
      },
    },
    null,
    2,
  );

test("exposes the four swarm roles", () => {
  assert.deepEqual(SWARM_ROLE_KEYS, ["manager", "worker", "reviewer", "tester"]);
});

test("readRoleSampling returns blank strings when unset", () => {
  const { ok, values } = readRoleSampling(baseConfig());
  assert.equal(ok, true);
  for (const role of SWARM_ROLE_KEYS) {
    assert.deepEqual(values[role], { temperature: "", top_p: "", max_tokens: "" });
  }
});

test("applyRoleSampling sets a numeric value", () => {
  const result = applyRoleSampling(baseConfig(), "reviewer", "temperature", "0.1");
  assert.equal(result.ok, true);
  const parsed = JSON.parse(result.text);
  assert.equal(parsed.swarm.reviewer.temperature, 0.1);
  // Existing provider/model are preserved.
  assert.equal(parsed.swarm.manager.provider, "openai");
});

test("blank value removes the key (unset, never forced)", () => {
  const withTemp = applyRoleSampling(baseConfig(), "worker", "temperature", "0.5").text;
  assert.equal(JSON.parse(withTemp).swarm.worker.temperature, 0.5);
  const cleared = applyRoleSampling(withTemp, "worker", "temperature", "");
  assert.equal(cleared.ok, true);
  assert.equal("temperature" in JSON.parse(cleared.text).swarm.worker, false);
});

test("max_tokens requires a whole number >= 1", () => {
  assert.equal(applyRoleSampling(baseConfig(), "tester", "max_tokens", "0").ok, false);
  assert.equal(applyRoleSampling(baseConfig(), "tester", "max_tokens", "1.5").ok, false);
  const ok = applyRoleSampling(baseConfig(), "tester", "max_tokens", "2048");
  assert.equal(ok.ok, true);
  assert.equal(JSON.parse(ok.text).swarm.tester.max_tokens, 2048);
});

test("rejects non-numeric input", () => {
  assert.equal(coerceSamplingValue("temperature", "warm").ok, false);
  assert.equal(coerceSamplingValue("temperature", "").value, null);
  assert.equal(coerceSamplingValue("temperature", "0.2").value, 0.2);
});

test("invalid JSON is reported, not mutated", () => {
  const result = applyRoleSampling("{not json", "manager", "temperature", "0.1");
  assert.equal(result.ok, false);
  assert.match(result.error, /invalid/i);
});

test("omitting sampling leaves config untouched", () => {
  const before = baseConfig();
  const { values } = readRoleSampling(before);
  // No edits performed → no sampling keys exist anywhere.
  const parsed = JSON.parse(before);
  for (const role of SWARM_ROLE_KEYS) {
    assert.equal("temperature" in parsed.swarm[role], false);
    assert.equal(values[role].temperature, "");
  }
});
