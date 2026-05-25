import test from "node:test";
import assert from "node:assert/strict";
import {
  applyEsc,
  pauseFollowOnManualNavigation,
  reconcileSelection,
  shouldAutoFocusOnDecision,
  toggleExpand,
  toggleFullscreen,
  type BlackboardPanelMode,
} from "./blackboardPanelState.js";

test("toggleExpand switches between docked and expanded", () => {
  assert.equal(toggleExpand("docked"), "expanded");
  assert.equal(toggleExpand("expanded"), "docked");
});

test("toggleFullscreen switches expanded/docked into fullscreen and back to expanded", () => {
  assert.equal(toggleFullscreen("docked"), "fullscreen");
  assert.equal(toggleFullscreen("expanded"), "fullscreen");
  assert.equal(toggleFullscreen("fullscreen"), "expanded");
});

test("applyEsc only exits fullscreen", () => {
  const modes: BlackboardPanelMode[] = ["docked", "expanded", "fullscreen"];
  const next = modes.map((mode) => applyEsc(mode));
  assert.deepEqual(next, ["docked", "expanded", "expanded"]);
});

test("reconcileSelection preserves valid selection and falls back deterministically", () => {
  const rows = [{ id: "a" }, { id: "b" }];
  assert.equal(reconcileSelection(null, rows), "a");
  assert.equal(reconcileSelection("b", rows), "b");
  assert.equal(reconcileSelection("missing", rows), "a");
  assert.equal(reconcileSelection("a", []), null);
});

test("manual navigation always pauses follow mode", () => {
  assert.equal(pauseFollowOnManualNavigation(true), false);
  assert.equal(pauseFollowOnManualNavigation(false), false);
});

test("decision auto-focus only runs for newer decisions while follow is enabled", () => {
  assert.equal(shouldAutoFocusOnDecision(false, 10, 8), false);
  assert.equal(shouldAutoFocusOnDecision(true, null, 8), false);
  assert.equal(shouldAutoFocusOnDecision(true, 8, 8), false);
  assert.equal(shouldAutoFocusOnDecision(true, 9, 8), true);
});
