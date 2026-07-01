function toArray(input, key) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function stringValue(value, fallback = "") {
  if (value === null || value === undefined) return fallback;
  const text = String(value).trim();
  return text || fallback;
}

function normalizeKey(value) {
  return stringValue(value)
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[\s-]+/g, "_")
    .toLowerCase();
}

function titleCase(value, fallback = "Unknown") {
  const key = normalizeKey(value);
  if (!key) return fallback;
  return key
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function read(row, keys, fallback = "") {
  for (const key of keys) {
    const value = row?.[key];
    if (value !== undefined && value !== null && value !== "") return value;
  }
  return fallback;
}

function uniqueBy(rows, identity) {
  const seen = new Set();
  const out = [];
  for (const row of rows) {
    const key = identity(row);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(row);
  }
  return out;
}

function resourceKey(resource = {}) {
  const kind = read(resource, ["resource_kind", "resourceKind"]);
  const id = read(resource, ["resource_id", "resourceId"]);
  return kind && id ? `${kind}:${id}` : "";
}

function resourceLabel(resource = {}) {
  const kind = read(resource, ["resource_kind", "resourceKind"]);
  const id = read(resource, ["resource_id", "resourceId"]);
  return [titleCase(kind, ""), id].filter(Boolean).join(" / ") || "Tenant scope";
}

function principalLabel(principal = {}) {
  return [read(principal, ["kind"]), read(principal, ["id", "tenant_actor_id", "tenantActorId"])].filter(Boolean).join(":");
}

function orgUnitKey(unit = {}) {
  const taxonomy = read(unit, ["taxonomy_id", "taxonomyId"], "organization_unit");
  const id = read(unit, ["unit_id", "unitId"]);
  return id ? `${taxonomy}/${id}` : "";
}

function runRecord(row = {}) {
  return row?.run && typeof row.run === "object" ? row.run : row;
}

function runEnterpriseScope(row = {}) {
  return row?.enterprise_scope || row?.enterpriseScope || runRecord(row)?.enterprise_scope || runRecord(row)?.enterpriseScope || {};
}

function runResourceKey(row = {}) {
  const enterprise = runEnterpriseScope(row);
  const resource =
    enterprise.resource_scope?.root ||
    enterprise.resourceScope?.root ||
    enterprise.root_resource ||
    enterprise.rootResource ||
    {};
  const kind = read(enterprise, ["resource_kind", "resourceKind"]) || read(resource, ["resource_kind", "resourceKind"]);
  const id = read(enterprise, ["resource_id", "resourceId"]) || read(resource, ["resource_id", "resourceId"]);
  return kind && id ? `${kind}:${id}` : "";
}

function runOrgUnitId(row = {}) {
  const enterprise = runEnterpriseScope(row);
  return read(enterprise, ["owning_org_unit_id", "owningOrgUnitId"]);
}

function runId(row = {}) {
  const run = runRecord(row);
  return read(run, ["run_id", "runId", "id"]);
}

function runTitle(row = {}) {
  const run = runRecord(row);
  return (
    read(run, ["workflow_name", "workflowName", "automation_id", "automationId", "name", "kind"], "Stateful run") ||
    "Stateful run"
  );
}

function selectedMatchesRun(scope, row) {
  if (!scope) return false;
  if (scope.kind === "org_unit") return runOrgUnitId(row) === scope.orgUnitId;
  if (scope.resourceKey) return runResourceKey(row) === scope.resourceKey;
  return false;
}

function selectedMatchesResource(scope, resource = {}) {
  if (!scope) return false;
  if (scope.resourceKey) return resourceKey(resource) === scope.resourceKey;
  return false;
}

function selectedMatchesOrgUnit(scope, unitId) {
  if (!scope || !unitId) return false;
  return scope.kind === "org_unit" && scope.orgUnitId === unitId;
}

function buildOrgTree(orgUnits = []) {
  const byId = new Map();
  for (const unit of orgUnits) {
    byId.set(read(unit, ["unit_id", "unitId"]), {
      id: orgUnitKey(unit),
      unitId: read(unit, ["unit_id", "unitId"]),
      parentUnitId: read(unit, ["parent_unit_id", "parentUnitId"]),
      label: read(unit, ["display_name", "displayName", "unit_id", "unitId"], "Org unit"),
      kind: read(unit, ["kind"], "custom"),
      state: read(unit, ["state"], "active"),
      depth: 0,
      children: [],
      raw: unit,
    });
  }
  for (const node of byId.values()) {
    const parent = byId.get(node.parentUnitId);
    if (parent) parent.children.push(node);
  }
  const roots = [...byId.values()].filter((node) => !node.parentUnitId || !byId.has(node.parentUnitId));
  const flat = [];
  const visit = (node, depth) => {
    node.depth = depth;
    flat.push(node);
    node.children.sort((a, b) => a.label.localeCompare(b.label));
    for (const child of node.children) visit(child, depth + 1);
  };
  roots.sort((a, b) => a.label.localeCompare(b.label));
  for (const root of roots) visit(root, 0);
  return { roots, flat };
}

function scopeFromOrgUnit(unit) {
  const unitId = read(unit, ["unit_id", "unitId"]);
  return {
    id: `org:${orgUnitKey(unit)}`,
    kind: "org_unit",
    label: read(unit, ["display_name", "displayName"], unitId),
    orgUnitId: unitId,
    resourceKey: "",
    resourceLabel: "All resources",
    state: read(unit, ["state"], "active"),
    raw: unit,
  };
}

function scopeFromResource(resource, source) {
  const key = resourceKey(resource);
  return {
    id: `resource:${key}`,
    kind: "resource",
    label: resourceLabel(resource),
    orgUnitId: "",
    resourceKey: key,
    resourceLabel: resourceLabel(resource),
    state: read(source, ["state"], "active"),
    raw: source,
  };
}

function buildScopeOptions({ orgUnits, accessGrants, sourceBindings, runs }) {
  const orgScopes = orgUnits.map(scopeFromOrgUnit);
  const grantScopes = accessGrants.map((grant) => scopeFromResource(read(grant, ["resource"], {}), grant));
  const bindingScopes = sourceBindings.map((binding) => scopeFromResource(read(binding, ["resource_ref", "resourceRef"], {}), binding));
  const runScopes = runs
    .map((row) => {
      const enterprise = runEnterpriseScope(row);
      const resource =
        enterprise.resource_scope?.root ||
        enterprise.resourceScope?.root ||
        enterprise.root_resource ||
        enterprise.rootResource ||
        {};
      return scopeFromResource(
        {
          resource_kind: read(enterprise, ["resource_kind", "resourceKind"]) || read(resource, ["resource_kind", "resourceKind"]),
          resource_id: read(enterprise, ["resource_id", "resourceId"]) || read(resource, ["resource_id", "resourceId"]),
        },
        row
      );
    })
    .filter((scope) => scope.resourceKey);
  return uniqueBy([...orgScopes, ...bindingScopes, ...grantScopes, ...runScopes], (scope) => scope.id);
}

function matchingGrants(scope, grants = []) {
  return grants.filter((grant) => {
    const unitId = read(read(grant, ["unit"], {}), ["id"]);
    return selectedMatchesOrgUnit(scope, unitId) || selectedMatchesResource(scope, read(grant, ["resource"], {}));
  });
}

function matchingBindings(scope, bindings = []) {
  return bindings.filter((binding) => selectedMatchesResource(scope, read(binding, ["resource_ref", "resourceRef"], {})));
}

function matchingRuns(scope, runs = []) {
  return runs.filter((row) => selectedMatchesRun(scope, row));
}

function policyLayersForScope(scope, grants, bindings, runs) {
  const denyGrants = grants.filter((grant) => normalizeKey(read(grant, ["effect"], "allow")) === "deny");
  const policyVersions = uniqueBy(
    runs
      .map((row) => read(runEnterpriseScope(row), ["policy_version_id", "policyVersionId"]))
      .filter(Boolean)
      .map((version) => ({ version })),
    (row) => row.version
  );
  const blockedBindings = bindings.filter((binding) => {
    const policy = read(binding, ["ingestion_policy", "ingestionPolicy"], {}) || {};
    return (
      normalizeKey(read(binding, ["state"], "active")) !== "active" ||
      policy.allow_prompt_context === false ||
      policy.allow_indexing === false
    );
  });

  return [
    {
      order: 1,
      layer: "Enterprise",
      source: "Tenant baseline",
      decision: "Default policy",
      overrides: [],
      conflicts: [],
    },
    {
      order: 2,
      layer: "Org Unit",
      source: scope?.kind === "org_unit" ? scope.label : "Inherited owner",
      decision: grants.length ? `${grants.length} scoped grants` : "No scoped grants",
      overrides: grants.filter((grant) => normalizeKey(read(grant, ["effect"], "allow")) === "allow").map((grant) => read(grant, ["grant_id", "grantId"])),
      conflicts: denyGrants.map((grant) => `Deny grant ${read(grant, ["grant_id", "grantId"])}`),
    },
    {
      order: 3,
      layer: "Resource",
      source: scope?.resourceLabel || "Selected resource",
      decision: bindings.length ? `${bindings.length} source bindings` : "No source bindings",
      overrides: bindings.map((binding) => read(binding, ["binding_id", "bindingId"])),
      conflicts: blockedBindings.map((binding) => `Blocked source ${read(binding, ["binding_id", "bindingId"])}`),
    },
    {
      order: 4,
      layer: "Run",
      source: "Stateful runtime",
      decision: policyVersions.length
        ? policyVersions.map((row) => row.version).join(", ")
        : "No run policy version",
      overrides: policyVersions.map((row) => row.version),
      conflicts: [],
    },
  ];
}

function sourceVisibility(scope, binding) {
  const resource = read(binding, ["resource_ref", "resourceRef"], {}) || {};
  const policy = read(binding, ["ingestion_policy", "ingestionPolicy"], {}) || {};
  const state = normalizeKey(read(binding, ["state"], "active"));
  if (!selectedMatchesResource(scope, resource)) {
    return { visibility: "blocked", reason: "resource scope mismatch" };
  }
  if (state !== "active") return { visibility: "blocked", reason: `binding ${state}` };
  if (policy.allow_prompt_context === false) return { visibility: "blocked", reason: "prompt context disabled" };
  if (policy.allow_indexing === false) return { visibility: "blocked", reason: "indexing disabled" };
  if (policy.require_review) return { visibility: "visible", reason: "visible after review" };
  return { visibility: "visible", reason: "resource grant allows context" };
}

function knowledgeRows(scope, sourceBindings = [], sourceObjects = [], runs = []) {
  const visibleFromRuns = runs.flatMap((row) => toArray(runEnterpriseScope(row).visible_knowledge_sources || runEnterpriseScope(row).visibleKnowledgeSources, "visible_knowledge_sources"));
  const bindingRows = sourceBindings.map((binding) => {
    const resource = read(binding, ["resource_ref", "resourceRef"], {}) || {};
    const visibility = sourceVisibility(scope, binding);
    const objects = sourceObjects.filter((object) => read(object, ["source_binding_id", "sourceBindingId"]) === read(binding, ["binding_id", "bindingId"]));
    return {
      id: read(binding, ["binding_id", "bindingId"]),
      label: read(binding, ["source_root_label", "sourceRootLabel", "native_source_id", "nativeSourceId"], "Source binding"),
      connectorId: read(binding, ["connector_id", "connectorId"]),
      sourceType: read(binding, ["source_type", "sourceType"]),
      resourceKey: resourceKey(resource),
      resourceLabel: resourceLabel(resource),
      dataClass: read(binding, ["data_class", "dataClass"]),
      objectCount: objects.length,
      ...visibility,
    };
  });
  const runRows = visibleFromRuns.map((source, index) => ({
    id: read(source, ["binding_id", "bindingId"], `run-source-${index + 1}`),
    label: read(source, ["source_root_label", "sourceRootLabel", "binding_id", "bindingId"], "Run source"),
    connectorId: read(source, ["connector_id", "connectorId"]),
    sourceType: read(source, ["source_type", "sourceType"]),
    resourceKey: scope?.resourceKey || "",
    resourceLabel: scope?.resourceLabel || "",
    dataClass: read(source, ["data_class", "dataClass"]),
    objectCount: 0,
    visibility: "visible",
    reason: "visible in run scope",
  }));
  return uniqueBy([...bindingRows, ...runRows], (row) => row.id || `${row.label}:${row.reason}`);
}

function recentRunRows(scope, runs = []) {
  return matchingRuns(scope, runs)
    .map((row) => {
      const run = runRecord(row);
      const id = runId(row);
      const enterprise = runEnterpriseScope(row);
      return {
        id,
        title: runTitle(row),
        status: read(run, ["status"], "unknown"),
        owner: principalLabel(read(enterprise, ["owner_principal", "ownerPrincipal"], {})),
        orgUnitId: runOrgUnitId(row),
        resourceKey: runResourceKey(row),
        updatedAtMs: Number(read(run, ["updated_at_ms", "updatedAtMs", "created_at_ms", "createdAtMs"], 0)) || 0,
        route: id ? `runs?run=${encodeURIComponent(id)}` : "",
      };
    })
    .sort((a, b) => b.updatedAtMs - a.updatedAtMs)
    .slice(0, 12);
}

function buildEnterpriseScopeExplorerModel(input = {}) {
  const orgUnits = toArray(input.orgUnits, "org_units");
  const memberships = toArray(input.memberships, "memberships");
  const accessGrants = toArray(input.accessGrants, "access_grants");
  const effectiveGrants = toArray(input.effectiveGrants, "grants");
  const sourceBindings = toArray(input.sourceBindings, "source_bindings");
  const sourceObjects = toArray(input.sourceObjects, "source_objects");
  const runs = toArray(input.runs, "runs");
  const orgTree = buildOrgTree(orgUnits);
  const scopes = buildScopeOptions({ orgUnits, accessGrants, sourceBindings, runs });
  return {
    orgTree,
    scopes,
    orgUnits,
    memberships,
    accessGrants,
    effectiveGrants,
    sourceBindings,
    sourceObjects,
    runs,
    summary: {
      scopes: scopes.length,
      orgUnits: orgUnits.length,
      memberships: memberships.length,
      grants: accessGrants.length + effectiveGrants.length,
      sourceBindings: sourceBindings.length,
      recentRuns: runs.length,
    },
  };
}

function selectEnterpriseScope(model, scopeId) {
  const scope = model.scopes.find((item) => item.id === scopeId) || model.scopes[0] || null;
  const grants = matchingGrants(scope, [...model.accessGrants, ...model.effectiveGrants]);
  const bindings = matchingBindings(scope, model.sourceBindings);
  const runs = matchingRuns(scope, model.runs);
  const knowledge = knowledgeRows(scope, model.sourceBindings, model.sourceObjects, runs);
  return {
    scope,
    grants,
    bindings,
    runs: recentRunRows(scope, model.runs),
    policyLayers: policyLayersForScope(scope, grants, bindings, runs),
    knowledge,
    blockedKnowledge: knowledge.filter((row) => row.visibility === "blocked"),
    visibleKnowledge: knowledge.filter((row) => row.visibility === "visible"),
  };
}

export {
  buildEnterpriseScopeExplorerModel,
  resourceKey,
  resourceLabel,
  selectEnterpriseScope,
  titleCase,
};
