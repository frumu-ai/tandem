---
name: content-strategy
description: Create a practical content strategy that connects searchable and shareable content to business goals. Use when planning topics, clusters, editorial calendar, repurposing, and channel priorities. Produce deterministic file outputs under scripts/marketing/<slug>/ and require web research when available.
---

# Content Strategy

Plan a realistic, repeatable content system for non-developer teams.

## Required Inputs

- Campaign slug
- Primary growth goal (traffic, leads, activation, awareness)
- Target audience and funnel stage focus
- Capacity constraints (time, team, budget)

## Workflow

1. Read `scripts/marketing/_shared/product-marketing-context.md` if present. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. Inventory current content assets and known performance signals.
3. If web browsing is available, gather current search and social demand signals with sources.
4. If web browsing is unavailable, generate a fallback assumptions file.
5. Build topic clusters by buyer stage.
6. Prioritize with Impact x Effort x Confidence scoring.
7. Produce a 30-day calendar plus repurposing plan.

## Output Files

- `scripts/marketing/<slug>/01-topic-clusters.md`
- `scripts/marketing/<slug>/02-editorial-calendar-30d.md`
- `scripts/marketing/<slug>/03-repurpose-map.md`
- `scripts/marketing/<slug>/01-priority-matrix.csv`
- `scripts/marketing/<slug>/01-sources.md`
- `scripts/marketing/<slug>/NO-WEB-FALLBACK.md` (only when web is unavailable)

## QA Checklist

- Every planned asset has a goal, audience, and distribution channel.
- Content balance includes searchable and shareable formats.
- Capacity assumptions are realistic.
- Prioritization logic is explicit.
- External trends and claims are sourced.

## Failure Modes

- Over-scoped calendar: reduce scope to the highest-leverage 30 days.
- Topic drift from product value: realign to context doc.
- No distribution plan: require at least one channel per asset.

## Next Skill Routing

- Run `seo-audit` to validate organic opportunities.
- Run `social-content` for distribution copy and cadence.
- Run `copywriting` for long-form production.
