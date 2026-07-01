import assert from "node:assert/strict";
import test from "node:test";
import {
  buildEnterpriseScopeExplorerModel,
  resourceKey,
  selectEnterpriseScope,
} from "../lib/enterprise/scope-explorer.js";

const financeResource = {
  organization_id: "org-a",
  workspace_id: "workspace-a",
  resource_kind: "repository",
  resource_id: "repo-a",
};

function fixtureModel() {
  return buildEnterpriseScopeExplorerModel({
    orgUnits: [
      {
        taxonomy_id: "department",
        unit_id: "finance",
        display_name: "Finance",
        kind: "department",
        state: "active",
      },
      {
        taxonomy_id: "team",
        unit_id: "ap",
        parent_unit_id: "finance",
        display_name: "Accounts Payable",
        kind: "team",
        state: "active",
      },
    ],
    memberships: [
      {
        membership_id: "membership-a",
        unit: { kind: "org_unit", id: "finance" },
        member: { kind: "user", id: "operator-a" },
        state: "active",
      },
    ],
    accessGrants: [
      {
        grant_id: "grant-allow",
        unit: { kind: "org_unit", id: "finance" },
        resource: financeResource,
        effect: "allow",
        permissions: ["read", "execute"],
        data_classes: ["financial_record"],
        state: "active",
      },
      {
        grant_id: "grant-deny",
        unit: { kind: "org_unit", id: "finance" },
        resource: { ...financeResource, resource_id: "repo-secret" },
        effect: "deny",
        permissions: ["read"],
        data_classes: ["restricted"],
        state: "active",
      },
    ],
    sourceBindings: [
      {
        binding_id: "binding-visible",
        connector_id: "github",
        source_type: "repository",
        native_source_id: "repo-a",
        source_root_label: "Finance repo",
        resource_ref: financeResource,
        data_class: "financial_record",
        state: "active",
        ingestion_policy: { allow_indexing: true, allow_prompt_context: true },
      },
      {
        binding_id: "binding-denied",
        connector_id: "drive",
        source_type: "shared_drive",
        native_source_id: "drive-secret",
        source_root_label: "Restricted drive",
        resource_ref: { ...financeResource, resource_id: "repo-secret" },
        data_class: "restricted",
        state: "active",
        ingestion_policy: { allow_indexing: true, allow_prompt_context: false },
      },
    ],
    sourceObjects: [
      {
        source_object_id: "object-a",
        source_binding_id: "binding-visible",
        connector_id: "github",
        state: "indexed",
        tier: "hot",
        import_namespace: "finance",
        indexed_path: "README.md",
        native_object_id: "README.md",
        resource_ref: financeResource,
        data_class: "financial_record",
        first_seen_at_ms: 1000,
        last_seen_at_ms: 2000,
      },
    ],
    runs: [
      {
        run: {
          run_id: "run-finance",
          automation_id: "close-books",
          status: "running",
          updated_at_ms: 3000,
        },
        enterprise_scope: {
          owning_org_unit_id: "finance",
          owner_principal: { kind: "automation", id: "close-books" },
          resource_kind: "repository",
          resource_id: "repo-a",
          policy_version_id: "policy-2026-07",
          visible_knowledge_sources: [
            {
              binding_id: "binding-visible",
              source_type: "repository",
              source_root_label: "Finance repo",
              data_class: "financial_record",
            },
          ],
        },
      },
    ],
  });
}

test("enterprise scope explorer builds a selectable org tree", () => {
  const model = fixtureModel();

  assert.equal(model.orgTree.flat.length, 2);
  assert.equal(model.orgTree.flat[0].label, "Finance");
  assert.equal(model.orgTree.flat[1].label, "Accounts Payable");
  assert.equal(model.orgTree.flat[1].depth, 1);
  assert.ok(model.scopes.find((scope) => scope.id === "org:department/finance"));
});

test("enterprise scope explorer renders ordered policy layers with conflicts", () => {
  const detail = selectEnterpriseScope(fixtureModel(), "org:department/finance");

  assert.deepEqual(
    detail.policyLayers.map((layer) => layer.layer),
    ["Enterprise", "Org Unit", "Resource", "Run"]
  );
  assert.equal(detail.policyLayers[1].overrides[0], "grant-allow");
  assert.equal(detail.policyLayers[1].conflicts[0], "Deny grant grant-deny");
  assert.equal(detail.policyLayers[3].decision, "policy-2026-07");
});

test("enterprise scope explorer explains visible and blocked knowledge boundaries", () => {
  const model = fixtureModel();
  const visible = selectEnterpriseScope(model, `resource:${resourceKey(financeResource)}`);
  const denied = selectEnterpriseScope(model, "resource:repository:repo-secret");

  assert.equal(visible.visibleKnowledge[0].id, "binding-visible");
  assert.equal(visible.visibleKnowledge[0].reason, "resource grant allows context");
  assert.equal(visible.blockedKnowledge[0].id, "binding-denied");
  assert.equal(visible.blockedKnowledge[0].reason, "resource scope mismatch");
  assert.equal(denied.blockedKnowledge.find((row) => row.id === "binding-denied").reason, "prompt context disabled");
});

test("enterprise scope explorer includes deep links to stateful run detail", () => {
  const detail = selectEnterpriseScope(fixtureModel(), "org:department/finance");

  assert.equal(detail.runs.length, 1);
  assert.equal(detail.runs[0].id, "run-finance");
  assert.equal(detail.runs[0].route, "runs?run=run-finance");
  assert.equal(detail.runs[0].owner, "automation:close-books");
});
