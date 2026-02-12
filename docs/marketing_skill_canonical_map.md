# Marketing Skill Canonical Map

This document defines the no-duplicate marketing skill strategy for Tandem.

## Core 9 Canonical Skills

Use these as the default path for marketing workflows:

1. `product-marketing-context`
2. `content-strategy`
3. `seo-audit`
4. `social-content`
5. `copywriting`
6. `copy-editing`
7. `email-sequence`
8. `competitor-alternatives`
9. `launch-strategy`

## Routing Rules

- For SEO diagnosis and prioritization: use `seo-audit`.
- For social planning and ready-to-post drafts: use `social-content`.
- For lifecycle email planning and draft copy: use `email-sequence`.
- For page and campaign first drafts: use `copywriting`.
- For final quality sweeps: use `copy-editing`.
- For comparison and alternative pages: use `competitor-alternatives`.
- For launch planning and phased execution: use `launch-strategy`.
- For baseline marketing context and terminology: use `product-marketing-context`.
- For editorial planning and content prioritization: use `content-strategy`.

## Legacy/Fallback Templates (Keep, Do Not Lead)

These remain available but are secondary for day-to-day workflows:

- `marketing-content-creation`: broad fallback only. Route operational work to `copywriting`, `copy-editing`, `social-content`, and `email-sequence`.
- `marketing-campaign-planning`: campaign PM structure fallback. Route launch execution to `launch-strategy`.
- `marketing-brand-voice`: governance fallback. Route production drafting to `copywriting` and `copy-editing`.
- `marketing-competitive-analysis`: broad competitive intel fallback. Route competitor page execution to `competitor-alternatives`.
- `marketing-research-posting-plan`: heavy file-output research fallback. Route routine planning to `content-strategy`, `social-content`, and `seo-audit`.

## Quality Standard (Canonical Skills)

Each canonical skill should follow these constraints:

1. File-first outputs under `scripts/marketing/<slug>/...`
2. Required web research when available, with explicit no-web fallback file
3. Deterministic sections:
   - Required Inputs
   - Workflow
   - Output Files
   - QA Checklist
   - Failure Modes
   - Next Skill Routing

## Validation Scenarios

1. Discovery: canonical skills are recommended before legacy marketing templates.
2. Routing:
   - "SEO audit" -> `seo-audit`
   - "LinkedIn posts" -> `social-content`
   - "welcome drip" -> `email-sequence`
3. Repeatability: each canonical skill produces deterministic artifact files.
4. No-duplicate UX: legacy templates are marked fallback in docs and guidance.
