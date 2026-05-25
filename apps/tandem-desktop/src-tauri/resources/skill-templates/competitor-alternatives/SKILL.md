---
name: competitor-alternatives
description: Plan and draft competitor comparison and alternative-page strategies for SEO and sales evaluation use cases. Use for "[competitor] alternatives", "you vs competitor", and comparison-page content architecture. Produce deterministic outputs under scripts/marketing/<slug>/ with verified claim handling.
---

# Competitor Alternatives

Create trustworthy comparison assets that drive qualified demand.

## Required Inputs

- Campaign slug
- Target competitor set
- Desired page type (alternative, vs, alternatives list)
- Primary audience segment

## Workflow

1. Read `scripts/marketing/_shared/product-marketing-context.md` if present. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. Define comparison intent and page format.
3. Gather internal truth for your product strengths and limits.
4. If web browsing is available, verify competitor claims and cite sources.
5. If web browsing is unavailable, create fallback assumptions and caution flags.
6. Build comparison narrative beyond feature tables.
7. Produce outline, proof requirements, and recommendation logic by use case.

## Output Files

- `scripts/marketing/<slug>/07-competitor-briefs.md`
- `scripts/marketing/<slug>/08-page-outline-vs-and-alt.md`
- `scripts/marketing/<slug>/09-claim-verification-table.csv`
- `scripts/marketing/<slug>/09-proof-required.md`
- `scripts/marketing/<slug>/07-sources.md`
- `scripts/marketing/<slug>/NO-WEB-FALLBACK.md` (only when web is unavailable)

## QA Checklist

- Competitor claims are sourced and date-stamped.
- Your product limitations are acknowledged honestly.
- Recommendations are audience/use-case specific.
- CTA path is clear for readers ready to switch.

## Failure Modes

- Unverified competitor data: mark as unverified and avoid hard claims.
- Biased framing: add balanced "best-for" guidance.
- Thin content: include scenarios and migration notes.

## Next Skill Routing

- Run `copywriting` to draft final page copy.
- Run `seo-audit` after publish for optimization feedback.
