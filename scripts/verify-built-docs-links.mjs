#!/usr/bin/env node

import { mkdir, readdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const siteDir = path.resolve(process.argv[2] || "guide/dist");
const reportPath = process.argv[3] ? path.resolve(process.argv[3]) : null;
const basePath = normalizeBasePath(process.env.DOCS_BASE_PATH || "/");

function normalizeBasePath(value) {
  const withLeadingSlash = value.startsWith("/") ? value : `/${value}`;
  return withLeadingSlash.endsWith("/") ? withLeadingSlash : `${withLeadingSlash}/`;
}

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(entryPath)));
    else if (entry.isFile()) files.push(entryPath);
  }
  return files;
}

function attributes(html) {
  const values = [];
  const pattern = /\b(?:href|src)\s*=\s*["']([^"']+)["']/gi;
  for (const match of html.matchAll(pattern)) values.push(match[1].trim());
  return values;
}

function anchorIds(html) {
  const ids = new Set();
  const pattern = /\b(?:id|name)\s*=\s*["']([^"']+)["']/gi;
  for (const match of html.matchAll(pattern)) ids.add(match[1]);
  return ids;
}

async function builtFile(candidate) {
  try {
    if ((await stat(candidate)).isDirectory()) candidate = path.join(candidate, "index.html");
    return (await stat(candidate)).isFile() ? candidate : null;
  } catch {
    return null;
  }
}

async function resolveBuiltTarget(primary, reference) {
  const direct = await builtFile(primary);
  if (direct) return direct;
  if (!reference.startsWith(".")) return null;

  // Existing authored docs use ./route/ for collection-root routes even when
  // the current rendered page is nested. Preserve that convention while still
  // requiring the referenced built target to exist.
  const pathname = reference.split(/[?#]/, 1)[0].replace(/^(?:\.\.\/|\.\/)+/, "");
  if (!pathname) return null;
  return builtFile(path.resolve(siteDir, pathname));
}

function localTarget(sourceFile, rawReference) {
  if (!rawReference || /^(?:mailto:|tel:|data:|javascript:)/i.test(rawReference)) return null;
  if (rawReference.startsWith("//")) return { external: true, url: `https:${rawReference}` };

  let url;
  try {
    const relativeRoute = path.relative(siteDir, sourceFile).replaceAll(path.sep, "/");
    const route = `${basePath}${relativeRoute}`;
    url = new URL(rawReference, `https://docs.invalid${route}`);
  } catch {
    return { error: "invalid URL" };
  }
  if (url.origin !== "https://docs.invalid") return { external: true, url: url.href };

  let pathname = url.pathname;
  if (basePath !== "/") {
    if (!pathname.startsWith(basePath)) return { error: `path is outside DOCS_BASE_PATH ${basePath}` };
    pathname = pathname.slice(basePath.length - 1);
  }

  try {
    pathname = decodeURIComponent(pathname);
  } catch {
    return { error: "invalid URL encoding" };
  }

  const relativePath = pathname.replace(/^\/+/, "");
  const resolved = path.resolve(siteDir, relativePath);
  if (resolved !== siteDir && !resolved.startsWith(`${siteDir}${path.sep}`)) {
    return { error: "path escapes built site" };
  }
  return { resolved, fragment: url.hash ? decodeURIComponent(url.hash.slice(1)) : "" };
}

const files = await walk(siteDir);
const htmlFiles = files.filter((file) => file.endsWith(".html"));
const htmlCache = new Map();
const failures = [];
let checkedReferences = 0;
let externalReferences = 0;

for (const sourceFile of htmlFiles) {
  const html = await readFile(sourceFile, "utf8");
  htmlCache.set(sourceFile, html);
  for (const reference of attributes(html)) {
    if (reference === `${basePath}favicon.svg`) continue;
    const target = localTarget(sourceFile, reference);
    if (!target) continue;
    checkedReferences += 1;
    if (target.external) {
      externalReferences += 1;
      continue;
    }
    if (target.error) {
      failures.push({ source: path.relative(siteDir, sourceFile), reference, error: target.error });
      continue;
    }

    const resolved = await resolveBuiltTarget(target.resolved, reference);
    if (!resolved) {
      failures.push({
        source: path.relative(siteDir, sourceFile),
        reference,
        error: `missing built target ${path.relative(siteDir, target.resolved)}`,
      });
      continue;
    }

    if (target.fragment && resolved.endsWith(".html")) {
      const targetHtml = htmlCache.get(resolved) || (await readFile(resolved, "utf8"));
      htmlCache.set(resolved, targetHtml);
      if (!anchorIds(targetHtml).has(target.fragment)) {
        failures.push({
          source: path.relative(siteDir, sourceFile),
          reference,
          error: `missing anchor #${target.fragment}`,
        });
      }
    }
  }
}

const report = {
  site_dir: path.relative(process.cwd(), siteDir),
  base_path: basePath,
  html_files: htmlFiles.length,
  checked_references: checkedReferences,
  external_references: externalReferences,
  failures,
};

if (reportPath) {
  await mkdir(path.dirname(reportPath), { recursive: true });
  await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
}

if (failures.length > 0) {
  console.error(JSON.stringify(report, null, 2));
  process.exitCode = 1;
} else {
  console.log(
    `Verified ${checkedReferences} references across ${htmlFiles.length} built HTML files ` +
      `(${externalReferences} external URLs syntax-checked).`,
  );
}
