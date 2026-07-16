import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../src/features/mission-builder/shared.ts", import.meta.url),
  "utf8",
);

test("mission builder permits compact three-stage approval workflows", () => {
  assert.match(source, /Use 1 to 7 scoped workstreams/);
  assert.match(source, /Honor an explicit requested stage or workstream count/);
  assert.match(source, /inspect and draft -> approval -> write/);
  assert.doesNotMatch(source, /Use 3 to 7 scoped workstreams/);
});
