#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceRoot = path.join(root, "crates");

function rustFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) return rustFiles(entryPath);
    return entry.isFile() && entry.name.endsWith(".rs") ? [entryPath] : [];
  });
}

function matchingParen(source, open) {
  let depth = 0;
  let quote = null;
  for (let index = open; index < source.length; index += 1) {
    const char = source[index];
    if (quote) {
      if (char === "\\") index += 1;
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }
    if (char === "(") depth += 1;
    else if (char === ")" && --depth === 0) return index;
  }
  return source.length - 1;
}

function silentDiscards(source) {
  const calls = /\bappend_protected_audit_event\s*\(/g;
  const violations = [];
  for (const match of source.matchAll(calls)) {
    const offset = match.index ?? 0;
    const open = source.indexOf("(", offset);
    const close = matchingParen(source, open);
    const start = Math.max(
      source.lastIndexOf(";", offset),
      source.lastIndexOf("{", offset),
      source.lastIndexOf("}", offset),
    ) + 1;
    const semicolon = source.indexOf(";", close);
    const closingBlock = source.slice(close).search(/\n\s*}/);
    const blockEnd = closingBlock === -1 ? source.length : close + closingBlock + 1;
    const end = semicolon === -1 ? blockEnd : Math.min(semicolon + 1, blockEnd);
    const statement = source.slice(start, end);
    const discardedBinding = /^\s*(?:let\s+_[A-Za-z0-9_]*|_)\s*=/.test(statement);
    const unhandledNamedBinding =
      /^\s*let\s+[A-Za-z][A-Za-z0-9_]*\s*=/.test(statement) &&
      /\.await\s*;\s*$/.test(statement);
    const discardedCall = /\bdrop\s*\([\s\S]*\bappend_protected_audit_event\s*\(/.test(
      statement,
    );
    const discardedAdapter = /\.await\s*\.\s*(?:ok|is_ok|unwrap_or_default)\s*\(/.test(
      statement,
    );
    const bareAwait =
      /^\s*(?:(?:crate|super|self|[A-Za-z_][A-Za-z0-9_]*)::)*append_protected_audit_event\s*\(/.test(
        statement,
      ) && /\.await\s*;\s*$/.test(statement);
    if (
      discardedBinding ||
      unhandledNamedBinding ||
      discardedCall ||
      discardedAdapter ||
      bareAwait
    ) {
      violations.push(offset);
    }
  }
  return violations;
}

function lineNumber(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

if (process.argv.includes("--self-test")) {
  const rejected = [
    "let _ = crate::audit::append_protected_audit_event(&state).await;",
    "let _ignored = crate::audit::append_protected_audit_event(&state).await;",
    "_ = crate::audit::append_protected_audit_event(&state).await;",
    "drop(crate::audit::append_protected_audit_event(&state).await);",
    "crate::audit::append_protected_audit_event(&state).await.ok();",
    "crate::audit::append_protected_audit_event(&state).await;",
    "let result = crate::audit::append_protected_audit_event(&state).await; drop(result);",
  ];
  const accepted =
    "crate::audit::append_protected_audit_event_best_effort(&state).await;";
  if (
    rejected.some((mutation) => silentDiscards(mutation).length !== 1) ||
    silentDiscards(accepted).length !== 0
  ) {
    throw new Error("protected audit result guard self-test failed");
  }
  process.stdout.write("protected audit result guard self-test passed\n");
  process.exit(0);
}

const violations = [];
for (const file of rustFiles(sourceRoot)) {
  const source = fs.readFileSync(file, "utf8");
  for (const offset of silentDiscards(source)) {
    violations.push(`${path.relative(root, file)}:${lineNumber(source, offset)}`);
  }
}

if (violations.length > 0) {
  process.stderr.write(
    "Protected audit results may not be silently discarded. Propagate the error or use " +
      "append_protected_audit_event_best_effort for an intentional best-effort path:\n" +
      violations.map((violation) => `  ${violation}`).join("\n") +
      "\n",
  );
  process.exit(1);
}

process.stdout.write("protected audit result guard passed\n");
