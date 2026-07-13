import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRequire } from "node:module";
import test from "node:test";
import ts from "typescript";

function loadModel() {
  const directory = mkdtempSync(join(tmpdir(), "tandem-workflow-flow-"));
  writeFileSync(join(directory, "package.json"), JSON.stringify({ type: "commonjs" }));
  const source = readFileSync(
    new URL("../src/features/automations/workflowFlowModel.ts", import.meta.url),
    "utf8"
  );
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: "workflowFlowModel.ts",
  });
  const output = join(directory, "workflowFlowModel.js");
  writeFileSync(output, compiled.outputText);
  const require = createRequire(import.meta.url);
  return { model: require(output), cleanup: () => rmSync(directory, { recursive: true }) };
}

test("workflow flow stages preserve fan-out and fan-in concurrency", () => {
  const { model, cleanup } = loadModel();
  try {
    const graph = model.buildWorkflowFlowGraph({
      executionMode: "swarm",
      maxParallelAgents: 3,
      nodes: [
        { nodeId: "plan", dependsOn: [] },
        { nodeId: "research", dependsOn: ["plan"] },
        { nodeId: "implement", dependsOn: ["plan"] },
        { nodeId: "verify", dependsOn: ["plan"] },
        { nodeId: "publish", dependsOn: ["research", "implement", "verify"] },
      ],
    });

    assert.deepEqual(
      graph.stages.map((stage) => stage.nodes.map((node) => node.nodeId)),
      [["plan"], ["research", "implement", "verify"], ["publish"]]
    );
    assert.equal(graph.parallelStageCount, 1);
    assert.equal(graph.maxConcurrentTasks, 3);
    assert.equal(graph.edgeCount, 6);
    assert.equal(graph.startCount, 1);
  } finally {
    cleanup();
  }
});

test("workflow flow stages respect execution caps and report missing dependencies", () => {
  const { model, cleanup } = loadModel();
  try {
    const nodes = [
      { node_id: "one", depends_on: [] },
      { node_id: "two", depends_on: [] },
      { node_id: "three", depends_on: ["missing"] },
    ];
    const capped = model.buildWorkflowFlowGraph({
      nodes,
      executionMode: "team",
      maxParallelAgents: 2,
    });
    const single = model.buildWorkflowFlowGraph({
      nodes,
      executionMode: "single",
      maxParallelAgents: 8,
    });

    assert.equal(capped.maxConcurrentTasks, 2);
    assert.equal(capped.missingDependencyCount, 1);
    assert.equal(single.concurrencyLimit, 1);
    assert.equal(single.maxConcurrentTasks, 1);
  } finally {
    cleanup();
  }
});
