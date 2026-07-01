import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_WORKFLOW_LIBRARY_FILTERS,
  DEFAULT_WORKFLOW_SORT_MODE,
  classifyAutomationSource,
  filterWorkflowAutomations,
  formatAutomationCreatedAtLabel,
  normalizeFavoriteAutomationIds,
  normalizeWorkflowLibraryFilters,
  normalizeWorkflowSortMode,
  sortWorkflowAutomations,
  toggleFavoriteAutomationId,
  workflowLibraryFiltersEqual,
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

test("workflow list helpers classify and filter library sources", () => {
  const rows = [
    { automation_id: "user", name: "Daily notes", status: "active", creator_id: "desktop" },
    {
      automation_id: "bug",
      name: "Incident Monitor triage: failure",
      status: "active",
      creator_id: "incident_monitor",
      metadata: { source: "incident_monitor" },
    },
    {
      automation_id: "agent",
      name: "Generated workflow",
      status: "paused",
      creator_id: "workflow_planner",
    },
    {
      automation_id: "system",
      name: "System helper",
      status: "active",
      metadata: { source: "system" },
    },
  ];

  assert.equal(classifyAutomationSource(rows[1]).key, "incident_monitor");
  assert.deepEqual(
    filterWorkflowAutomations(rows, DEFAULT_WORKFLOW_LIBRARY_FILTERS).map(
      (row) => row.automation_id
    ),
    ["user", "agent"]
  );
  assert.deepEqual(
    filterWorkflowAutomations(rows, {
      sources: { user_created: false, agent_created: false, incident_monitor: true, system: false },
      statuses: { active: true, paused: true, draft: true },
    }).map((row) => row.automation_id),
    ["bug"]
  );
});

test("workflow list helpers normalize library filters", () => {
  const filters = normalizeWorkflowLibraryFilters({
    sources: { incident_monitor: true, system: true, unknown: true },
    statuses: { paused: false },
  });

  assert.equal(filters.sources.user_created, true);
  assert.equal(filters.sources.agent_created, true);
  assert.equal(filters.sources.incident_monitor, true);
  assert.equal(filters.sources.system, true);
  assert.equal(filters.statuses.active, true);
  assert.equal(filters.statuses.paused, false);
  assert.equal(filters.statuses.draft, true);
  assert.equal(workflowLibraryFiltersEqual(filters, filters), true);
  assert.equal(workflowLibraryFiltersEqual(filters, DEFAULT_WORKFLOW_LIBRARY_FILTERS), false);
});
