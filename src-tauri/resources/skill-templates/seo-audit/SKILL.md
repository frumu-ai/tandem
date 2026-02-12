---
name: seo-audit
description: Audit technical and on-page SEO issues, prioritize fixes, and produce a ranked action backlog. Use for ranking drops, low organic growth, indexation concerns, and page optimization reviews. Output file-first artifacts under scripts/marketing/<slug>/ and use web research when available.
---

# SEO Audit

Diagnose SEO issues and convert findings into an execution backlog.

## Required Inputs

- Campaign slug
- Target domain or page set
- Priority keywords or topic areas
- Access notes (Search Console, analytics, crawl exports if available)

## Workflow

1. Read `scripts/marketing/_shared/product-marketing-context.md` if present. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. Define audit scope (full site or selected pages).
3. Evaluate crawlability, indexation, technical health, on-page quality, internal linking, and SERP intent match.
4. If web browsing is available, validate competitor SERP patterns and cite sources.
5. If web browsing is unavailable, create fallback assumptions.
6. Score each issue with Severity x Effort x Business Impact.
7. Convert findings into a prioritized backlog with owners and sequencing.

## Output Files

- `scripts/marketing/<slug>/04-seo-audit-report.md`
- `scripts/marketing/<slug>/05-priority-fixes.md`
- `scripts/marketing/<slug>/06-seo-backlog.csv`
- `scripts/marketing/<slug>/04-sources.md`
- `scripts/marketing/<slug>/NO-WEB-FALLBACK.md` (only when web is unavailable)

## QA Checklist

- Findings include evidence, impact, and concrete fix guidance.
- Priority list separates quick wins from structural fixes.
- No recommendation depends on unsourced external claims.
- Backlog fields are implementation-ready.

## Failure Modes

- Audit too broad: split into technical and content passes.
- Weak prioritization: enforce numeric scoring.
- Missing business tie-in: map each issue to traffic or conversion impact.

## Next Skill Routing

- Run `competitor-alternatives` for bottom-funnel SEO pages.
- Run `copywriting` for page rewrites.
- Run `social-content` to distribute refreshed content.
