import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import test from "node:test";
import * as lucide from "lucide";

// Guard against the recurring "blank icon" class of bug: every `<Icon name="...">`
// name used in the source must be both (a) registered in src/ui/Icon.tsx and
// (b) a real export of the installed `lucide` package. An unregistered name
// resolves to `undefined` in the ICONS map, and the Icon component renders an
// empty placeholder svg — so without this check a typo or a missing registry
// entry used to ship as an invisible icon (see TAN-576, TAN-578). The typed
// registry now rejects unknown names, while this test verifies source coverage.

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "..", "src");

// lucide resolves an attribute value (kebab-case) to a PascalCase export.
const toPascalCase = (value) =>
  value.replace(/(\w)(\w*)(_|-|\s*)/g, (_all, first, rest) => first.toUpperCase() + rest.toLowerCase());

function registeredNames() {
  const source = readFileSync(join(srcDir, "ui", "Icon.tsx"), "utf8");
  const match = source.match(/const ICONS = \{([\s\S]*?)\} as const/);
  assert.ok(match, "could not locate the ICONS registry object in src/ui/Icon.tsx");
  const names = new Set();
  for (const m of match[1].matchAll(/"([a-z0-9-]+)":/g)) names.add(m[1]);
  return names;
}

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules" && entry.name !== "generated") walk(full, out);
    } else if (/\.(tsx?|jsx?|html)$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

// Collect every kebab-case name used as an <Icon name="..."> / name={...}
// prop, including dynamic expressions (`name={cond ? "a" : "b"}`) and
// icon-config fields (`icon: "a"`) that feed into <Icon name={cfg.icon}>.
function usedIconNames() {
  const used = new Map(); // name -> Set(relative file)
  const add = (name, file) => {
    if (!/^[a-z][a-z0-9]*(-[a-z0-9]+)*$/.test(name)) return;
    if (!used.has(name)) used.set(name, new Set());
    used.get(name).add(file.replace(`${srcDir}/`, ""));
  };
  for (const file of walk(srcDir)) {
    if (file.endsWith(join("ui", "Icon.tsx"))) continue;
    const source = readFileSync(file, "utf8");
    for (const m of source.matchAll(/<Icon\b[^>]*?\bname\s*=\s*"([a-z0-9-]+)"/gs)) add(m[1], file);
    for (const m of source.matchAll(/<Icon\b[^>]*?\bname\s*=\s*\{([^}]*)\}/gs)) {
      for (const q of m[1].matchAll(/["'`]([a-z0-9-]+)["'`]/g)) add(q[1], file);
    }
    for (const m of source.matchAll(/\b(?:icon|confirmIcon)\s*[:=]\s*["'`]([a-z0-9-]+)["'`]/g)) add(m[1], file);
  }
  return used;
}

test("every <Icon name> value is registered and exists in lucide", () => {
  const registered = registeredNames();
  const used = usedIconNames();
  const problems = [];
  for (const [name, files] of used) {
    const pascal = toPascalCase(name);
    const inLucide = pascal in lucide;
    const inRegistry = registered.has(name);
    // Names that are neither a real lucide icon nor registered are almost always
    // string fragments extracted from dynamic expressions (e.g. a status value),
    // not actual icon usages — skip those to avoid false positives.
    if (!inLucide) continue;
    if (!inRegistry) {
      problems.push(`  "${name}" (${pascal}) used in ${[...files].join(", ")} is a lucide icon but is NOT registered in src/ui/Icon.tsx`);
    }
  }
  assert.equal(
    problems.length,
    0,
    `Unregistered lucide icons will render blank. Register them in src/ui/Icon.tsx:\n${problems.join("\n")}`,
  );
});
