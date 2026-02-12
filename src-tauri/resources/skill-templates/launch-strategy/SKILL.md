---
name: launch-strategy
description: Plan phased product and feature launches across owned, rented, and borrowed channels. Use when coordinating prelaunch, launch-day, and postlaunch execution with clear owners and assets. Produce deterministic runbooks under scripts/marketing/<slug>/.
---

# Launch Strategy

Turn launch ideas into an execution-ready operating plan.

## Required Inputs

- Campaign slug
- Launch type (feature, product, pricing, partnership)
- Target audience and launch objective
- Team capacity and timeline constraints

## Workflow

1. Read `scripts/marketing/_shared/product-marketing-context.md` if present. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. Define launch objective, success metrics, and go/no-go criteria.
3. Build phased plan: prelaunch, launch, postlaunch.
4. Align channels by role (owned, rented, borrowed).
5. If web browsing is available, validate current platform/community tactics and cite sources.
6. If web browsing is unavailable, create fallback assumptions.
7. Produce day-by-day runbook, asset checklist, and risk mitigations.

## Output Files

- `scripts/marketing/<slug>/20-launch-plan.md`
- `scripts/marketing/<slug>/21-launch-asset-checklist.md`
- `scripts/marketing/<slug>/22-day-by-day-runbook.md`
- `scripts/marketing/<slug>/22-risk-register.csv`
- `scripts/marketing/<slug>/20-sources.md`
- `scripts/marketing/<slug>/NO-WEB-FALLBACK.md` (only when web is unavailable)

## QA Checklist

- Plan includes prelaunch, launch-day, and postlaunch actions.
- Owners and deadlines are assigned for each major task.
- Channel tactics map to audience and objective.
- Risks, dependencies, and fallback paths are explicit.

## Failure Modes

- Launch too broad: narrow to one core message and one primary CTA.
- Weak postlaunch plan: include 2-4 weeks of follow-through actions.
- Asset bottleneck: prioritize must-have assets first.

## Next Skill Routing

- Run `social-content` for launch distribution assets.
- Run `email-sequence` for nurture and follow-up flows.
