#!/usr/bin/env node

import { cp, mkdir, readFile, readdir, stat, writeFile } from "fs/promises";
import path from "path";
import process from "process";
import readline from "readline/promises";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const TEMPLATE_DIR = path.join(__dirname, "template");
const DEFAULT_DIR = "tandem-panel-app";

function toPackageName(value) {
  return String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "") || "tandem-panel-app";
}

async function promptForTargetDir() {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
  });
  try {
    const answer = await rl.question(
      `Where should we create your editable Tandem panel? (${DEFAULT_DIR}) `
    );
    return String(answer || "").trim() || DEFAULT_DIR;
  } finally {
    rl.close();
  }
}

async function ensureTargetDir(targetDir) {
  const resolved = path.resolve(process.cwd(), targetDir);
  let existing = null;
  try {
    existing = await stat(resolved);
  } catch {}

  if (existing && !existing.isDirectory()) {
    throw new Error(`Target path exists and is not a directory: ${resolved}`);
  }

  if (existing) {
    const entries = await readdir(resolved);
    if (entries.length > 0) {
      throw new Error(`Target directory is not empty: ${resolved}`);
    }
  } else {
    await mkdir(resolved, { recursive: true });
  }

  return resolved;
}

async function rewriteTemplateFiles(targetDir, packageName) {
  const packageJsonPath = path.join(targetDir, "package.json");
  const readmePath = path.join(targetDir, "README.md");

  const packageJson = JSON.parse(await readFile(packageJsonPath, "utf8"));
  packageJson.name = packageName;
  await writeFile(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`, "utf8");

  const readme = await readFile(readmePath, "utf8");
  await writeFile(readmePath, readme.replaceAll("__PROJECT_NAME__", packageName), "utf8");
}

async function main() {
  const requestedTarget = String(process.argv[2] || "").trim() || (await promptForTargetDir());
  const targetDir = await ensureTargetDir(requestedTarget);
  const packageName = toPackageName(path.basename(targetDir));

  await cp(TEMPLATE_DIR, targetDir, { recursive: true });
  await rewriteTemplateFiles(targetDir, packageName);

  const relativeTarget = path.relative(process.cwd(), targetDir) || ".";
  const cdTarget = path.isAbsolute(requestedTarget) ? targetDir : relativeTarget;
  console.log("");
  console.log("Editable Tandem panel created.");
  console.log("");
  console.log("Next steps:");
  console.log(`  cd ${cdTarget}`);
  console.log("  npm install");
  console.log("  npm run dev");
  console.log("");
  console.log("Helpful commands:");
  console.log("  npm run init:env");
  console.log("  npm run build");
  console.log("  npm run start");
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
