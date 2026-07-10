#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const sourceRoots = ["crates", "engine"]
  .map((directory) => path.join(root, directory))
  .filter((directory) => fs.existsSync(directory));
const legacyMethods = [
  "default_complete",
  "complete_for_provider",
  "complete_cheapest",
  "default_stream",
  "stream_for_provider",
];
const rawProviderMethods = [
  "complete",
  "complete_with_auth_override",
  "stream",
  "stream_with_auth_override",
];
const legacyAlternation = legacyMethods.join("|");
const rawAlternation = rawProviderMethods.join("|");
const methodAlternation = [...legacyMethods, ...rawProviderMethods]
  .sort((left, right) => right.length - left.length)
  .join("|");
const canonicalCalls = {
  "crates/tandem-providers/src/guarded_dispatch.rs": {
    allowed: {
      complete_with_egress_permit: ["complete_for_provider"],
      stream_with_egress_permit: ["stream_for_provider"],
    },
    expected: { complete_for_provider: 1, stream_for_provider: 1 },
  },
  "crates/tandem-providers/src/lib_parts/part01.rs": {
    allowed: {
      complete_with_auth_override: ["complete"],
      stream: ["complete"],
      stream_with_auth_override: ["stream"],
      default_complete: ["complete_for_provider"],
      complete_for_provider: ["complete_with_auth_override"],
      complete_cheapest: ["complete_for_provider"],
      default_stream: ["stream_for_provider"],
      stream_for_provider: ["stream_with_auth_override"],
    },
    expected: {
      complete: 2,
      stream: 1,
      complete_for_provider: 2,
      stream_for_provider: 1,
      complete_with_auth_override: 2,
      stream_with_auth_override: 2,
    },
  },
  "crates/tandem-providers/src/lib_parts/part02.rs": {
    allowed: {
      complete_with_auth_override: ["complete"],
      stream: ["stream_with_auth_override"],
    },
    expected: { complete: 2, stream_with_auth_override: 1 },
  },
};

function maskRustTrivia(source) {
  const output = source.split("");
  const mask = (index) => {
    if (output[index] !== "\n") output[index] = " ";
  };

  for (let index = 0; index < source.length; index += 1) {
    const current = source[index];
    const next = source[index + 1];
    if (current === "/" && next === "/") {
      while (index < source.length && source[index] !== "\n") {
        mask(index);
        index += 1;
      }
      index -= 1;
      continue;
    }
    if (current === "/" && next === "*") {
      let depth = 1;
      mask(index);
      mask(index + 1);
      index += 2;
      while (index < source.length && depth > 0) {
        if (source[index] === "/" && source[index + 1] === "*") {
          depth += 1;
          mask(index);
          mask(index + 1);
          index += 2;
        } else if (source[index] === "*" && source[index + 1] === "/") {
          depth -= 1;
          mask(index);
          mask(index + 1);
          index += 2;
        } else {
          mask(index);
          index += 1;
        }
      }
      index -= 1;
      continue;
    }

    const raw = /^(?:br|r)(#+)?"/.exec(source.slice(index));
    if (raw) {
      const hashes = raw[1] ?? "";
      const terminator = `"${hashes}`;
      const close = source.indexOf(terminator, index + raw[0].length);
      const end = close === -1 ? source.length : close + terminator.length;
      while (index < end) {
        mask(index);
        index += 1;
      }
      index -= 1;
      continue;
    }
    if (current === '"' || (current === "b" && next === '"')) {
      if (current === "b") {
        mask(index);
        index += 1;
      }
      mask(index);
      for (index += 1; index < source.length; index += 1) {
        mask(index);
        if (source[index] === "\\") {
          index += 1;
          mask(index);
        } else if (source[index] === '"') {
          break;
        }
      }
      continue;
    }
    if (current === "'" && source.indexOf("'", index + 1) - index <= 8) {
      const close = source.indexOf("'", index + 1);
      if (close !== -1) {
        while (index <= close) {
          mask(index);
          index += 1;
        }
        index -= 1;
      }
    }
  }
  return output.join("");
}

function matchingRustBrace(source, openIndex) {
  let depth = 0;
  let blockCommentDepth = 0;
  for (let index = openIndex; index < source.length; index += 1) {
    const current = source[index];
    const next = source[index + 1];
    if (blockCommentDepth > 0) {
      if (current === "/" && next === "*") {
        blockCommentDepth += 1;
        index += 1;
      } else if (current === "*" && next === "/") {
        blockCommentDepth -= 1;
        index += 1;
      }
      continue;
    }
    if (current === "/" && next === "*") {
      blockCommentDepth = 1;
      index += 1;
      continue;
    }
    if (current === "/" && next === "/") {
      index = source.indexOf("\n", index + 2);
      if (index === -1) return -1;
      continue;
    }
    if (current === '"') {
      for (index += 1; index < source.length; index += 1) {
        if (source[index] === "\\") index += 1;
        else if (source[index] === '"') break;
      }
      continue;
    }
    if (current === "'") {
      const close = source.indexOf("'", index + 1);
      if (close !== -1 && close - index <= 8) {
        index = close;
        continue;
      }
    }
    if (current === "r" && (next === '"' || next === "#")) {
      const raw = /^r(#+)?"/.exec(source.slice(index));
      if (raw) {
        const hashes = raw[1] ?? "";
        const terminator = `"${hashes}`;
        const close = source.indexOf(terminator, index + raw[0].length);
        if (close === -1) return -1;
        index = close + terminator.length - 1;
        continue;
      }
    }
    if (current === "{") depth += 1;
    if (current === "}") {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

function productionSource(source) {
  // Mask only balanced inline test modules. Keeping line breaks and all source
  // after the module means a production bypass cannot hide behind test code.
  const testModule = /#\[cfg\s*\(\s*test\s*\)\s*\]\s*(?:pub\s+)?mod\s+[A-Za-z0-9_]+\s*\{/g;
  const searchable = maskRustTrivia(source);
  let masked = searchable;
  for (const match of searchable.matchAll(testModule)) {
    const open = searchable.indexOf("{", match.index);
    const close = matchingRustBrace(source, open);
    if (close === -1) continue;
    const body = searchable
      .slice(match.index, close + 1)
      .replace(/[^\n]/g, " ");
    masked = `${masked.slice(0, match.index)}${body}${masked.slice(close + 1)}`;
  }
  return masked;
}

function functionRanges(source) {
  const searchable = productionSource(source);
  const declaration =
    /\b(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b[^;{]*\{/g;
  const ranges = [];
  for (const match of searchable.matchAll(declaration)) {
    const open = searchable.indexOf("{", match.index);
    const close = matchingRustBrace(source, open);
    if (close !== -1) ranges.push({ name: match[1], start: match.index, end: close });
  }
  return ranges;
}

function enclosingFunction(ranges, index) {
  return ranges
    .filter((range) => range.start <= index && index <= range.end)
    .sort((left, right) => left.end - left.start - (right.end - right.start))[0]?.name;
}

function dispatchViolations(source, relative = "crates/example/src/lib.rs") {
  const production = productionSource(source);
  const patterns = [];
  patterns.push(
    new RegExp(`\\.\\s*(?:${legacyAlternation})\\s*\\(`, "g"),
    new RegExp(`::\\s*(?:${legacyAlternation})\\b`, "g"),
    new RegExp(`\\buse\\b[^;]*\\b(?:${legacyAlternation})\\b[^;]*;`, "g"),
  );
  const providerTraitNames = new Set(["Provider"]);
  for (const match of production.matchAll(/\bProvider\s+as\s+([A-Za-z_][A-Za-z0-9_]*)/g)) {
    providerTraitNames.add(match[1]);
  }
  const providerTraits = [...providerTraitNames].join("|");
  patterns.push(
    new RegExp(`\\.\\s*(?:${rawAlternation})\\s*\\(`, "g"),
    new RegExp(
      `(?:\\b(?:${providerTraits})|<[^>\\n]*\\b(?:${providerTraits})\\s*>)\\s*::\\s*(?:${rawAlternation})\\b`,
      "g",
    ),
    new RegExp(
      `\\buse\\b[^;]*\\b(?:${providerTraits})\\s*::\\s*(?:\\{[^;]*\\b)?(?:${rawAlternation})\\b[^;]*;`,
      "g",
    ),
  );

  const violations = [];
  const indexes = new Set();
  for (const pattern of patterns) {
    for (const match of production.matchAll(pattern)) {
      if (indexes.has(match.index)) continue;
      indexes.add(match.index);
      const line = source.slice(0, match.index).split("\n").length;
      const call = match[0].replace(/\s+/g, " ").trim();
      const method = call.match(
        new RegExp(`(?:${methodAlternation})`),
      )?.[0];
      violations.push({ index: match.index, line, call, method });
    }
  }
  const canonical = canonicalCalls[relative];
  if (!canonical) return violations.sort((left, right) => left.line - right.line);

  const ranges = functionRanges(source);
  const observed = {};
  const failures = [];
  for (const violation of violations) {
    const functionName = enclosingFunction(ranges, violation.index);
    const allowedMethods = canonical.allowed[functionName] ?? [];
    if (!violation.method || !allowedMethods.includes(violation.method)) {
      failures.push({
        ...violation,
        call: `${violation.call} inside ${functionName ?? "no function"}`,
      });
      continue;
    }
    observed[violation.method] = (observed[violation.method] ?? 0) + 1;
  }
  for (const [method, expected] of Object.entries(canonical.expected)) {
    const actual = observed[method] ?? 0;
    if (actual !== expected) {
      failures.push({
        index: 0,
        line: 1,
        call: `${method} canonical call count ${actual}; expected ${expected}`,
        method,
      });
    }
  }
  return failures.sort((left, right) => left.line - right.line);
}

function runSelfTest() {
  const guarded = `registry.stream_with_egress_permit(&permit, Some("openai"), None);`;
  const testOnly = `${guarded}\n#[cfg(test)]\nmod tests {\n  fn direct() { registry.complete_for_provider(None, "x", None); }\n}`;
  const bypassAfterTest = `${testOnly}\nfn production() { registry.complete_for_provider(None, "x", None); }`;
  if (dispatchViolations(guarded).length !== 0) {
    throw new Error("guard self-test rejected the canonical permit API");
  }
  if (dispatchViolations(testOnly).length !== 0) {
    throw new Error("guard self-test treated an inline test module as production");
  }
  if (dispatchViolations(bypassAfterTest).length !== 1) {
    throw new Error("guard self-test missed a bypass after an inline test module");
  }

  const mutations = [
    ["legacy method call", `registry.complete_for_provider(None, "x", None);`],
    [
      "legacy UFCS call",
      `ProviderRegistry::complete_for_provider(&registry, None, "x", None);`,
    ],
    [
      "imported legacy alias",
      `use tandem_providers::ProviderRegistry::complete_for_provider as dispatch;\ndispatch(&registry, None, "x", None);`,
    ],
    ["raw trait method call", `provider.complete("x", None);`],
    ["raw trait UFCS call", `Provider::complete(&provider, "x", None);`],
    [
      "imported trait alias",
      `use tandem_providers::Provider as RawProvider;\nRawProvider::stream(&provider, messages, None, mode, None, sampling, cancel);`,
    ],
  ];
  for (const [name, mutation] of mutations) {
    if (dispatchViolations(mutation).length === 0) {
      throw new Error(`guard self-test did not detect ${name}`);
    }
  }
  const providerBypass = `registry.default_stream(messages, mode, None, cancel);`;
  if (
    dispatchViolations(
      providerBypass,
      "crates/tandem-providers/src/lib_parts/part03.rs",
    ).length === 0
  ) {
    throw new Error("guard self-test missed a provider-crate bypass");
  }
  const canonicalWrappers = `
    async fn complete_with_egress_permit(&self) {
      self.complete_for_provider(None, "x", None).await;
    }
    async fn stream_with_egress_permit(&self) {
      self.stream_for_provider(None, None, messages).await;
    }`;
  const guardedPath = "crates/tandem-providers/src/guarded_dispatch.rs";
  if (dispatchViolations(canonicalWrappers, guardedPath).length !== 0) {
    throw new Error("guard self-test rejected the exact canonical wrappers");
  }
  const duplicateInsideExemptFile = canonicalWrappers.replace(
    "self.complete_for_provider(None, \"x\", None).await;",
    "self.complete_for_provider(None, \"x\", None).await; self.complete_for_provider(None, \"y\", None).await;",
  );
  if (dispatchViolations(duplicateInsideExemptFile, guardedPath).length === 0) {
    throw new Error("guard self-test missed a second dispatch inside an implementation file");
  }
  console.log("Provider egress boundary guard self-test passed.");
}

function* rustFiles(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      yield* rustFiles(absolute);
    } else if (entry.isFile() && entry.name.endsWith(".rs")) {
      yield absolute;
    }
  }
}

if (process.argv.includes("--self-test")) {
  runSelfTest();
  process.exit(0);
}

const failures = [];
for (const sourceRoot of sourceRoots) {
  for (const absolute of rustFiles(sourceRoot)) {
    const relative = path.relative(root, absolute).split(path.sep).join("/");
    const source = fs.readFileSync(absolute, "utf8");
    for (const violation of dispatchViolations(source, relative)) {
      failures.push(`${relative}:${violation.line}: unguarded ${violation.call}`);
    }
  }
}

if (failures.length > 0) {
  console.error("Provider egress boundary guard failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  console.error(
    "Production provider dispatches must use complete_with_egress_permit or stream_with_egress_permit.",
  );
  process.exit(1);
}

console.log("Provider egress boundary guard verified: no production bypasses found.");
