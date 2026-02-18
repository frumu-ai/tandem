import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const LOCALES_ROOT = path.resolve("src", "i18n", "locales");
const BASE_LOCALE = "en";
const TARGET_LOCALE = "zh-CN";

function readJson(filePath) {
  const raw = fs.readFileSync(filePath, "utf8").replace(/^\uFEFF/, "");
  return JSON.parse(raw);
}

function collectLeafPaths(value, prefix = "", out = new Map()) {
  if (value === null || value === undefined) {
    out.set(prefix, value);
    return out;
  }

  if (typeof value !== "object" || Array.isArray(value)) {
    out.set(prefix, value);
    return out;
  }

  for (const [key, nested] of Object.entries(value)) {
    const next = prefix ? `${prefix}.${key}` : key;
    collectLeafPaths(nested, next, out);
  }
  return out;
}

function ensureDirExists(dirPath) {
  if (!fs.existsSync(dirPath)) {
    throw new Error(`Missing locale directory: ${dirPath}`);
  }
}

function listJsonFiles(dirPath) {
  return fs
    .readdirSync(dirPath, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort();
}

function main() {
  const baseDir = path.join(LOCALES_ROOT, BASE_LOCALE);
  const targetDir = path.join(LOCALES_ROOT, TARGET_LOCALE);
  ensureDirExists(baseDir);
  ensureDirExists(targetDir);

  const baseFiles = listJsonFiles(baseDir);
  const targetFiles = listJsonFiles(targetDir);
  const allFiles = new Set([...baseFiles, ...targetFiles]);

  const errors = [];

  for (const fileName of allFiles) {
    if (!baseFiles.includes(fileName)) {
      errors.push(`[${fileName}] Missing in ${BASE_LOCALE}`);
      continue;
    }
    if (!targetFiles.includes(fileName)) {
      errors.push(`[${fileName}] Missing in ${TARGET_LOCALE}`);
      continue;
    }

    const baseJson = readJson(path.join(baseDir, fileName));
    const targetJson = readJson(path.join(targetDir, fileName));
    const baseLeaves = collectLeafPaths(baseJson);
    const targetLeaves = collectLeafPaths(targetJson);

    for (const key of baseLeaves.keys()) {
      if (!targetLeaves.has(key)) {
        errors.push(`[${fileName}] Missing key in ${TARGET_LOCALE}: ${key}`);
      }
    }

    for (const key of targetLeaves.keys()) {
      if (!baseLeaves.has(key)) {
        errors.push(`[${fileName}] Extra key in ${TARGET_LOCALE}: ${key}`);
      }
    }

    for (const [key, value] of baseLeaves.entries()) {
      if (typeof value === "string" && value.trim().length === 0) {
        errors.push(`[${fileName}] Empty translation in ${BASE_LOCALE}: ${key}`);
      }
    }

    for (const [key, value] of targetLeaves.entries()) {
      if (typeof value === "string" && value.trim().length === 0) {
        errors.push(`[${fileName}] Empty translation in ${TARGET_LOCALE}: ${key}`);
      }
    }

    // Detect object/scalar shape conflicts for matching top-level keys.
    const topKeys = new Set([...Object.keys(baseJson), ...Object.keys(targetJson)]);
    for (const topKey of topKeys) {
      const a = baseJson[topKey];
      const b = targetJson[topKey];
      if (a === undefined || b === undefined) continue;
      const aIsObj = a !== null && typeof a === "object" && !Array.isArray(a);
      const bIsObj = b !== null && typeof b === "object" && !Array.isArray(b);
      if (aIsObj !== bIsObj) {
        errors.push(`[${fileName}] Type mismatch for key '${topKey}' between locales`);
      }
    }
  }

  if (errors.length > 0) {
    console.error("i18n parity check failed:");
    for (const error of errors) {
      console.error(`- ${error}`);
    }
    process.exit(1);
  }

  console.log("i18n parity check passed.");
}

main();
