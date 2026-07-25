#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ALLOWED_ENV_SUFFIXES = [".example", ".sample", ".template"];
const DANGEROUS_BASENAMES = new Set([
  ".npmrc",
  ".pypirc",
  "credentials.json",
  "service-account.json",
  "terraform.tfstate",
  "terraform.tfstate.backup",
  "id_rsa",
  "id_ed25519",
]);
const REVIEWED_PUBLIC_CONFIG_CONTENT = new Map([["packages/tandem-ai/.npmrc", "git-checks=false"]]);

export function isDangerousSecretPath(filename) {
  const normalized = filename.replaceAll("\\", "/");
  const basename = path.posix.basename(normalized).toLowerCase();
  if (basename === ".env" || basename.startsWith(".env.")) {
    if (ALLOWED_ENV_SUFFIXES.some((suffix) => basename.endsWith(suffix))) return false;
    return true;
  }
  if (DANGEROUS_BASENAMES.has(basename)) return true;
  if (/\.(?:pem|key|p12|pfx|jks|keystore)$/i.test(basename)) return true;
  return /(^|\/)(?:secret|secrets|credential|credentials)(\/|$)/i.test(normalized);
}

function git(args, root) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
}

export function verifySecretFileHistory(root = process.cwd()) {
  const refs = new Set(["HEAD"]);
  for (const ref of git(["for-each-ref", "--format=%(refname)"], root).split(/\r?\n/)) {
    if (ref.trim()) refs.add(ref.trim());
  }
  const violations = new Map();
  const pathOutput = git(
    [
      "-c",
      "core.quotePath=false",
      "log",
      "--all",
      "--full-history",
      "--root",
      "--diff-merges=first-parent",
      "--no-renames",
      "--format=",
      "--name-only",
      "-z",
    ],
    root
  );
  const historicalPaths = pathOutput.split("\0").map((value) => value.replace(/^\n+/, ""));
  const uniquePaths = new Set(historicalPaths.filter(Boolean));
  let blobsScanned = 0;
  for (const filename of uniquePaths) {
    if (!filename || !isDangerousSecretPath(filename)) continue;
    blobsScanned += 1;
    const allowedContent = REVIEWED_PUBLIC_CONFIG_CONTENT.get(filename);
    if (allowedContent !== undefined) {
      let allowed = true;
      const commits = git(["log", "--all", "--full-history", "--format=%H", "--", filename], root)
        .split(/\r?\n/)
        .filter(Boolean);
      for (const commit of commits) {
        const objectSpec = `${commit}:${filename}`;
        try {
          if (git(["cat-file", "-t", objectSpec], root).trim() !== "blob") continue;
          if (git(["cat-file", "-p", objectSpec], root).trim() !== allowedContent) {
            allowed = false;
            break;
          }
        } catch {
          // The path was deleted in this commit; an earlier revision is checked separately.
        }
      }
      if (allowed) continue;
    }
    if (!violations.has(filename)) violations.set(filename, []);
    violations.get(filename).push("historical-path");
  }
  return { blobsScanned, pathsScanned: uniquePaths.size, refsScanned: refs.size, violations };
}

function selfTest() {
  const dangerous = [".env", ".env.production", "secrets/token", "id_rsa", "tls/private.key"];
  const allowed = [".env.example", "docs/secrets.md", "src/credentials.ts", "cert/public.crt"];
  if (!dangerous.every(isDangerousSecretPath) || allowed.some(isDangerousSecretPath)) {
    throw new Error("secret-file path self-test failed");
  }
  const testRoot = mkdtempSync(path.join(os.tmpdir(), "tandem-secret-history-"));
  try {
    git(["init"], testRoot);
    mkdirSync(path.join(testRoot, "secrets"));
    writeFileSync(path.join(testRoot, "README"), "same public bytes\n");
    writeFileSync(path.join(testRoot, "secrets", "token"), "same public bytes\n");
    git(["add", "README", "secrets/token"], testRoot);
    git(
      [
        "-c",
        "user.name=Tandem Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-m",
        "fixture",
      ],
      testRoot
    );
    const result = verifySecretFileHistory(testRoot);
    if (!result.violations.has("secrets/token")) {
      throw new Error(
        "secret-file history self-test missed a dangerous path sharing an allowed blob"
      );
    }
  } finally {
    rmSync(testRoot, { recursive: true, force: true });
  }
}

function main() {
  if (process.argv.includes("--self-test")) selfTest();
  const result = verifySecretFileHistory();
  if (result.violations.size > 0) {
    const detail = [...result.violations.entries()]
      .map(([filename, refs]) => `${filename} (${refs.slice(0, 5).join(", ")})`)
      .join("\n");
    throw new Error(`tracked secret-file patterns found across refs:\n${detail}`);
  }
  process.stdout.write(
    `secret-file path policy passed (${result.refsScanned} refs, ${result.blobsScanned} sensitive historical paths inspected)\n`
  );
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
