---
name: support-escalation
description: Structure and package support escalations for engineering, product, or leadership with full context, reproduction steps, and business impact.
---

# Support Escalation

Determine when and how to escalate support issues. Structure escalation briefs that give receiving teams everything they need to act quickly, and follow escalation through to resolution.

## When to Escalate

- **Technical**: Bug confirmed and needs a code fix, infrastructure investigation, data corruption.
- **Complexity**: Beyond support's ability to diagnose, requires access support doesn't have.
- **Impact**: Multiple customers affected, production system down, security concern.
- **Business**: High-value customer at risk, SLA breach imminent, executive involvement.
- **Time**: Issue has been open beyond SLA, customer waiting unreasonably long.
- **Pattern**: Same issue reported by 3+ customers.

## Escalation Tiers

1. **Support Escalation (L1 -> L2)**: For deeper investigation or advanced troubleshooting.
2. **Engineering Escalation**: For confirmed bugs, infrastructure issues, code changes.
3. **Product Escalation**: For feature gaps, design decisions, prioritization.
4. **Security Escalation**: For potential data exposure, vulnerabilities, compliance (Escalate IMMEDIATELY).
5. **Leadership Escalation**: For high churn risk, SLA breaches, cross-functional decisions.

## Structured Escalation Format

Every escalation should follow this structure:

```markdown
ESCALATION: [One-line summary]
Severity: [Critical / High / Medium]
Target: [Engineering / Product / Security / Leadership]

IMPACT

- Customers affected: [Number and names if relevant]
- Workflow impact: [What's broken for them]
- Revenue at risk: [If applicable]
- SLA status: [Within SLA / At risk / Breached]

ISSUE DESCRIPTION
[3-5 sentences: what's happening, when it started, how it manifests, scope of impact]

REPRODUCTION STEPS (for bugs)

1. [Step]
2. [Step]
3. [Step]
   Expected: [X]
   Actual: [Y]
   Environment: [Details]

WHAT'S BEEN TRIED

1. [Action] -> [Result]
2. [Action] -> [Result]
3. [Action] -> [Result]

CUSTOMER COMMUNICATION

- Last update: [Date — what was said]
- Customer expectation: [What they expect and by when]
- Escalation risk: [Will they escalate further?]

WHAT'S NEEDED

- [Specific ask: investigate, fix, decide, approve]
- Deadline: [Date/time]
```
