---
name: product-marketing-context
description: Build and maintain the shared product marketing context used by other marketing skills. Use when setting positioning, ICP, messaging pillars, voice rules, proof points, or when other marketing tasks need foundational context. Produce file-first outputs under scripts/marketing/<slug>/ and update scripts/marketing/_shared/product-marketing-context.md.
---

# Product Marketing Context

Create a durable marketing context baseline that other skills can reuse.

## Required Inputs

- Product or company name
- Primary audience
- Main conversion goal
- Campaign slug (or derive one)

## Workflow

1. Check for `scripts/marketing/_shared/product-marketing-context.md`. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. If it exists, audit for staleness and gaps. If older than 90 days, mark as stale.
3. Collect evidence from workspace docs first (`README`, landing copy, changelogs, testimonials).
4. If web browsing is available, research current positioning signals and cite sources.
5. If web browsing is unavailable, create an explicit fallback assumptions file.
6. Draft or refresh context sections: positioning, ICP, JTBD, pain points, differentiation, objections, proof, voice, terms to use/avoid.
7. Write outputs and include unresolved questions.

## Output Files

- `scripts/marketing/<slug>/00-product-context.md`
- `scripts/marketing/<slug>/00-open-questions.md`
- `scripts/marketing/<slug>/00-sources.md`
- `scripts/marketing/<slug>/NO-WEB-FALLBACK.md` (only when web is unavailable)
- `scripts/marketing/_shared/product-marketing-context.md` (canonical shared context)

## QA Checklist

- Context reflects current product and audience.
- Differentiation includes evidence, not claims without proof.
- Voice and terminology rules are explicit and actionable.
- Open questions are separated from confirmed facts.
- Sources are listed for external statements.

## Failure Modes

- Missing audience specificity: narrow ICP and JTBD before finalizing.
- Generic messaging: replace with customer language and concrete proof.
- No evidence for claims: downgrade confidence and document assumptions.

## Next Skill Routing

- Run `content-strategy` for planning.
- Run `copywriting` for first drafts.
- Run `seo-audit` for organic discovery priorities.
