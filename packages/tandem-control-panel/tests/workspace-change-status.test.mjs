import assert from "node:assert/strict";
import test from "node:test";

import { classifyStatusOnlyWorkspaceChange } from "../lib/setup/workspace-change-status.js";

test("unchanged status-only workspace entries are not task output", () => {
  assert.equal(classifyStatusOnlyWorkspaceChange("D", "D"), null);
  assert.equal(classifyStatusOnlyWorkspaceChange("A", "A"), null);
  assert.equal(classifyStatusOnlyWorkspaceChange("", ""), null);
});

test("status-only workspace transitions retain meaningful classifications", () => {
  assert.equal(classifyStatusOnlyWorkspaceChange("", "D"), "deleted");
  assert.equal(classifyStatusOnlyWorkspaceChange("D", ""), "updated");
  assert.equal(classifyStatusOnlyWorkspaceChange("A", "M"), "updated");
});
