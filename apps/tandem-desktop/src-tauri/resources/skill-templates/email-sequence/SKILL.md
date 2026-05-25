---
name: email-sequence
description: Design and draft lifecycle email sequences such as welcome, onboarding, nurture, re-engagement, and upgrade flows. Use when defining triggers, sequence maps, email copy, and performance metrics. Produce file-first outputs under scripts/marketing/<slug>/ and align with product marketing context.
---

# Email Sequence

Build practical lifecycle flows with clear goals and triggers.

## Required Inputs

- Campaign slug
- Sequence type (welcome, onboarding, nurture, re-engagement, upgrade)
- Entry trigger
- Primary conversion objective

## Workflow

1. Read `scripts/marketing/_shared/product-marketing-context.md` if present. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. Define audience state at sequence entry.
3. Map sequence steps with one job per email.
4. Draft each email with subject, preview, body, and one CTA.
5. Add branching logic for key user states when needed.
6. Define metrics and guardrails for sequence quality.

## Output Files

- `scripts/marketing/<slug>/17-email-sequence-map.md`
- `scripts/marketing/<slug>/18-email-copy.md`
- `scripts/marketing/<slug>/19-email-metrics-plan.md`
- `scripts/marketing/<slug>/19-email-qa-checklist.md`

## QA Checklist

- Sequence objective and stop conditions are explicit.
- Each email has one primary job and one main CTA.
- Timing and cadence are realistic for audience context.
- Metrics include open, click, conversion, and negative guardrails.

## Failure Modes

- Too many asks per email: reduce to one primary action.
- Sequence fatigue risk: increase spacing or shorten flow.
- Misaligned stage messaging: rewrite based on entry trigger state.

## Next Skill Routing

- Run `copy-editing` for final polish.
- Run `launch-strategy` when sequence supports a release campaign.
