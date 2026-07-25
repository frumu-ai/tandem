#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const advisoryPattern = /RUSTSEC-\d{4}-\d{4}/g;

function uniqueIds(text) {
  return [...new Set(text.match(advisoryPattern) || [])].sort();
}

function ignoredIds(path) {
  const text = readFileSync(path, "utf8");
  const block = text.match(/\[advisories\][\s\S]*?ignore\s*=\s*\[([\s\S]*?)\]/);
  if (!block) throw new Error(`missing [advisories].ignore in ${path}`);
  return uniqueIds(block[1]);
}

const auditPath = resolve(root, ".cargo/audit.toml");
const denyPath = resolve(root, ".config/deny.toml");
const docsPath = resolve(root, "docs/CI_SECURITY_AND_COVERAGE.md");
const auditIds = ignoredIds(auditPath);
const denyIds = ignoredIds(denyPath);
if (JSON.stringify(auditIds) !== JSON.stringify(denyIds)) {
  throw new Error("Cargo Audit and Cargo Deny advisory exceptions differ");
}

const docs = readFileSync(docsPath, "utf8");
const section = docs.match(/### Current Advisory Exceptions([\s\S]*?)### Current License Exceptions/);
if (!section) throw new Error("missing Current Advisory Exceptions table");

const documented = new Map();
for (const line of section[1].split("\n")) {
  if (!line.startsWith("|") || /^\|\s*(?:---|Advisory IDs)/.test(line)) continue;
  const cells = line.split("|").slice(1, -1).map((cell) => cell.trim());
  if (cells.length !== 5) throw new Error(`malformed exception row: ${line}`);
  const [idsCell, , owner, control, expires] = cells;
  const ids = uniqueIds(idsCell);
  if (!ids.length) throw new Error(`exception row has no advisory ID: ${line}`);
  if (!owner || owner.length < 3) throw new Error(`exception row has no owner: ${line}`);
  if (!/reach|call path|untrusted boundary|build-time|compile-time|local document|Tauri|limited to|transitive through/i.test(control) || control.length < 80) {
    throw new Error(`exception row lacks reachability and compensating-control evidence: ${line}`);
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(expires) || Date.parse(`${expires}T23:59:59Z`) <= Date.now()) {
    throw new Error(`exception row is expired or has an invalid expiry: ${line}`);
  }
  for (const id of ids) {
    if (documented.has(id)) throw new Error(`duplicate documented exception ${id}`);
    documented.set(id, { owner, control, expires });
  }
}

const documentedIds = [...documented.keys()].sort();
if (JSON.stringify(auditIds) !== JSON.stringify(documentedIds)) {
  const missing = auditIds.filter((id) => !documented.has(id));
  const stale = documentedIds.filter((id) => !auditIds.includes(id));
  throw new Error(`exception documentation mismatch; missing=${missing.join(",")}; stale=${stale.join(",")}`);
}

console.log(JSON.stringify({ verifiedExceptions: auditIds.length, advisoryIds: auditIds }));
