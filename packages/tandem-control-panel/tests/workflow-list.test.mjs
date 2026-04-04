import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_WORKFLOW_SORT_MODE,
  formatAutomationCreatedAtLabel,
  normalizeFavoriteAutomationIds,
  normalizeWorkflowSortMode,
  sortWorkflowAutomations,
  toggleFavoriteAutomationId,
} from "../lib/automations/workflow-list.js";

test("workflow list helpers pin favorites first and sort by created date", () => {
  const rows = [
    { automation_id: "c", name: "Charlie", created_at_ms: 1000 },
    { automation_id: "a", name: "Alpha", created_at_ms: 3000 },
    { automation_id: "b", name: "Bravo", created_at_ms: 2000 },
  ];

  const sorted = sortWorkflowAutomations(rows, {
    sortMode: "created_desc",
    favoriteAutomationIds: ["b"],
  });

  assert.deepEqual(
    sorted.map((row) => row.automation_id),
    ["b", "a", "c"]
  );
});

test("workflow list helpers normalize favorites and sort mode", () => {
  assert.equal(normalizeWorkflowSortMode("unknown"), DEFAULT_WORKFLOW_SORT_MODE);
  assert.deepEqual(normalizeFavoriteAutomationIds(["x", "x", " y ", "", null]), ["x", "y"]);
  assert.deepEqual(toggleFavoriteAutomationId(["x", "y"], "y"), ["x"]);
  assert.deepEqual(toggleFavoriteAutomationId(["x"], "y"), ["x", "y"]);
});

test("workflow list helpers format created labels with date and time", () => {
  const originalDateFormat = Intl.DateTimeFormat;
  const calls = [];
  Intl.DateTimeFormat = function (_locale, options) {
    calls.push(options);
    return {
      format() {
        return options?.minute === "2-digit" ? "12:34 PM" : "Apr 4, 2026";
      },
    };
  };

  try {
    assert.equal(
      formatAutomationCreatedAtLabel({ created_at_ms: 1_234_567_890_000 }),
      "Apr 4, 2026 · 12:34 PM"
    );
    assert.deepEqual(calls, [
      { month: "short", day: "numeric", year: "numeric" },
      { hour: "numeric", minute: "2-digit" },
    ]);
  } finally {
    Intl.DateTimeFormat = originalDateFormat;
  }
});
