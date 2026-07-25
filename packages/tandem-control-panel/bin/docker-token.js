#!/usr/bin/env node

import { randomBytes } from "node:crypto";
import {
  chmodSync,
  closeSync,
  constants,
  existsSync,
  fchmodSync,
  fchownSync,
  fstatSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const TOKEN_PATTERN = /^tk_[0-9a-f]{32}$/;

function openRegularTokenFile(pathname, flags, { allowMissing = false } = {}) {
  let descriptor;
  try {
    descriptor = openSync(pathname, flags | (constants.O_NOFOLLOW || 0));
  } catch (error) {
    if (allowMissing && error?.code === "ENOENT") return null;
    throw new Error(
      `Token path must be a regular file, not a link: ${pathname} (${error?.code || "open failed"})`
    );
  }
  const metadata = fstatSync(descriptor);
  if (!metadata.isFile()) {
    closeSync(descriptor);
    throw new Error(`Token path must be a regular file, not a link: ${pathname}`);
  }
  return descriptor;
}

function readTokenDescriptor(descriptor, tokenPath) {
  const token = readFileSync(descriptor, "utf8").trim();
  if (!TOKEN_PATTERN.test(token)) {
    throw new Error(`Token file is empty or has an invalid format: ${tokenPath}`);
  }
  return token;
}

export function readTokenFile(tokenPath) {
  const descriptor = openRegularTokenFile(tokenPath, constants.O_RDONLY);
  try {
    return readTokenDescriptor(descriptor, tokenPath);
  } finally {
    closeSync(descriptor);
  }
}

function secureTokenDescriptor(descriptor, ownerUid, ownerGid) {
  if (
    process.platform !== "win32" &&
    Number.isInteger(ownerUid) &&
    Number.isInteger(ownerGid) &&
    typeof process.getuid === "function" &&
    process.getuid() === 0
  ) {
    fchownSync(descriptor, ownerUid, ownerGid);
  }
  fchmodSync(descriptor, 0o600);
}

function readExistingToken(tokenPath, ownerUid, ownerGid, { allowMissing = false } = {}) {
  const descriptor = openRegularTokenFile(tokenPath, constants.O_RDONLY, { allowMissing });
  if (descriptor === null) return null;
  try {
    secureTokenDescriptor(descriptor, ownerUid, ownerGid);
    return { created: false, token: readTokenDescriptor(descriptor, tokenPath), tokenPath };
  } finally {
    closeSync(descriptor);
  }
}

export function ensureTokenFile({ cwd = process.cwd(), ownerUid, ownerGid } = {}) {
  const tokenDirectory = resolve(cwd, "secrets");
  const tokenPath = resolve(tokenDirectory, "tandem_api_token");
  if (existsSync(tokenDirectory)) {
    const metadata = lstatSync(tokenDirectory);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      throw new Error(`Token directory must be a real directory, not a link: ${tokenDirectory}`);
    }
  } else {
    mkdirSync(tokenDirectory, { recursive: true, mode: 0o700 });
  }
  chmodSync(tokenDirectory, 0o700);

  const existing = readExistingToken(tokenPath, ownerUid, ownerGid, { allowMissing: true });
  if (existing) return existing;

  const token = `tk_${randomBytes(16).toString("hex")}`;
  const flags =
    constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | (constants.O_NOFOLLOW || 0);
  let descriptor;
  try {
    descriptor = openSync(tokenPath, flags, 0o600);
  } catch (error) {
    if (error?.code === "EEXIST") {
      return readExistingToken(tokenPath, ownerUid, ownerGid);
    }
    throw error;
  }
  try {
    writeFileSync(descriptor, `${token}\n`, "utf8");
    secureTokenDescriptor(descriptor, ownerUid, ownerGid);
  } finally {
    closeSync(descriptor);
  }
  return { created: true, token, tokenPath };
}

function main() {
  const ensure = process.argv.slice(2).includes("--ensure");
  const tokenPath = resolve(process.cwd(), "secrets", "tandem_api_token");
  const result = ensure ? ensureTokenFile() : { token: readTokenFile(tokenPath), tokenPath };
  if (ensure && result.created) {
    process.stderr.write(`[tandem-control-panel] Created ${result.tokenPath} with mode 0600.\n`);
  }
  process.stdout.write(`${result.token}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    main();
  } catch (error) {
    process.stderr.write(
      `[tandem-control-panel] ${error instanceof Error ? error.message : String(error)}\n`
    );
    process.exitCode = 1;
  }
}
