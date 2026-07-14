# RFC: Parameter-aware permission predicates

- Status: Proposed
- Linear: TAN-741
- Owners: Governance and runtime
- Required reviewers: Runtime engineering, security engineering
- Target model version: `permission_predicates/v1`

## Summary

Tandem should extend enterprise permission rules with a typed, bounded predicate expression evaluated against trusted tool-call arguments. The predicate engine belongs in the same server-side decision path that resolves tool identity, inherited scope, approval requirements, and receipts. A rule may match domains and hosts, fixed-precision monetary amounts, filesystem paths, repository coordinates, and ordinary JSON scalar fields without adding a new hardcoded evaluator for every connector.

The model is deny-by-default. A missing, malformed, unnormalizable, or over-budget value can never make an allow rule match. Non-overridable denies retain their current inheritance semantics, and a predicate cannot grant authority beyond the rule's tool, tenant, workflow, phase, data-class, or resource scope.

## Motivation and current state

The central tool dispatcher already supplies the full argument object to its policy context in `crates/tandem-tools/src/tool_dispatcher.rs`. The general standing-rule model in `crates/tandem-core/src/permissions.rs` matches permission and pattern identity, while `crates/tandem-enterprise-contract/src/policy_inheritance.rs` resolves scoped enterprise rules by inheritance, effect, version, and override behavior.

Argument-aware controls exist today, but only as specialized code. `crates/tandem-server/src/agent_teams_parts/part02.rs` contains dedicated host, filesystem, git, and secret checks, and `crates/tandem-server/src/agent_teams_parts/egress_preflight.rs` walks outbound arguments for DLP classification. These controls prove that arguments are available at enforcement time, but they are not a general policy-authoring capability.

## Goals

- Express typed predicates over trusted tool arguments without connector-specific Rust changes.
- Preserve the existing enterprise scope and inheritance model.
- Resolve allow, deny, and approval-required outcomes deterministically and fail closed.
- Validate policies before publication and reject ambiguous or unsafe expressions.
- Record enough predicate evidence to explain a decision without copying sensitive arguments into receipts.
- Provide a migration path for current permission patterns and specialized host, path, git, secret, and DLP evaluators.
- Bound evaluation time, memory, selector depth, and expression size.

## Non-goals

- A general-purpose scripting language, regular-expression engine, or arbitrary JSONPath implementation.
- Transforming, redacting, or mutating tool arguments.
- Replacing content inspection and secret/DLP classifiers with administrator-authored string predicates.
- Resolving symlinks or network destinations solely from model-supplied strings; execution-time guards remain authoritative.
- Allowing a narrower-scope rule to override a non-overridable deny.

## Proposed rule shape

`EnterprisePolicyRule` gains optional predicate, expiry, and lifecycle fields. Existing rules deserialize with no predicate and keep their current behavior.

```yaml
rule_id: finance-payments-over-10000
policy_id: finance-payment-controls
version: 3
scope_level: workflow
workflow_id: monthly-close
tool_patterns: ["mcp.payments.create_payment"]
effect: approval_required
overridable: false
expires_at_ms: 1798761600000
predicate:
  all:
    - selector: /amount/value
      value_type: decimal
      operator: greater_than_or_equal
      operand: "10000.00"
    - selector: /amount/currency
      value_type: currency_code
      operator: equals
      operand: USD
reason_code: finance_large_payment
reason: Payments of USD 10,000 or more require approval
```

The serialized predicate is a tagged expression:

```text
PredicateExpression =
  all(PredicateExpression[1..16]) |
  any(PredicateExpression[1..16]) |
  not(PredicateExpression) |
  condition(PredicateCondition)
```

The maximum nesting depth is four and a rule may contain at most 32 conditions. Empty `all` and `any` groups are invalid. `not` is allowed only around a condition in v1; this prevents confusing missing-value behavior in compound negations.

### Selectors

Selectors are RFC 6901 JSON Pointers relative to the dispatcher’s sanitized tool argument object. They must:

- start with `/`, or be the empty pointer only for supported composite root types;
- use only object keys and non-negative array indexes;
- contain at most 12 segments and 512 UTF-8 bytes;
- never select dispatcher-reserved keys beginning with `__`;
- never use wildcards, recursive descent, filters, functions, or dynamic expressions.

The engine evaluates exactly the arguments passed to the registered tool after trusted dispatch metadata is attached. Policy authors cannot select trusted metadata; tenant, principal, workflow, phase, and tool identity remain first-class rule scope fields.

### Value types and operators

| Value type      | Accepted input                                                | Operators                                                                                                |
| --------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `string`        | JSON string                                                   | `equals`, `not_equals`, `in`, `not_in`, `starts_with`, `ends_with`                                       |
| `boolean`       | JSON boolean                                                  | `equals`, `not_equals`                                                                                   |
| `integer`       | JSON integer in signed 64-bit range                           | `equals`, `not_equals`, `less_than`, `less_than_or_equal`, `greater_than`, `greater_than_or_equal`, `in` |
| `decimal`       | JSON string or number parsed as bounded fixed precision       | numeric comparison operators, `in`                                                                       |
| `currency_code` | three-letter string                                           | `equals`, `not_equals`, `in`, `not_in`                                                                   |
| `host`          | hostname or URL string                                        | `equals`, `in`, `is_subdomain_of`, `not_subdomain_of`                                                    |
| `email_domain`  | email address or domain string                                | `equals`, `in`, `is_subdomain_of`, `not_subdomain_of`                                                    |
| `path`          | path string                                                   | `equals`, `within`, `not_within`                                                                         |
| `repository`    | `owner/name`, recognized repository URL, or repository object | `equals`, `in`, `owner_equals`, `name_equals`                                                            |
| `array_length`  | JSON array                                                    | integer comparison operators                                                                             |
| `exists`        | any JSON value                                                | `exists`, `not_exists`                                                                                   |

Operands are validated against `value_type` at publication. There is no implicit cross-type coercion except the documented decimal, host, email-domain, path, and repository parsers. Floating-point comparison is prohibited.

### Normalization

Normalization happens once per unique selector and value type in a decision:

- Strings are Unicode NFC-normalized; case remains significant.
- Hosts are parsed with the URL/IDNA library, lowercased, converted to ASCII, stripped of a trailing dot and default port, and rejected if they contain userinfo, an invalid label, or an ambiguous IP representation.
- Email domains are extracted after validating a single address delimiter, then normalized as hosts. Local parts are never stored in predicate evidence.
- Decimals use a signed fixed-precision representation with at most 38 significant digits and 12 fractional digits. Currency codes are uppercased and checked against the server’s pinned ISO 4217 table.
- Paths reject NUL bytes, normalize separators and `.`/`..` lexically against a trusted workspace root, and retain a flag when the target does not exist. Filesystem tools must still perform their existing canonicalization and symlink checks immediately before access.
- Repositories normalize recognized HTTPS/SSH URLs or objects to lowercase host plus owner/name. Ref or target branch is a separate selector and remains case-sensitive.

Normalization failure produces `indeterminate`, never a match for an allow or approval rule.

## Three-valued evaluation and missing fields

Each condition produces `match`, `no_match`, or `indeterminate`.

- A missing selector produces `indeterminate`, except `exists` and `not_exists`, which resolve normally.
- A present value of the wrong type, an unnormalizable value, a selector budget violation, or an internal evaluation error produces `indeterminate`.
- `all` returns `no_match` if any child is `no_match`, otherwise `indeterminate` if any child is indeterminate.
- `any` returns `match` if any child matches, otherwise `indeterminate` if any child is indeterminate.

Rule matching maps the result by effect:

| Rule effect       | `match`   | `no_match` | `indeterminate` |
| ----------------- | --------- | ---------- | --------------- |
| Allow             | candidate | ignored    | ignored         |
| Approval required | candidate | ignored    | ignored         |
| Deny              | candidate | ignored    | candidate deny  |

This makes malformed or omitted arguments incapable of activating authority. A deny rule fails closed when the server cannot safely determine whether its forbidden condition applies. If no rule wins, the existing enterprise resolver returns deny.

## Precedence and conflict resolution

Predicate evaluation filters rules before the existing inheritance winner is selected. The resolver otherwise retains its current order:

1. Match tenant, organization unit, workspace/resource, workflow, phase, permission, data class, and tool pattern.
2. Drop expired or disabled rules.
3. Evaluate predicates and retain candidate rules using the table above.
4. Sort by scope inheritance rank, same-level effect priority (`allow < approval_required < deny`), version, update time, and stable rule ID.
5. Select the most specific/highest-priority candidate.
6. Before returning it, apply the current non-overridable restriction: a less-specific, non-overridable rule with a more restrictive effect wins over a more-specific permissive rule.

Predicates add no separate priority dimension. Two same-level rules that both match resolve using the existing effect, version, update-time, and rule-ID order. Publishing two indistinguishable same-level rules with conflicting effects generates a validation warning; the preview must show the deterministic winner.

## Expiry, publication, and invalid policy handling

- `expires_at_ms` is optional. An expired rule is ignored for allow and approval outcomes and treated as a candidate deny only when the rule is a non-overridable deny whose expiry cannot be verified because the clock is unavailable.
- Draft policies may be incomplete, but publication performs full schema, selector, operand, scope, conflict, and resource-limit validation.
- A publish request is atomic: either the complete version validates and becomes active, or the prior active version remains authoritative.
- A previously published rule that fails to deserialize or validate after an upgrade is quarantined. The server emits an operator-visible startup/readiness error for hosted deployments and evaluates the affected policy set as deny until repaired.
- Unknown expression versions, value types, and operators are invalid; they are never ignored.

## Decision receipts and privacy

Policy decision receipts add a bounded `predicate_evidence` object:

```json
{
  "expression_version": "permission_predicates/v1",
  "expression_digest": "sha256:...",
  "result": "match",
  "conditions": [
    {
      "condition_id": "amount-threshold",
      "selector_digest": "sha256:...",
      "value_type": "decimal",
      "operator": "greater_than_or_equal",
      "result": "match",
      "value_digest": "hmac-sha256:..."
    }
  ],
  "truncated": false
}
```

Receipts never contain raw selected values, raw operands, email local parts, paths, repository URLs, or request arguments. `value_digest` uses a deployment-scoped audit HMAC key so operators can correlate equal values inside one deployment without enabling cross-deployment correlation or offline guessing. Low-cardinality values such as booleans and currency codes omit `value_digest`. Evidence contains at most 32 condition rows and records truncation or indeterminate causes using stable reason codes.

Allow, deny, approval-required, and execution receipts link to the same policy decision ID and expression digest. Failure to write a required policy receipt remains fail closed.

## Authoring and preview contract

The server API must expose:

- schema metadata for selectors, types, operators, and limits;
- draft validation returning field-addressable errors and warnings;
- effective-policy preview for a supplied trusted scope plus redacted example arguments;
- the winning rule, inherited candidates, predicate results, non-overridable constraints, and default-deny outcome;
- atomic publish, disable, supersede, rollback, and expiry operations with access control and receipts.

The Control Panel should generate the typed expression; it should not expose raw JSON as the primary workflow. An advanced JSON view may be read-only in v1.

## Compatibility and migration

### Existing `PermissionRule`

Standing `PermissionRule` records remain identity/pattern rules. They do not gain predicates in place because their persistence, session semantics, and `always allow` behavior differ from enterprise scoped policy. The dispatcher adapter converts a matched standing rule to a synthetic enterprise candidate with no predicate. New parameter-aware authoring targets `EnterprisePolicyRule`.

### Existing enterprise rules

Rules without `predicate` deserialize as unconditional within their existing scope. Their policy version digest changes only when they are republished under the new schema, avoiding surprise receipt churn during read-only upgrades.

### Specialized evaluators

Migration is staged:

1. Run the predicate engine alongside current host, path, git, secret, and DLP evaluators in observe-only mode and compare redacted outcomes.
2. Express equivalent host allowlists, path roots, repository constraints, and amount thresholds as generated enterprise rules.
3. Keep secret scanning, content DLP, symlink/canonical-path enforcement, URL resolution, and provider-specific transaction validation as mandatory lower-level guards.
4. Remove a specialized identity/value check only after equivalence tests cover its normalization, missing-field, and bypass cases.

Hardcoded safety checks may always make a decision more restrictive. A predicate rule can never suppress a lower-level deny or approval requirement.

## Threat model

The design assumes arguments may be adversarial model output. Threats and mitigations include:

- **Field omission or type confusion:** three-valued evaluation prevents malformed values from activating allow authority.
- **Unicode/IDNA and URL ambiguity:** typed normalization rejects ambiguous encodings and userinfo tricks.
- **Path traversal and symlink races:** lexical predicate checks are advisory authority filters; execution-time canonicalization remains mandatory.
- **Floating-point and currency confusion:** fixed precision and explicit currency selectors; no implicit FX conversion.
- **Repository URL confusion:** recognized schemes and normalized host/owner/name only.
- **Expression denial of service:** hard limits on bytes, nodes, depth, arrays, and evaluation time.
- **Secret leakage through receipts:** HMAC digests and stable reason codes replace raw values.
- **Policy shadowing:** preview shows every candidate and the winner; non-overridable restrictive rules retain precedence.
- **Stale policy versions:** atomic publication and decision receipts pin the policy version and expression digest.

## Performance limits

- Maximum 32 conditions, four expression levels, 16 children per group, and 16 operands for `in`/`not_in`.
- Maximum 64 KiB serialized predicate per policy version and 512 bytes per selector.
- Maximum 128 KiB of selected argument data normalized per decision; larger values become indeterminate.
- Compiled predicates are cached by policy version digest.
- Selector lookups and normalization are memoized within a decision.
- Evaluation has a 5 ms soft budget and 20 ms hard budget. Exceeding the hard budget returns indeterminate and fails closed according to effect.
- No network, DNS, filesystem read, secret lookup, or provider call occurs during predicate evaluation.

## Test strategy

- Unit tests for every selector escape, type/operator pair, normalization rule, and three-valued truth table.
- Property tests for Unicode hosts, decimal boundaries, path traversal, repository parsers, and deterministic serialization/digests.
- Inheritance matrix tests covering allow/deny/approval, same-level conflict, non-overridable deny, expiry, supersession, and default deny.
- Receipt tests proving raw values never appear and expression/value digests are stable only within the intended scope.
- Differential tests against current host, path, git, secret, and DLP evaluators during migration.
- Round-trip API and Control Panel tests: author, validate, preview, publish, execute, disable, supersede, rollback.
- Fuzz tests for policy decoding and argument evaluation under size/time limits.
- End-to-end cases must invoke registered native and MCP tools through the same dispatcher enforcement point.

## Worked examples

### CRM agent

Allow draft creation only for company recipients, require approval for external domains, and keep credential/DLP scanning mandatory:

```yaml
- tool_patterns: ["mcp.crm.create_email_draft"]
  effect: allow
  predicate:
    condition:
      selector: /recipient/email
      value_type: email_domain
      operator: in
      operand: [example.com, example.org]
- tool_patterns: ["mcp.crm.create_email_draft"]
  effect: approval_required
  predicate:
    condition:
      selector: /recipient/email
      value_type: email_domain
      operator: not_in
      operand: [example.com, example.org]
```

### Finance agent

Allow same-currency payments below USD 10,000 and require approval at or above the threshold. A missing or malformed amount matches neither permissive rule and therefore denies by default.

```yaml
- tool_patterns: ["mcp.payments.create_payment"]
  effect: allow
  predicate:
    all:
      - condition:
          { selector: /amount/currency, value_type: currency_code, operator: equals, operand: USD }
      - condition:
          { selector: /amount/value, value_type: decimal, operator: less_than, operand: "10000.00" }
- tool_patterns: ["mcp.payments.create_payment"]
  effect: approval_required
  predicate:
    all:
      - condition:
          { selector: /amount/currency, value_type: currency_code, operator: equals, operand: USD }
      - condition:
          {
            selector: /amount/value,
            value_type: decimal,
            operator: greater_than_or_equal,
            operand: "10000.00",
          }
```

### Coding agent

Allow writes only inside the workspace and require approval for pushes to the protected repository. Git credential, branch-protection, and filesystem canonicalization checks remain lower-level enforcement.

```yaml
- tool_patterns: [write, edit, apply_patch]
  effect: allow
  predicate:
    condition:
      selector: /path
      value_type: path
      operator: within
      operand: "${trusted.workspace_root}"
- tool_patterns: [git_push]
  effect: approval_required
  overridable: false
  predicate:
    all:
      - condition:
          {
            selector: /repository,
            value_type: repository,
            operator: equals,
            operand: frumu-ai/tandem,
          }
      - condition: { selector: /branch, value_type: string, operator: in, operand: [main, release] }
```

`${trusted.workspace_root}` is not general interpolation. It is a schema-defined operand token resolved from verified server context; v1 supports only a fixed allowlist of typed trusted tokens.

## Rollout

1. Land types, validation, compilation, and receipt schema behind `TANDEM_PARAMETER_POLICY_V1=observe`.
2. Run differential telemetry with no authority changes and publish mismatch metrics without argument values.
3. Enable authored predicates for selected tenants with the existing specialized guards still authoritative.
4. Make predicate evaluation generally available after security review and performance targets are met.
5. Consider deprecating duplicated specialized checks only through separate reviewed changes.

## Required review record

Implementation issues TAN-742 and TAN-743 must remain in design/backlog until both reviews are recorded here or in the merged pull request:

- [ ] Runtime engineering review: selector/evaluator integration, inheritance, performance, migration.
- [ ] Security engineering review: normalization, fail-closed behavior, receipt privacy, bypass analysis.

Any material change to missing-field semantics, deny precedence, receipt redaction, or trusted operand tokens requires both reviewers again.
