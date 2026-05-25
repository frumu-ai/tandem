import test from "node:test";
import assert from "node:assert/strict";
import { closeDrawersOnEsc, openDriftDrawerIfNeeded } from "./blackboardUiState.js";

test("drift drawer opens only when drift exists", () => {
  assert.equal(openDriftDrawerIfNeeded(true), true);
  assert.equal(openDriftDrawerIfNeeded(false), false);
});

test("escape closes open drawers", () => {
  assert.equal(closeDrawersOnEsc(true), false);
  assert.equal(closeDrawersOnEsc(false), false);
});
