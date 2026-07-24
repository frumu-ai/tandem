#!/usr/bin/env node

import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const FULL_SHA = /^[0-9a-f]{40}$/;

function stripComment(value) {
  return value.replace(/\s+#.*$/, "").trim();
}

export function inspectWorkflowSource(source, filename, requirePermissions = true) {
  const errors = [];
  const lines = source.split(/\r?\n/);

  for (const [index, line] of lines.entries()) {
    const match = line.match(/^\s*(?:-\s*)?uses:\s*([^\s#]+)/);
    if (!match) continue;
    const reference = stripComment(match[1]);
    if (reference.startsWith("./")) continue;
    if (reference.startsWith("docker://")) {
      if (!/@sha256:[0-9a-f]{64}$/.test(reference)) {
        errors.push(`${filename}:${index + 1}: container action is not digest-pinned`);
      }
      continue;
    }
    const separator = reference.lastIndexOf("@");
    const revision = separator >= 0 ? reference.slice(separator + 1) : "";
    if (separator <= 0 || !FULL_SHA.test(revision)) {
      errors.push(`${filename}:${index + 1}: action is not pinned to a full SHA`);
    }
  }

  for (const [index, line] of lines.entries()) {
    if (/^\s*permissions:\s*(write-all|read-all)\s*(?:#.*)?$/.test(line)) {
      errors.push(`${filename}:${index + 1}: broad permissions shorthand is forbidden`);
    }
  }

  if (requirePermissions) {
    const permissionsIndex = lines.findIndex((line) => /^permissions:\s*(?:#.*)?$/.test(line));
    if (permissionsIndex < 0) {
      errors.push(`${filename}: workflow must declare top-level permissions`);
    } else {
      for (let index = permissionsIndex + 1; index < lines.length; index += 1) {
        const line = lines[index];
        if (/^[A-Za-z_]/.test(line)) break;
        const match = line.match(/^\s{2}([a-z-]+):\s*([^#\s]+)/);
        if (match && match[2] === "write") {
          errors.push(
            `${filename}:${index + 1}: write permission must be scoped to the job that needs it`
          );
        }
      }
    }

    const jobsIndex = lines.findIndex((line) => /^jobs:\s*(?:#.*)?$/.test(line));
    const topLevel = lines.slice(0, jobsIndex < 0 ? lines.length : jobsIndex).join("\n");
    if (
      /^\s{2,}[A-Z0-9_]*(?:SIGNING|TOKEN|SECRET|PASSWORD)[A-Z0-9_]*:\s*\$\{\{\s*secrets\./m.test(
        topLevel
      )
    ) {
      errors.push(`${filename}: signing credentials and secrets must not be workflow-global`);
    }
  }

  return errors;
}

async function yamlFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await yamlFiles(fullPath)));
    } else if (/\.ya?ml$/.test(entry.name)) {
      files.push(fullPath);
    }
  }
  return files.sort();
}

export async function verifyWorkflowSecurity(root = process.cwd()) {
  const workflowDirectory = path.join(root, ".github", "workflows");
  const actionDirectory = path.join(root, ".github", "actions");
  const files = [...(await yamlFiles(workflowDirectory)), ...(await yamlFiles(actionDirectory))];
  const errors = [];
  for (const filename of files) {
    const source = await readFile(filename, "utf8");
    errors.push(
      ...inspectWorkflowSource(
        source,
        path.relative(root, filename),
        filename.startsWith(workflowDirectory)
      )
    );
  }
  return errors;
}

async function selfTest() {
  const insecure = `name: test
on: push
permissions:
  contents: write
jobs:
  test:
    steps:
      - uses: actions/checkout@v4
`;
  const secure = `name: test
on: push
permissions:
  contents: read
jobs:
  test:
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@${"a".repeat(40)} # v4
`;
  if (inspectWorkflowSource(insecure, "insecure.yml").length !== 2) {
    throw new Error("workflow security self-test failed to reject insecure input");
  }
  if (inspectWorkflowSource(secure, "secure.yml").length !== 0) {
    throw new Error("workflow security self-test rejected secure input");
  }
}

async function main() {
  if (process.argv.includes("--self-test")) await selfTest();
  const errors = await verifyWorkflowSecurity();
  if (errors.length > 0) {
    throw new Error(`workflow security policy failed:\n${errors.join("\n")}`);
  }
  process.stdout.write("workflow security policy passed\n");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}
