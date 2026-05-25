---
name: social-content
description: Create channel-specific social content plans and ready-to-post drafts tied to business goals. Use for LinkedIn, X, Instagram, TikTok, and similar channels when planning cadence, post formats, and engagement routines. Produce deterministic outputs under scripts/marketing/<slug>/ and require web research when available.
---

# Social Content

Turn strategy into scheduled social execution.

## Required Inputs

- Campaign slug
- Priority channels
- Goal (awareness, traffic, leads, community)
- Brand account type (personal, company, or both)

## Workflow

1. Read `scripts/marketing/_shared/product-marketing-context.md` if present. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. Define content pillars and channel mix.
3. If web browsing is available, verify current platform patterns and cite sources.
4. If web browsing is unavailable, create fallback assumptions.
5. Build a 30-day posting calendar with clear cadence.
6. Produce ready-to-post drafts and engagement routines.
7. Add measurement plan and weekly optimization loop.

## Output Files

- `scripts/marketing/<slug>/14-social-calendar-30d.md`
- `scripts/marketing/<slug>/15-ready-posts.md`
- `scripts/marketing/<slug>/16-engagement-playbook.md`
- `scripts/marketing/<slug>/16-metrics-plan.md`
- `scripts/marketing/<slug>/14-sources.md`
- `scripts/marketing/<slug>/NO-WEB-FALLBACK.md` (only when web is unavailable)

## QA Checklist

- Calendar aligns to capacity and goals.
- Posts are channel-native, not copy-pasted clones.
- Hooks, CTA, and format are explicit for each draft.
- Engagement routine is actionable and time-bounded.

## Failure Modes

- Over-production: reduce volume and preserve consistency.
- Low channel fit: adapt format to platform norms.
- Weak conversion path: add clear CTA bridge to owned channels.

## Next Skill Routing

- Run `copywriting` for long-form source content.
- Run `email-sequence` to nurture social-origin leads.
- Run `launch-strategy` for coordinated release campaigns.
