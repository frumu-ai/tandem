#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const repoRoot = path.resolve(new URL("..", import.meta.url).pathname);

const defaultTargets = [
  "crates/tandem-server/src/app/state/approval_message_map.rs",
  "crates/tandem-server/src/pack_manager.rs",
  "crates/tandem-server/src/bug_monitor/log_watcher.rs",
];

const args = process.argv.slice(2);
let maxPerFile = 0;
let reportAllServer = false;
let selfTest = false;
const targets = [];

for (const arg of args) {
  if (arg === "--self-test") {
    selfTest = true;
    continue;
  }
  if (arg === "--all-server") {
    reportAllServer = true;
    continue;
  }
  if (arg.startsWith("--max-per-file=")) {
    maxPerFile = Number.parseInt(arg.split("=", 2)[1], 10);
    if (!Number.isFinite(maxPerFile) || maxPerFile < 0) {
      console.error(`Invalid --max-per-file value: ${arg}`);
      process.exit(2);
    }
    continue;
  }
  targets.push(arg);
}

function listRustFiles(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const absolute = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "target") {
        continue;
      }
      files.push(...listRustFiles(absolute));
    } else if (entry.isFile() && entry.name.endsWith(".rs")) {
      files.push(path.relative(repoRoot, absolute));
    }
  }
  return files;
}

function isRustTestPath(file) {
  const normalized = file.split(path.sep).join("/");
  const segments = normalized.split("/");
  return (
    segments.some((segment) => segment === "tests" || segment.startsWith("tests_")) ||
    normalized.endsWith("/tests.rs") ||
    normalized.endsWith("_tests.rs") ||
    normalized.includes("_tests/")
  );
}

function lineStarts(source) {
  const starts = [0];
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === "\n") {
      starts.push(index + 1);
    }
  }
  return starts;
}

function lineForOffset(starts, offset) {
  let low = 0;
  let high = starts.length - 1;
  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    if (starts[mid] <= offset) {
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return high + 1;
}

function findMatchingBrace(source, openIndex) {
  let depth = 0;
  let inLineComment = false;
  let inBlockComment = false;
  let inString = false;
  let inChar = false;
  let escaped = false;

  for (let index = openIndex; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];

    if (inLineComment) {
      if (char === "\n") {
        inLineComment = false;
      }
      continue;
    }
    if (inBlockComment) {
      if (char === "*" && next === "/") {
        inBlockComment = false;
        index += 1;
      }
      continue;
    }
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === "\"") {
        inString = false;
      }
      continue;
    }
    if (inChar) {
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === "'") {
        inChar = false;
      }
      continue;
    }

    if (char === "/" && next === "/") {
      inLineComment = true;
      index += 1;
      continue;
    }
    if (char === "/" && next === "*") {
      inBlockComment = true;
      index += 1;
      continue;
    }
    if (char === "\"") {
      inString = true;
      continue;
    }
    if (char === "'") {
      inChar = true;
      continue;
    }
    if (char === "{") {
      depth += 1;
    } else if (char === "}") {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }
  return source.length - 1;
}

function stripCfgTestBlocks(source) {
  const cfgTest = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g;
  let output = "";
  let cursor = 0;
  let match;

  while ((match = cfgTest.exec(source)) !== null) {
    output += source.slice(cursor, match.index);
    const openIndex = source.indexOf("{", cfgTest.lastIndex);
    const semicolonIndex = source.indexOf(";", cfgTest.lastIndex);
    if (openIndex === -1 || (semicolonIndex !== -1 && semicolonIndex < openIndex)) {
      cursor = semicolonIndex === -1 ? cfgTest.lastIndex : semicolonIndex + 1;
      continue;
    }
    const closeIndex = findMatchingBrace(source, openIndex);
    const removed = source.slice(match.index, closeIndex + 1).replace(/[^\n]/g, " ");
    output += removed;
    cursor = closeIndex + 1;
    cfgTest.lastIndex = cursor;
  }
  output += source.slice(cursor);
  return output;
}

function stripCommentsAndLiterals(source) {
  let output = "";
  let inLineComment = false;
  let inBlockComment = false;
  let inString = false;
  let escaped = false;

  for (let index = 0; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];

    if (inLineComment) {
      if (char === "\n") {
        inLineComment = false;
        output += "\n";
      } else {
        output += " ";
      }
      continue;
    }
    if (inBlockComment) {
      if (char === "*" && next === "/") {
        inBlockComment = false;
        output += "  ";
        index += 1;
      } else {
        output += char === "\n" ? "\n" : " ";
      }
      continue;
    }
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === "\"") {
        inString = false;
      }
      output += char === "\n" ? "\n" : " ";
      continue;
    }
    if (char === "/" && next === "/") {
      inLineComment = true;
      output += "  ";
      index += 1;
      continue;
    }
    if (char === "/" && next === "*") {
      inBlockComment = true;
      output += "  ";
      index += 1;
      continue;
    }
    if (char === "\"") {
      inString = true;
      output += " ";
      continue;
    }
    output += char;
  }

  return output;
}

function panicFindings(file) {
  const absolute = path.join(repoRoot, file);
  const source = fs.readFileSync(absolute, "utf8");
  return panicFindingsForSource(source);
}

function panicFindingsForSource(source) {
  const productionSource = stripCommentsAndLiterals(stripCfgTestBlocks(source));
  const starts = lineStarts(productionSource);
  const panicMacro = /\bpanic!\s*[\(\[\{]/;
  const methodCall = /\.(unwrap|expect)\s*\(/;
  const ufcsCall = /\b[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*::(unwrap|expect)\s*\(/;
  const pattern = new RegExp(
    `${panicMacro.source}|${methodCall.source}|${ufcsCall.source}`,
    "g",
  );
  const findings = [];
  let match;
  while ((match = pattern.exec(productionSource)) !== null) {
    findings.push({
      line: lineForOffset(starts, match.index),
      expression: match[0].trim(),
    });
  }
  return findings;
}

function runSelfTest() {
  const source = `
fn production(result: Result<u8, ()>, option: Option<u8>, value: Result<u8, ()>) {
    panic!("paren");
    panic!["bracket"];
    panic! { "brace" };
    let _ = value.unwrap();
    let _ = value.expect("method");
    let _ = Result::unwrap(result);
    let _ = Option::expect(option, "ufcs");
    let _ = "panic!(string literal) and Result::unwrap(fake)";
    // panic!("comment") and Result::unwrap(comment)
}

#[cfg(test)]
mod tests {
    fn test_only(result: Result<u8, ()>) {
        panic!("ignored");
        let _ = Result::unwrap(result);
    }
}
`;
  const findings = panicFindingsForSource(source);
  if (findings.length !== 7) {
    console.error(`Self-test expected 7 findings, got ${findings.length}.`);
    console.error(JSON.stringify(findings, null, 2));
    process.exit(1);
  }
  console.log("Rust production panic-surface self-test passed.");
}

if (selfTest) {
  runSelfTest();
  process.exit(0);
}

const files = reportAllServer
  ? listRustFiles(path.join(repoRoot, "crates/tandem-server/src")).filter(
      (file) => !isRustTestPath(file),
    )
  : targets.length > 0
    ? targets
    : defaultTargets;

let failed = false;
let total = 0;

console.log("Rust production panic-surface check");
for (const file of files) {
  const findings = panicFindings(file);
  total += findings.length;
  console.log(`${file}: ${findings.length}`);
  for (const finding of findings) {
    console.log(`  line ${finding.line}: ${finding.expression}`);
  }
  if (findings.length > maxPerFile) {
    failed = true;
  }
}

console.log(`Total production panic-surface findings: ${total}`);
if (failed) {
  console.error(`One or more files exceeded --max-per-file=${maxPerFile}.`);
  process.exit(1);
}
