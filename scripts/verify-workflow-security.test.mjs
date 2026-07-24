import assert from "node:assert/strict";
import test from "node:test";

import { inspectWorkflowSource } from "./verify-workflow-security.mjs";

test("rejects movable action references", () => {
  const source = `name: insecure
on: push
permissions:
  contents: read
jobs:
  test:
    steps:
      - uses: actions/checkout@v4
      - uses: docker://alpine:latest
`;
  assert.deepEqual(inspectWorkflowSource(source, "workflow.yml"), [
    "workflow.yml:8: action is not pinned to a full SHA",
    "workflow.yml:9: container action is not digest-pinned",
  ]);
});

test("accepts full action SHAs and local actions", () => {
  const source = `name: secure
on: push
permissions:
  contents: read
jobs:
  test:
    steps:
      - uses: actions/checkout@${"a".repeat(40)} # v4
      - uses: ./.github/actions/setup-rust-ci
`;
  assert.deepEqual(inspectWorkflowSource(source, "workflow.yml"), []);
});

test("rejects global write permissions and global secrets", () => {
  const source = `name: insecure
on: push
permissions:
  contents: write
env:
  TAURI_SIGNING_PRIVATE_KEY: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
jobs: {}
`;
  assert.deepEqual(inspectWorkflowSource(source, "workflow.yml"), [
    "workflow.yml:4: write permission must be scoped to the job that needs it",
    "workflow.yml: signing credentials and secrets must not be workflow-global",
  ]);
});

test("requires an explicit workflow permission baseline", () => {
  const source = `name: insecure
on: push
jobs:
  test:
    steps: []
`;
  assert.deepEqual(inspectWorkflowSource(source, "workflow.yml"), [
    "workflow.yml: workflow must declare top-level permissions",
  ]);
});

test("allows narrowly scoped job write permissions", () => {
  const source = `name: secure
on: push
permissions:
  contents: read
jobs:
  release:
    permissions:
      contents: write
    steps: []
`;
  assert.deepEqual(inspectWorkflowSource(source, "workflow.yml"), []);
});
