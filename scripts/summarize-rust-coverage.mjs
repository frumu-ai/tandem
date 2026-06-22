#!/usr/bin/env node
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

function parseArgs(argv) {
  const out = {};
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--self-test") {
      out.selfTest = true;
      continue;
    }
    if (!arg.startsWith("--")) {
      throw new Error(`unexpected argument: ${arg}`);
    }
    const key = arg.slice(2);
    const value = argv[i + 1];
    if (!value || value.startsWith("--")) {
      throw new Error(`${arg} requires a value`);
    }
    out[key] = value;
    i += 1;
  }
  return out;
}

function crateForSource(sourcePath) {
  const normalized = sourcePath.replaceAll("\\", "/");
  const match = normalized.match(/(?:^|\/)crates\/([^/]+)\//);
  return match?.[1] ?? null;
}

function parseLcov(text) {
  const crates = new Map();
  let currentSource = null;
  let currentFound = 0;
  let currentHit = 0;

  function flush() {
    if (!currentSource) {
      return;
    }
    const crateName = crateForSource(currentSource);
    if (crateName) {
      const row = crates.get(crateName) ?? { found: 0, hit: 0, files: 0 };
      row.found += currentFound;
      row.hit += currentHit;
      row.files += 1;
      crates.set(crateName, row);
    }
    currentSource = null;
    currentFound = 0;
    currentHit = 0;
  }

  for (const line of text.split(/\r?\n/)) {
    if (line.startsWith("SF:")) {
      flush();
      currentSource = line.slice(3);
    } else if (line.startsWith("LF:")) {
      currentFound = Number.parseInt(line.slice(3), 10) || 0;
    } else if (line.startsWith("LH:")) {
      currentHit = Number.parseInt(line.slice(3), 10) || 0;
    } else if (line === "end_of_record") {
      flush();
    }
  }
  flush();
  return crates;
}

function percent(hit, found) {
  if (!found) {
    return 0;
  }
  return Number(((hit / found) * 100).toFixed(2));
}

function loadBaseline(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function summarize(lcovText, baseline) {
  const coverage = parseLcov(lcovText);
  const crateNames = Object.keys(baseline.crates ?? {}).sort();
  return crateNames.map((crateName) => {
    const row = coverage.get(crateName) ?? { found: 0, hit: 0, files: 0 };
    const coveragePercent = percent(row.hit, row.found);
    const baselinePercent =
      baseline.crates?.[crateName]?.line_coverage_percent ?? null;
    const delta =
      typeof baselinePercent === "number"
        ? Number((coveragePercent - baselinePercent).toFixed(2))
        : null;
    return {
      crate: crateName,
      files: row.files,
      lines_found: row.found,
      lines_hit: row.hit,
      line_coverage_percent: coveragePercent,
      baseline_percent: baselinePercent,
      delta_percent: delta,
    };
  });
}

function renderMarkdown(rows) {
  const lines = [
    "# Rust Governance Coverage",
    "",
    "| Crate | Files | Lines hit/found | Coverage | Baseline | Delta |",
    "| --- | ---: | ---: | ---: | ---: | ---: |",
  ];
  for (const row of rows) {
    const baseline =
      row.baseline_percent === null ? "n/a" : `${row.baseline_percent.toFixed(2)}%`;
    const delta =
      row.delta_percent === null
        ? "n/a"
        : `${row.delta_percent >= 0 ? "+" : ""}${row.delta_percent.toFixed(2)}%`;
    lines.push(
      `| \`${row.crate}\` | ${row.files} | ${row.lines_hit}/${row.lines_found} | ${row.line_coverage_percent.toFixed(2)}% | ${baseline} | ${delta} |`,
    );
  }
  lines.push("");
  return `${lines.join("\n")}\n`;
}

function writeOutputs(rows, outMd, outJson) {
  fs.mkdirSync(path.dirname(outMd), { recursive: true });
  fs.writeFileSync(outMd, renderMarkdown(rows));
  fs.writeFileSync(outJson, `${JSON.stringify({ crates: rows }, null, 2)}\n`);
}

function selfTest() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "coverage-summary-"));
  const lcov = [
    "TN:",
    "SF:/repo/crates/tandem-tools/src/lib.rs",
    "LF:10",
    "LH:7",
    "end_of_record",
    "TN:",
    "SF:/repo/crates/tandem-plan-compiler/src/lib.rs",
    "LF:4",
    "LH:1",
    "end_of_record",
  ].join("\n");
  const baseline = {
    crates: {
      "tandem-tools": { line_coverage_percent: 50 },
      "tandem-plan-compiler": { line_coverage_percent: 10 },
    },
  };
  const rows = summarize(lcov, baseline);
  assert.equal(rows.length, 2);
  assert.equal(rows.find((row) => row.crate === "tandem-tools").line_coverage_percent, 70);
  const outMd = path.join(tmp, "summary.md");
  const outJson = path.join(tmp, "summary.json");
  writeOutputs(rows, outMd, outJson);
  assert.match(fs.readFileSync(outMd, "utf8"), /tandem-tools/);
  assert.equal(JSON.parse(fs.readFileSync(outJson, "utf8")).crates.length, 2);
}

const args = parseArgs(process.argv);
if (args.selfTest) {
  selfTest();
} else {
  for (const key of ["lcov", "baseline", "out-md", "out-json"]) {
    if (!args[key]) {
      throw new Error(`missing --${key}`);
    }
  }
  const rows = summarize(
    fs.readFileSync(args.lcov, "utf8"),
    loadBaseline(args.baseline),
  );
  writeOutputs(rows, args["out-md"], args["out-json"]);
}
