#!/usr/bin/env node

import { randomBytes } from "node:crypto";
import {
  chmodSync,
  closeSync,
  constants,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const TOKEN_PATTERN = /^tk_[0-9a-f]{32}$/;

function assertRegularFile(pathname) {
  const metadata = lstatSync(pathname);
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    throw new Error(`Token path must be a regular file, not a link: ${pathname}`);
  }
}

export function readTokenFile(tokenPath) {
  assertRegularFile(tokenPath);
  const token = readFileSync(tokenPath, "utf8").trim();
  if (!TOKEN_PATTERN.test(token)) {
    throw new Error(`Token file is empty or has an invalid format: ${tokenPath}`);
  }
  return token;
}

export function ensureTokenFile({ cwd = process.cwd() } = {}) {
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

  if (existsSync(tokenPath)) {
    const token = readTokenFile(tokenPath);
    chmodSync(tokenPath, 0o600);
    return { created: false, token, tokenPath };
  }

  const token = `tk_${randomBytes(16).toString("hex")}`;
  const flags = constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | (constants.O_NOFOLLOW || 0);
  const descriptor = openSync(tokenPath, flags, 0o600);
  try {
    writeFileSync(descriptor, `${token}\n`, "utf8");
  } finally {
    closeSync(descriptor);
  }
  chmodSync(tokenPath, 0o600);
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
    process.stderr.write(`[tandem-control-panel] ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
