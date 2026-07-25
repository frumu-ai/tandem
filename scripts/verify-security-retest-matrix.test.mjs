import test from "node:test";
import assert from "node:assert/strict";

import {
  loadMatrix,
  matrixSymbols,
  validateMatrix,
  verifyNextestEvidence,
} from "./verify-security-retest-matrix.mjs";

test("the committed matrix covers TA-01 through TA-17 with real test symbols", () => {
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

test("nextest evidence requires discovery and execution", () => {
  const matrix = loadMatrix();
  const symbols = matrixSymbols(matrix);
  const discovered = symbols.join("\n");
  const executed = `<testsuite>${symbols
    .map((symbol) => `<testcase classname="security" name="${symbol}"/>`)
    .join("")}</testsuite>`;
  assert.doesNotThrow(() => verifyNextestEvidence(matrix, discovered, executed));
  assert.throws(
    () => verifyNextestEvidence(matrix, discovered, executed.replace(symbols[0], "")),
    /not executed/,
  );
  assert.throws(
    () =>
      verifyNextestEvidence(
        matrix,
        discovered.replace(symbols[0], `${symbols[0]}_lookalike`),
        executed,
      ),
    /not discovered/,
  );
  assert.throws(
    () =>
      verifyNextestEvidence(
        matrix,
        discovered,
        executed.replace(
          `<testcase classname="security" name="${symbols[0]}"/>`,
          `<testcase classname="security" name="${symbols[0]}"><skipped/></testcase>`,
        ),
      ),
    /not executed/,
  );
});
