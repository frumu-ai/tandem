#!/usr/bin/env node

import { execFileSync } from "node:child_process";
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
const REVIEWED_PUBLIC_CONFIG_CONTENT = new Map([
  ["packages/tandem-ai/.npmrc", "git-checks=false"],
]);

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
  let blobsScanned = 0;
  const objects = git(["-c", "core.quotePath=false", "rev-list", "--objects", "--all"], root);
  for (const record of objects.split(/\r?\n/)) {
    const separator = record.indexOf(" ");
    if (separator <= 0) continue;
    const objectId = record.slice(0, separator);
    const filename = record.slice(separator + 1);
    if (!filename || !isDangerousSecretPath(filename)) continue;
    if (git(["cat-file", "-t", objectId], root).trim() !== "blob") continue;
    blobsScanned += 1;
    const allowedContent = REVIEWED_PUBLIC_CONFIG_CONTENT.get(filename);
    if (allowedContent !== undefined) {
      const content = git(["cat-file", "-p", objectId], root).trim();
      if (content === allowedContent) continue;
    }
    if (!violations.has(filename)) violations.set(filename, []);
    violations.get(filename).push(objectId);
  }
  return { blobsScanned, refsScanned: refs.size, violations };
}

function selfTest() {
  const dangerous = [".env", ".env.production", "secrets/token", "id_rsa", "tls/private.key"];
  const allowed = [".env.example", "docs/secrets.md", "src/credentials.ts", "cert/public.crt"];
  if (!dangerous.every(isDangerousSecretPath) || allowed.some(isDangerousSecretPath)) {
    throw new Error("secret-file path self-test failed");
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
    `secret-file path policy passed (${result.refsScanned} refs, ${result.blobsScanned} sensitive-path blobs inspected)\n`
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
