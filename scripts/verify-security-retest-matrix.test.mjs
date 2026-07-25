import test from "node:test";
import assert from "node:assert/strict";

import {
  loadMatrix,
  matrixTests,
  validateMatrix,
  verifyNextestEvidence,
} from "./verify-security-retest-matrix.mjs";

function discoveryFor(tests) {
  const suites = {};
  for (const control of tests) {
    const suite = (suites[control.binary_id] ??= {
      "package-name": control.package,
      testcases: {},
    });
    suite.testcases[control.test_name] = { ignored: false };
  }
  return { "rust-suites": suites };
}

function junitFor(tests) {
  return `<testsuites>${tests
    .map(
      (control) =>
        `<testcase classname="${control.binary_id}" name="${control.test_name}"/>`,
    )
    .join("")}</testsuites>`;
}

test("the committed matrix covers TA-01 through TA-17 with exact test identities", () => {
  const matrix = loadMatrix();
  const summary = validateMatrix(matrix);
  assert.equal(summary.finding_count, 17);
  assert.ok(summary.test_count >= 34);
});

test("missing positive controls fail closed", () => {
  const matrix = structuredClone(loadMatrix());
  matrix.findings[0].positive_controls = [];
  assert.throws(() => validateMatrix(matrix), /positive_controls/);
});

test("stale test symbols fail closed", () => {
  const matrix = structuredClone(loadMatrix());
  matrix.findings[0].negative_controls[0].symbol = "removed_security_test";
  assert.throws(() => validateMatrix(matrix), /test symbol not found/);
});

test("a lookalike binary prefix is not package-qualified evidence", () => {
  const matrix = structuredClone(loadMatrix());
  matrix.findings[0].negative_controls[0].binary_id = "tandem-server-lookalike";
  assert.throws(() => validateMatrix(matrix), /invalid binary_id/);
});

test("nextest evidence requires discovery and execution", () => {
  const matrix = loadMatrix();
  const tests = matrixTests(matrix);
  const discovery = discoveryFor(tests);
  const junit = junitFor(tests);
  assert.doesNotThrow(() =>
    verifyNextestEvidence(matrix, JSON.stringify(discovery), junit),
  );

  const first = tests[0];
  const missingExecution = junit.replace(
    `<testcase classname="${first.binary_id}" name="${first.test_name}"/>`,
    "",
  );
  assert.throws(
    () => verifyNextestEvidence(matrix, JSON.stringify(discovery), missingExecution),
    /not executed/,
  );

  const wrongBinary = structuredClone(discovery);
  delete wrongBinary["rust-suites"][first.binary_id].testcases[first.test_name];
  wrongBinary["rust-suites"]["lookalike-package"] = {
    "package-name": first.package,
    testcases: { [first.test_name]: { ignored: false } },
  };
  assert.throws(
    () =>
      verifyNextestEvidence(
        matrix,
        JSON.stringify(wrongBinary),
        junit,
      ),
    /not discovered/,
  );

  const skipped = junit.replace(
    `<testcase classname="${first.binary_id}" name="${first.test_name}"/>`,
    `<testcase classname="${first.binary_id}" name="${first.test_name}"><skipped/></testcase>`,
  );
  assert.throws(
    () =>
      verifyNextestEvidence(
        matrix,
        JSON.stringify(discovery),
        skipped,
      ),
    /not executed/,
  );
});
