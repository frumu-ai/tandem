import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRequire } from "node:module";
import test from "node:test";
import ts from "typescript";

const sourceDir = new URL("../src/features/orchestration-studio/", import.meta.url);
const moduleNames = ["model", "graph", "layout", "history", "validation", "editing"];

function loadHelpers() {
  const directory = mkdtempSync(join(tmpdir(), "tandem-orchestration-model-"));
  writeFileSync(join(directory, "package.json"), JSON.stringify({ type: "commonjs" }));
  for (const name of moduleNames) {
    const source = readFileSync(new URL(`${name}.ts`, sourceDir), "utf8");
    const compiled = ts.transpileModule(source, {
      compilerOptions: {
        module: ts.ModuleKind.CommonJS,
        target: ts.ScriptTarget.ES2022,
        esModuleInterop: true,
      },
      fileName: `${name}.ts`,
    });
    writeFileSync(join(directory, `${name}.js`), compiled.outputText);
  }
  const require = createRequire(import.meta.url);
  const loaded = Object.fromEntries(
    moduleNames.map((name) => [name, require(join(directory, `${name}.js`))])
  );
  return { loaded, cleanup: () => rmSync(directory, { recursive: true, force: true }) };
}

function boundedLoopSpec() {
  const root = {
    node_id: "plan",
    name: "Plan",
    position: { x: 0, y: 0 },
    kind: "workflow",
    automation_id: "automation-plan",
    allowed_transition_keys: ["execute"],
  };
  const execute = {
    node_id: "execute",
    name: "Execute",
    position: { x: 0, y: 0 },
    kind: "workflow",
    automation_id: "automation-execute",
    allowed_transition_keys: ["replan", "complete"],
  };
  const complete = {
    node_id: "complete",
    name: "Complete",
    position: { x: 0, y: 0 },
    kind: "terminal",
    outcome: "complete",
  };
  return {
    schema_version: 1,
    orchestration_id: "orch-test",
    name: "Bounded loop",
    status: "draft",
    version: 0,
    root_node_id: root.node_id,
    nodes: [root, execute, complete],
    edges: [
      {
        edge_id: "plan-execute",
        from_node_id: "plan",
        to_node_id: "execute",
        transition_key: "execute",
      },
      {
        edge_id: "execute-plan",
        from_node_id: "execute",
        to_node_id: "plan",
        transition_key: "replan",
      },
      {
        edge_id: "execute-complete",
        from_node_id: "execute",
        to_node_id: "complete",
        transition_key: "complete",
      },
    ],
    goal_policy: { max_hops: 20, on_limit: "pause_for_review" },
    tenant_context: { org_id: "local", workspace_id: "local", source: "local_implicit" },
    created_at_ms: 1,
    updated_at_ms: 1,
  };
}

test("orchestration graph helpers preserve canonical draft and canvas contracts", () => {
  const { loaded, cleanup } = loadHelpers();
  try {
    const spec = boundedLoopSpec();
    const graph = loaded.model.toFlowGraph(spec);
    graph.nodes[0].position = { x: 91, y: 37 };
    const roundTrip = loaded.model.fromFlowGraph(spec, graph);
    assert.equal(roundTrip.version, 0);
    assert.deepEqual(roundTrip.nodes[0].position, { x: 91, y: 37 });
    assert.equal(roundTrip.nodes[0].automation_id, "automation-plan");

    const analysis = loaded.graph.analyzeGraph(roundTrip);
    assert.equal(analysis.counts.loops, 1);
    assert.deepEqual(new Set(analysis.reachableTerminalIds), new Set(["complete"]));

    const laidOut = loaded.layout.autoLayoutLeftToRight(roundTrip);
    assert.ok(laidOut.nodes.every((node) => Number.isFinite(node.position.x)));
    assert.ok(
      laidOut.nodes.find((node) => node.node_id === "complete").position.x >
        laidOut.nodes.find((node) => node.node_id === "plan").position.x
    );
  } finally {
    cleanup();
  }
});

test("orchestration validation and immutable editing catch unsafe authoring", () => {
  const { loaded, cleanup } = loadHelpers();
  try {
    const spec = boundedLoopSpec();
    assert.equal(loaded.validation.validateOrchestrationDraft(spec).valid, true);

    const unsafe = {
      ...spec,
      goal_policy: { ...spec.goal_policy, max_hops: 0 },
      edges: [
        ...spec.edges,
        {
          edge_id: "terminal-loop",
          from_node_id: "complete",
          to_node_id: "plan",
          transition_key: "again",
        },
      ],
    };
    const codes = loaded.validation
      .validateOrchestrationDraft(unsafe)
      .issues.map((issue) => issue.code);
    assert.ok(codes.includes("invalid_max_hops"));
    assert.ok(codes.includes("terminal_has_outgoing_edge"));

    const withoutExecute = loaded.editing.removeNode(spec, "execute");
    assert.equal(withoutExecute.nodes.length, 2);
    assert.equal(withoutExecute.edges.length, 0);
    assert.equal(spec.nodes.length, 3, "editing helpers must not mutate the original spec");

    let history = loaded.history.createHistory(spec, 4);
    const renamed = { ...withoutExecute, name: "Renamed orchestration" };
    history = loaded.history.pushHistory(history, renamed);
    assert.equal(loaded.history.undo(history).present.nodes.length, 3);
    assert.equal(loaded.history.undo(history).present.name, spec.name);
    assert.equal(loaded.history.redo(loaded.history.undo(history)).present.nodes.length, 2);
    assert.equal(
      loaded.history.redo(loaded.history.undo(history)).present.name,
      "Renamed orchestration"
    );
  } finally {
    cleanup();
  }
});

test("orchestration validation allows branches with the same terminal outcome", () => {
  const { loaded, cleanup } = loadHelpers();
  try {
    const spec = boundedLoopSpec();
    const branched = {
      ...spec,
      edges: [
        ...spec.edges,
        {
          edge_id: "execute-alternate-complete",
          from_node_id: "execute",
          to_node_id: "alternate-complete",
          transition_key: "alternate-complete",
        },
      ],
      nodes: [
        ...spec.nodes.map((node) =>
          node.node_id === "execute"
            ? {
                ...node,
                allowed_transition_keys: [
                  ...node.allowed_transition_keys,
                  "alternate-complete",
                ],
              }
            : node
        ),
        {
          node_id: "alternate-complete",
          name: "Alternate complete",
          position: { x: 0, y: 0 },
          kind: "terminal",
          outcome: "complete",
        },
      ],
    };

    assert.equal(loaded.validation.validateOrchestrationDraft(branched).valid, true);
  } finally {
    cleanup();
  }
});
