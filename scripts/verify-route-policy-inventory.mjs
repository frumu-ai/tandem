#!/usr/bin/env node

import fs from "node:fs";
import { isIP } from "node:net";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const inventoryPath = path.join(repoRoot, "docs", "SECURITY_ROUTE_POLICY_INVENTORY.json");
const methodNames = ["delete", "get", "head", "options", "patch", "post", "put", "trace"];
const runtimeAuthenticatedReadPaths = new Set([
  "/config/providers",
  "/enterprise/status",
  "/global/config",
  "/global/storage/files",
  "/global/workspace",
]);

function lineNumber(source, index) {
  return source.slice(0, index).split("\n").length;
}

function maskRangePreservingLines(source, start, end) {
  return `${source.slice(0, start)}${source
    .slice(start, end)
    .replace(/[^\n]/g, " ")}${source.slice(end)}`;
}

function matchingDelimiter(source, openIndex, open = "(", close = ")") {
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
      const newline = source.indexOf("\n", index + 2);
      if (newline === -1) return -1;
      index = newline;
      continue;
    }
    if (current === '"') {
      for (index += 1; index < source.length; index += 1) {
        if (source[index] === "\\") index += 1;
        else if (source[index] === '"') break;
      }
      continue;
    }
    if (current === "r" && (next === '"' || next === "#")) {
      const raw = /^r(#+)?"/.exec(source.slice(index));
      if (raw) {
        const terminator = `"${raw[1] ?? ""}`;
        const rawClose = source.indexOf(terminator, index + raw[0].length);
        if (rawClose === -1) return -1;
        index = rawClose + terminator.length - 1;
        continue;
      }
    }
    if (current === open) depth += 1;
    if (current === close) {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

function productionSource(source) {
  let result = source;
  const marker = /#\[cfg\s*\(\s*test\s*\)\s*\]\s*(?:pub\s+)?mod\s+[A-Za-z0-9_]+\s*\{/g;
  for (const match of [...source.matchAll(marker)].reverse()) {
    const open = source.indexOf("{", match.index);
    const close = matchingDelimiter(source, open, "{", "}");
    if (close !== -1) result = maskRangePreservingLines(result, match.index, close + 1);
  }
  return result;
}

function splitTopLevelArguments(source) {
  const argumentsList = [];
  let start = 0;
  let parens = 0;
  let braces = 0;
  let brackets = 0;
  let blockCommentDepth = 0;
  for (let index = 0; index < source.length; index += 1) {
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
      const newline = source.indexOf("\n", index + 2);
      if (newline === -1) break;
      index = newline;
      continue;
    }
    if (current === '"') {
      for (index += 1; index < source.length; index += 1) {
        if (source[index] === "\\") index += 1;
        else if (source[index] === '"') break;
      }
      continue;
    }
    if (current === "(") parens += 1;
    else if (current === ")") parens -= 1;
    else if (current === "{") braces += 1;
    else if (current === "}") braces -= 1;
    else if (current === "[") brackets += 1;
    else if (current === "]") brackets -= 1;
    else if (current === "," && parens === 0 && braces === 0 && brackets === 0) {
      argumentsList.push(source.slice(start, index).trim());
      start = index + 1;
    }
  }
  argumentsList.push(source.slice(start).trim());
  return argumentsList;
}

function rustStringLiteral(value) {
  const match = /^"((?:\\.|[^"\\])*)"$/.exec(value.trim());
  if (!match) return null;
  return JSON.parse(`"${match[1]}"`);
}

function methodsFromRouterExpression(expression) {
  const methods = new Set();
  const matcher = new RegExp(`(?:^|[\\.:])(${methodNames.join("|")})\\s*\\(`, "g");
  for (const match of expression.matchAll(matcher)) methods.add(match[1].toUpperCase());
  // Axum dispatches every GET handler for HEAD and strips the response body.
  // Inventory that implicit surface rather than recording only source tokens.
  if (methods.has("GET")) methods.add("HEAD");
  return [...methods].sort();
}

function callsNamed(source, token) {
  const calls = [];
  let cursor = 0;
  while (cursor < source.length) {
    const index = source.indexOf(token, cursor);
    if (index === -1) break;
    const open = index + token.length - 1;
    const close = matchingDelimiter(source, open);
    if (close === -1) throw new Error(`unterminated ${token} call at line ${lineNumber(source, index)}`);
    calls.push({ index, body: source.slice(open + 1, close) });
    cursor = close + 1;
  }
  return calls;
}

function callIsInsideNamedFunction(source, callIndex, functionName) {
  const matches = [...source.matchAll(new RegExp(`\\bfn\\s+${functionName}\\s*\\(`, "g"))];
  if (matches.length !== 1) return false;
  const signatureOpen = source.indexOf("(", matches[0].index);
  const signatureClose = matchingDelimiter(source, signatureOpen);
  if (signatureClose === -1) return false;
  const bodyOpen = source.indexOf("{", signatureClose + 1);
  if (bodyOpen === -1) return false;
  const bodyClose = matchingDelimiter(source, bodyOpen, "{", "}");
  return bodyClose !== -1 && callIndex > bodyOpen && callIndex < bodyClose;
}

export function extractRoutesFromRust(source, sourcePath = "fixture.rs") {
  const production = productionSource(source);
  const routes = [];
  const unsupported = [];
  for (const call of callsNamed(production, ".route(")) {
    const args = splitTopLevelArguments(call.body);
    const routePath = args.length >= 2 ? rustStringLiteral(args[0]) : null;
    if (!routePath) {
      const isCanonicalIncidentHelperRegistration =
        /routes_incident_monitor\.rs$/.test(sourcePath) &&
        args[0]?.trim() === "&path" &&
        args[1]?.trim() === "method_router" &&
        callIsInsideNamedFunction(production, call.index, "route_prefixed");
      if (isCanonicalIncidentHelperRegistration) continue;
      unsupported.push(`${sourcePath}:${lineNumber(production, call.index)} dynamic route path ${args[0] ?? "<missing>"}`);
      continue;
    }
    const methods = methodsFromRouterExpression(args.slice(1).join(","));
    if (methods.length === 0) {
      unsupported.push(`${sourcePath}:${lineNumber(production, call.index)} route has no statically visible method`);
      continue;
    }
    for (const method of methods) {
      routes.push({ method, path: routePath, source: `${sourcePath}:${lineNumber(production, call.index)}` });
    }
  }

  if (/routes_incident_monitor\.rs$/.test(sourcePath)) {
    for (const call of callsNamed(production, "route_prefixed(")) {
      const beforeCall = production.slice(Math.max(0, call.index - 16), call.index);
      if (/\bfn\s*$/.test(beforeCall)) continue;
      const args = splitTopLevelArguments(call.body);
      const suffix = args.length >= 4 ? rustStringLiteral(args[2]) : null;
      if (!suffix) {
        unsupported.push(
          `${sourcePath}:${lineNumber(production, call.index)} dynamic incident-monitor suffix ${args[2] ?? "<missing>"}`,
        );
        continue;
      }
      const methods = methodsFromRouterExpression(args.slice(3).join(","));
      if (methods.length === 0) {
        unsupported.push(`${sourcePath}:${lineNumber(production, call.index)} prefixed route has no statically visible method`);
        continue;
      }
      for (const method of methods) {
        routes.push({ method, path: `/incident-monitor${suffix}`, source: `${sourcePath}:${lineNumber(production, call.index)}` });
      }
    }
  }
  return { routes, unsupported };
}

function routeFiles() {
  const serverRoutes = fs
    .readdirSync(path.join(repoRoot, "crates", "tandem-server", "src", "http"))
    .filter((name) => /^routes_.*\.rs$/.test(name))
    .map((name) => path.join("crates", "tandem-server", "src", "http", name));
  const enterpriseRoutes = fs
    .readdirSync(path.join(repoRoot, "crates", "tandem-enterprise-server", "src", "http"))
    .filter((name) => /^routes_enterprise.*\.rs$/.test(name))
    .map((name) => path.join("crates", "tandem-enterprise-server", "src", "http", name));
  return [path.join("crates", "tandem-server", "src", "http", "router.rs"), ...serverRoutes, ...enterpriseRoutes]
    .filter((relativePath) => fs.existsSync(path.join(repoRoot, relativePath)))
    .sort();
}

function walkRustFiles(relativeDirectory) {
  const absoluteDirectory = path.join(repoRoot, relativeDirectory);
  const result = [];
  for (const entry of fs.readdirSync(absoluteDirectory, { withFileTypes: true })) {
    const relativePath = path.join(relativeDirectory, entry.name);
    if (entry.isDirectory()) result.push(...walkRustFiles(relativePath));
    else if (entry.isFile() && entry.name.endsWith(".rs")) result.push(relativePath);
  }
  return result;
}

function webUiRoutes() {
  const sourcePath = path.join("crates", "tandem-server", "src", "webui", "mod.rs");
  const source = fs.readFileSync(path.join(repoRoot, sourcePath), "utf8");
  const registrations = [
    [".route(&base, get(serve_index))", "{web_ui_prefix}"],
    [".route(&format!(\"{}/\", base), get(serve_index))", "{web_ui_prefix}/"],
    [".route(&wildcard, get(serve_index))", "{web_ui_prefix}/{*path}"],
  ];
  return registrations.flatMap(([registration, routePath]) => {
    const index = source.indexOf(registration);
    if (index === -1) {
      throw new Error(`web UI route registration drifted: ${registration}`);
    }
    const sourceLocation = `${sourcePath}:${lineNumber(source, index)}`;
    return ["GET", "HEAD"].map((method) => ({
      method,
      path: routePath,
      source: sourceLocation,
      listener: "engine_api",
    }));
  });
}

function loopbackOAuthRoutes() {
  const sourcePath = path.join(
    "crates",
    "tandem-server",
    "src",
    "http",
    "config_providers_parts",
    "part02.rs",
  );
  const source = fs.readFileSync(path.join(repoRoot, sourcePath), "utf8");
  if (!source.includes("TcpListener::bind(OPENAI_CODEX_LOCAL_CALLBACK_ADDR)")) {
    throw new Error("loopback OAuth callback listener binding drifted");
  }
  const constantSourcePath = path.join(
    "crates",
    "tandem-server",
    "src",
    "http",
    "config_providers_parts",
    "part01.rs",
  );
  assertLoopbackCallbackAddress(
    fs.readFileSync(path.join(repoRoot, constantSourcePath), "utf8"),
  );
  const extracted = extractRoutesFromRust(source, sourcePath);
  if (extracted.unsupported.length > 0) {
    throw new Error(`loopback OAuth route cannot be inventoried:\n${extracted.unsupported.join("\n")}`);
  }
  return extracted.routes.map((route) => ({
    ...route,
    path: `{loopback_oauth_listener}${route.path}`,
    listener: "loopback_oauth_callback",
  }));
}

export function assertLoopbackCallbackAddress(source) {
  const match =
    /const\s+OPENAI_CODEX_LOCAL_CALLBACK_ADDR\s*:\s*&str\s*=\s*"([^"]+)"\s*;/.exec(
      source,
    );
  const socket = match ? /^([^:]+):(\d+)$/.exec(match[1]) : null;
  const host = socket?.[1] ?? "";
  const port = Number(socket?.[2]);
  if (
    !socket ||
    isIP(host) !== 4 ||
    Number(host.split(".")[0]) !== 127 ||
    !Number.isSafeInteger(port) ||
    port < 1 ||
    port > 65_535
  ) {
    throw new Error("OpenAI Codex OAuth callback address is not statically loopback-only");
  }
}

function assertProductionRouteCoverage(files) {
  const allowed = new Set([
    ...files,
    path.join("crates", "tandem-server", "src", "webui", "mod.rs"),
    path.join(
      "crates",
      "tandem-server",
      "src",
      "http",
      "config_providers_parts",
      "part02.rs",
    ),
  ]);
  const explicitExclusions = new Set([
    // Feature-gated, loopback-only deterministic Slack API used by the ACME demo.
    path.join("crates", "tandem-server", "src", "acme_demo", "live.rs"),
  ]);
  const roots = [
    path.join("crates", "tandem-server", "src"),
    path.join("crates", "tandem-enterprise-server", "src"),
  ];
  const untracked = [];
  for (const sourcePath of roots.flatMap(walkRustFiles)) {
    if (sourcePath.includes(`${path.sep}tests${path.sep}`)) continue;
    const source = fs.readFileSync(path.join(repoRoot, sourcePath), "utf8");
    if (!productionSource(source).includes(".route(")) continue;
    if (!allowed.has(sourcePath) && !explicitExclusions.has(sourcePath)) untracked.push(sourcePath);
  }
  if (untracked.length > 0) {
    throw new Error(`production Axum registrars are outside the route inventory:\n${untracked.join("\n")}`);
  }
}

export function classifyRoute(route) {
  const { method, path: routePath } = route;
  if (route.listener === "loopback_oauth_callback") {
    return {
      ingress_policy: "loopback_listener",
      authorization_policy: "oauth_state",
      capability: null,
      resolver: "pending_provider_oauth_session",
      policy_origin: "oauth.loopback_callback",
    };
  }
  if (routePath.startsWith("{web_ui_prefix}")) {
    return {
      ingress_policy: "public_web_ui_get_head",
      authorization_policy: "static_admin_shell_only",
      capability: null,
      resolver: "configured_reserved_web_ui_prefix",
      policy_origin: "public.web_ui",
    };
  }
  if ((method === "GET" || method === "HEAD") && routePath === "/global/health") {
    return {
      ingress_policy: "public_exact_health",
      authorization_policy: "minimal_health_shape",
      capability: null,
      resolver: "none",
      policy_origin: "public.health",
    };
  }
  if (
    (method === "GET" || method === "HEAD") &&
    /^\/audit\/(?:protected|stream|data-boundary\/monitoring|ledger\/(?:manifest|export))$/.test(routePath)
  ) {
    return {
      ingress_policy: "runtime_auth_gate",
      authorization_policy: "api_token_or_control_panel_admin_source_and_tenant_scope",
      capability: null,
      resolver: "request_principal_source_and_tenant_audit_ledger",
      policy_origin: "audit.admin",
    };
  }
  if (method === "PATCH" && /^\/config(?:\/identity)?$/.test(routePath)) {
    return {
      ingress_policy: "runtime_auth_gate",
      authorization_policy: "deployment_administrator_or_loopback_local_owner",
      capability: "deployment.config.manage",
      resolver: "tenant_project_config",
      policy_origin: "admin.deployment",
    };
  }
  if (method === "POST" && routePath === "/channels/slack/events") {
    return {
      ingress_policy: "public_signed_webhook",
      authorization_policy: "slack_signature_installation_and_tenant_binding",
      capability: "channel.message.ingress",
      resolver: "slack_signature_installation_sender_tenant",
      policy_origin: "public.slack_events",
    };
  }
  if (
    method === "POST" &&
    (/^\/webhooks\/automations\//.test(routePath) ||
      /^\/api\/engine\/webhooks\/automations\//.test(routePath))
  ) {
    return {
      ingress_policy: "public_capability_webhook",
      authorization_policy: "path_capability_signature_nonce_and_tenant_binding",
      capability: "automation.webhook.trigger",
      resolver: "automation_trigger_from_public_capability",
      policy_origin: "public.automation_webhook",
    };
  }
  if (/^\/(?:mcp\/[^/]+\/auth\/callback|provider\/[^/]+\/oauth\/callback)$/.test(routePath)) {
    return {
      ingress_policy: "public_oauth_callback",
      authorization_policy: "oauth_state",
      capability: null,
      resolver: "pending_oauth_session",
      policy_origin: "public.oauth_callback",
    };
  }
  if (method === "POST" && routePath === "/incident-monitor/intake/report") {
    return {
      ingress_policy: "runtime_mode_incident_intake",
      authorization_policy: "incident_intake_key_or_verified_tenant",
      capability: "incident.report.ingest",
      resolver: "incident_intake_key_or_context",
      policy_origin: "public.incident_intake",
    };
  }

  if (
    (method === "GET" || method === "HEAD") &&
    runtimeAuthenticatedReadPaths.has(routePath)
  ) {
    return {
      ingress_policy: "runtime_auth_gate",
      authorization_policy: "verified_tenant_context",
      capability: null,
      resolver: "verified_tenant_context",
      policy_origin: "tenant.authenticated",
    };
  }

  if (
    method === "POST" &&
    /^\/channels\/(?:slack|discord|telegram)\/interactions$/.test(routePath)
  ) {
    return {
      ingress_policy: "runtime_auth_gate",
      authorization_policy: "channel_signature_identity_and_tenant_binding",
      capability: "channel.interaction",
      resolver: "signed_channel_installation_user_tenant",
      policy_origin: "channel.signed_interaction",
    };
  }

  if (
    /^(?:\/find(?:\/|$)|\/file(?:\/|$)|\/vcs$|\/lsp$|\/formatter$|\/command$|\/path$|\/scheduler\/metrics$|\/session\/\{id\}\/(?:command|shell)$|\/worktree(?:\/|$))/.test(routePath)
  ) {
    let capability = "host.files.read";
    if (routePath.includes("command") || routePath.endsWith("/shell")) capability = "host.command.execute";
    else if (routePath.startsWith("/worktree")) capability = "host.worktree.manage";
    return {
      ingress_policy: "runtime_auth_gate",
      authorization_policy: "direct_loopback_local_owner_and_exact_host_effect_grant",
      capability,
      resolver: "canonical_host_resource",
      policy_origin: "host.local_effect",
    };
  }

  if (
    /^(?:\/browser\/(?:install|smoke-test)|\/global\/(?:diagnostics|workspace|storage|config|dispose)|\/admin\/reload-config|\/auth(?:\/|$)|\/channels(?:\/|$)|\/config\/(?:provider|providers|channels)|\/enterprise(?:\/|$))/.test(routePath)
  ) {
    return {
      ingress_policy: "runtime_auth_gate",
      authorization_policy: "deployment_administrator_or_loopback_local_owner",
      capability: "deployment.admin",
      resolver: "deployment_or_enterprise_state",
      policy_origin: "admin.deployment",
    };
  }

  if (/^(?:\/permission(?:\/|$)|\/question(?:\/|$)|\/approvals?(?:\/|$)|\/governance(?:\/|$))/.test(routePath)) {
    return {
      ingress_policy: "runtime_auth_gate",
      authorization_policy: "tenant_scoped_qualified_independent_reviewer",
      capability: "governance.review",
      resolver: "tenant_request_or_governance_record",
      policy_origin: "governance.review",
    };
  }

  const resourcePrefixes = [
    "/agent", "/automation", "/capabilities", "/coder", "/context", "/external-actions",
    "/goals", "/incident-monitor", "/marketplace", "/mcp", "/memory", "/mission-builder",
    "/missions", "/optimizations", "/orchestrations", "/pack-builder", "/packs", "/presets",
    "/project", "/resource", "/routines", "/run", "/runs", "/session", "/sessions", "/skills",
    "/stateful-runtime", "/task-intake", "/team", "/tool", "/workflow", "/workflows",
  ];
  if (routePath.includes("{") || resourcePrefixes.some((prefix) => routePath === prefix || routePath.startsWith(`${prefix}/`))) {
    return {
      ingress_policy: "runtime_auth_gate",
      authorization_policy: "tenant_resource_owner_or_scoped_grant",
      capability: "resource.owner_or_grant",
      resolver: "tenant_resource_from_state",
      policy_origin: "tenant.resource",
    };
  }

  return {
    ingress_policy: "runtime_auth_gate",
    authorization_policy: "verified_tenant_context",
    capability: null,
    resolver: "verified_tenant_context",
    policy_origin: "tenant.authenticated",
  };
}

export function buildInventory(files = routeFiles()) {
  assertProductionRouteCoverage(files);
  const routes = [];
  const unsupported = [];
  for (const relativePath of files) {
    const source = fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
    const extracted = extractRoutesFromRust(source, relativePath);
    routes.push(
      ...extracted.routes.map((route) => ({ ...route, listener: "engine_api" })),
    );
    unsupported.push(...extracted.unsupported);
  }
  if (unsupported.length > 0) throw new Error(`route inventory cannot classify dynamic registrations:\n${unsupported.join("\n")}`);

  routes.push(...webUiRoutes(), ...loopbackOAuthRoutes());

  const byKey = new Map();
  for (const route of routes) {
    const key = `${route.listener} ${route.method} ${route.path}`;
    const existing = byKey.get(key);
    if (existing) {
      existing.sources.push(route.source);
    } else {
      const policy = classifyRoute(route);
      byKey.set(key, {
        listener: route.listener,
        method: route.method,
        path: route.path,
        ...policy,
        sources: [route.source],
      });
    }
  }
  const inventoryRoutes = [...byKey.values()]
    .map((route) => ({ ...route, sources: [...new Set(route.sources)].sort() }))
    .sort(
      (left, right) =>
        left.listener.localeCompare(right.listener) ||
        left.path.localeCompare(right.path) ||
        left.method.localeCompare(right.method),
    );
  return {
    schema_version: 2,
    generated_by: "node scripts/verify-route-policy-inventory.mjs --write",
    scope: {
      included: [
        "engine API route registrars",
        "enterprise route extensions",
        "configured Web UI GET/HEAD surface",
        "loopback OpenAI Codex OAuth callback listener",
      ],
      excluded: [
        "cfg(test) fixture routers",
        "feature-gated loopback ACME demo Slack mock",
        "outbound provider URLs",
      ],
      middleware_surfaces: [
        {
          method: "OPTIONS",
          path: "{registered_engine_api_path}",
          policy: "CORS preflight only; auth_gate bypass does not dispatch an application handler",
        },
      ],
    },
    route_count: inventoryRoutes.length,
    policies: inventoryRoutes,
  };
}

function stableJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

export function verifyInventory(expectedText, inventory) {
  const generated = stableJson(inventory);
  if (expectedText !== generated) {
    throw new Error("route policy inventory drifted; run node scripts/verify-route-policy-inventory.mjs --write and review every policy change");
  }
}

function runSelfTest() {
  const fixture = `
    fn apply(router: Router<AppState>) -> Router<AppState> {
      router.route("/items/{id}", get(read).patch(update))
    }
    #[cfg(test)] mod tests {
      fn fixture() { Router::new().route("/test-only", post(handler)); }
    }
  `;
  const extracted = extractRoutesFromRust(fixture, "routes_fixture.rs");
  const keys = extracted.routes.map((route) => `${route.method} ${route.path}`).sort();
  if (
    JSON.stringify(keys) !==
    JSON.stringify(["GET /items/{id}", "HEAD /items/{id}", "PATCH /items/{id}"])
  ) {
    throw new Error(`route extractor self-test failed: ${JSON.stringify(keys)}`);
  }
  let rejected = false;
  try {
    verifyInventory("{}\n", { schema_version: 1 });
  } catch {
    rejected = true;
  }
  if (!rejected) throw new Error("inventory drift self-test failed to reject stale output");
  process.stdout.write("route policy inventory self-test passed\n");
}

function main() {
  if (process.argv.includes("--self-test")) {
    runSelfTest();
    return;
  }
  const inventory = buildInventory();
  if (process.argv.includes("--write")) {
    fs.writeFileSync(inventoryPath, stableJson(inventory));
    process.stdout.write(`wrote ${inventory.route_count} route policies to ${path.relative(repoRoot, inventoryPath)}\n`);
    return;
  }
  const expected = fs.readFileSync(inventoryPath, "utf8");
  verifyInventory(expected, inventory);
  const ingressCounts = inventory.policies.reduce((counts, policy) => {
    counts[policy.ingress_policy] = (counts[policy.ingress_policy] ?? 0) + 1;
    return counts;
  }, {});
  process.stdout.write(`${JSON.stringify({ route_count: inventory.route_count, ingress_counts: ingressCounts })}\n`);
  process.stdout.write("route policy inventory verified\n");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
