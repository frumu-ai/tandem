---
name: copywriting
description: Draft conversion-focused marketing copy for landing pages, homepages, pricing pages, feature pages, and campaign assets. Use when creating first drafts from positioning and audience context. Output file-first drafts under scripts/marketing/<slug>/ with explicit proof and CTA requirements.
---

# Copywriting

Create high-clarity drafts that are ready for structured editing.

## Required Inputs

- Campaign slug
- Asset type (landing, pricing, feature, email, social long-form)
- Target audience segment
- Primary conversion action

## Workflow

1. Read `scripts/marketing/_shared/product-marketing-context.md` if present. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. Confirm one primary page or asset objective.
3. Pull supporting evidence from internal sources (metrics, testimonials, case studies).
4. If web browsing is available and external references are needed, collect and cite them.
5. If web browsing is unavailable, create fallback assumptions.
6. Draft copy by section with one argument per section.
7. Generate 2-3 headline and CTA variants.

## Output Files

- `scripts/marketing/<slug>/10-copy-drafts.md`
- `scripts/marketing/<slug>/11-headline-and-cta-variants.md`
- `scripts/marketing/<slug>/10-proof-required.md`
- `scripts/marketing/<slug>/10-sources.md`
- `scripts/marketing/<slug>/NO-WEB-FALLBACK.md` (only when web is unavailable)

## QA Checklist

- Clear value proposition in first screen/section.
- Claims are specific and traceable to evidence.
- Copy uses customer language, not internal jargon.
- CTA is singular and action-oriented.

## Failure Modes

- Generic copy: add specificity and concrete outcomes.
- Feature-heavy draft: convert features to benefits and use cases.
- Unsupported claims: move to proof-needed list.

## Next Skill Routing

- Run `copy-editing` for multi-pass quality sweep.
- Run `social-content` to repurpose into distribution posts.
