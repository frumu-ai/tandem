---
name: web-research-refresh
description: "Refresh stale content via web verification, source logging, and citation-backed updates."
version: "0.1.0"
author: tandem
compatibility: tandem
tags:
  - web
  - research
  - documentation
requires:
  - web
triggers:
  - fact check
  - refresh docs
  - verify claims
---

# Web Research Refresh Skill

## Mission

Replace stale claims with verified, current facts and maintain a transparent evidence trail.

## Inputs To Use First

- inputs/stale_brief.md
- inputs/verification_questions.md
- inputs/customer_support_tickets.md

## Workflow

1. Extract factual claims and mark each as verify/update/remove.
2. Prioritize high-risk claims (policy, pricing, legal, security).
3. Research authoritative sources and log each citation.
4. Rewrite facts with dates and references.
5. Publish clear before/after reporting.

## Required Outputs

- outputs/claims_inventory.md
- outputs/research_plan.md
- outputs/evidence_log.md
- outputs/updated_facts_sheet.md
- outputs/web_research_report.html

## Quality Bar

- Every factual update has a source URL and access date.
- Low-trust sources are explicitly flagged or excluded.
- Changes are traceable from original claim to new fact.
- Final report is readable by non-technical stakeholders.
