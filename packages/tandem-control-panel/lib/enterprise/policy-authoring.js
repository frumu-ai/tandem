/** @param {string} value */
export function splitPolicyList(value) {
  return String(value || "")
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

/**
 * @param {string} value
 * @param {string} operator
 * @param {string} valueType
 * @returns {unknown}
 */
export function parsePolicyOperand(value, operator, valueType) {
  if (operator === "exists" || operator === "not_exists") return null;
  if (operator === "in" || operator === "not_in") return splitPolicyList(value);
  if (valueType === "boolean") return value.trim().toLowerCase() === "true";
  if (valueType === "integer" || valueType === "array_length") {
    return Number.parseInt(value, 10);
  }
  return value;
}

/** @param {string} selector @param {unknown} value */
export function buildPolicyPreviewArguments(selector, value) {
  /** @type {Record<string, unknown>} */
  const root = {};
  const segments = String(selector || "")
    .split("/")
    .slice(1)
    .map((part) => part.replaceAll("~1", "/").replaceAll("~0", "~"));
  let cursor = root;
  segments.forEach((segment, index) => {
    if (index === segments.length - 1) cursor[segment] = value;
    else cursor = /** @type {Record<string, unknown>} */ (cursor[segment] = {});
  });
  return root;
}

/**
 * @param {string} ruleId
 * @param {string} conditionId
 * @param {string} operand
 */
export function buildTemplatePredicateOverrides(ruleId, conditionId, operand) {
  const rule = ruleId.trim();
  const condition = conditionId.trim();
  if (!rule || !condition) return [];
  return [{ rule_id: rule, predicate_operands: { [condition]: operand } }];
}

/**
 * @template {{ policy_id: string, state?: string }} T
 * @param {T[]} rules
 * @param {string} policyId
 * @returns {T[]}
 */
export function activePolicyRulesForSupersede(rules, policyId) {
  return rules.filter((rule) => rule.policy_id === policyId && rule.state === "published");
}
