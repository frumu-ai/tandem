import assert from "node:assert/strict";
import test from "node:test";

import {
  activePolicyRulesForSupersede,
  buildPolicyPreviewArguments,
  buildTemplatePredicateOverrides,
  parsePolicyOperand,
} from "../lib/enterprise/policy-authoring.js";

test("policy authoring builds typed predicate operands and preview arguments", () => {
  assert.deepEqual(parsePolicyOperand("example.com, example.org", "in", "email_domain"), [
    "example.com",
    "example.org",
  ]);
  assert.equal(parsePolicyOperand("10000", "greater_than_or_equal", "decimal"), "10000");
  assert.deepEqual(buildPolicyPreviewArguments("/amount/value", "15000.00"), {
    amount: { value: "15000.00" },
  });
});

test("template authoring emits bounded condition overrides without copying rule sets", () => {
  assert.deepEqual(
    buildTemplatePredicateOverrides("large-payments", "approval-threshold", "5000.00"),
    [
      {
        rule_id: "large-payments",
        predicate_operands: { "approval-threshold": "5000.00" },
      },
    ]
  );
  assert.deepEqual(buildTemplatePredicateOverrides("", "approval-threshold", "5000.00"), []);
  assert.deepEqual(
    buildTemplatePredicateOverrides(
      "internal-drafts",
      "company-domains",
      "example.com, example.org",
      "in",
      "email_domain"
    ),
    [
      {
        rule_id: "internal-drafts",
        predicate_operands: { "company-domains": ["example.com", "example.org"] },
      },
    ]
  );
});

test("policy supersede only carries forward published rules", () => {
  const rules = [
    { rule_id: "current", policy_id: "payments", state: "published" },
    { rule_id: "historical", policy_id: "payments", state: "superseded" },
    { rule_id: "disabled", policy_id: "payments", state: "disabled" },
    { rule_id: "other", policy_id: "repositories", state: "published" },
  ];

  assert.deepEqual(activePolicyRulesForSupersede(rules, "payments"), [rules[0]]);
});
