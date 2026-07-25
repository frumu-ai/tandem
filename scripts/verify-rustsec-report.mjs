#!/usr/bin/env node

import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const advisoryPattern = /RUSTSEC-\d{4}-\d{4}/g;
const auditConfig = readFileSync(resolve(root, ".cargo/audit.toml"), "utf8");
const lockfileText = readFileSync(resolve(root, "Cargo.lock"), "utf8");
const ignoreBlock = auditConfig.match(/\[advisories\][\s\S]*?ignore\s*=\s*\[([\s\S]*?)\]/);
if (!ignoreBlock) throw new Error("missing Cargo Audit exception list");
const expected = [...new Set(ignoreBlock[1].match(advisoryPattern) || [])].sort();

const scratch = mkdtempSync(resolve(tmpdir(), "tandem-rustsec-"));
try {
  const result = spawnSync(
    "cargo",
    ["audit", "--no-fetch", "--json", "--file", resolve(root, "Cargo.lock")],
    { cwd: scratch, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }
  );
  if (result.error) throw result.error;
  let report;
  try {
    report = JSON.parse(result.stdout);
  } catch {
    throw new Error(`Cargo Audit did not emit JSON: ${result.stderr || result.stdout}`);
  }

  const reported = new Set();
  for (const finding of report.vulnerabilities?.list || []) {
    if (finding.advisory?.id) reported.add(finding.advisory.id);
  }
  const yanked = [];
  for (const [kind, findings] of Object.entries(report.warnings || {})) {
    for (const finding of findings || []) {
      if (finding.advisory?.id) reported.add(finding.advisory.id);
      else if (kind === "yanked") yanked.push(`${finding.package?.name}@${finding.package?.version}`);
    }
  }
  if (yanked.length) throw new Error(`yanked packages are not allowed: ${yanked.join(", ")}`);

  const actual = [...reported].sort();
  const unexpected = actual.filter((id) => !expected.includes(id));
  const absent = expected.filter((id) => !reported.has(id));
  if (unexpected.length || absent.length) {
    throw new Error(`ignore-free RustSec mismatch; unexpected=${unexpected.join(",")}; absent=${absent.join(",")}`);
  }

  const quickXml = [...lockfileText.matchAll(/name = "quick-xml"\nversion = "([^"]+)"/g)]
    .map((match) => match[1])
    .filter((version) => /^0\.(?:[0-3]\d|39)\./.test(version));
  if (quickXml.length) throw new Error(`vulnerable quick-xml remains: ${quickXml.join(",")}`);

  console.log(JSON.stringify({
    scanner: "cargo-audit",
    lockfile: basename(resolve(root, "Cargo.lock")),
    dependencies: report.lockfile?.["dependency-count"],
    acceptedAdvisories: actual.length,
    unexpectedAdvisories: 0,
    yankedPackages: 0,
  }));
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
