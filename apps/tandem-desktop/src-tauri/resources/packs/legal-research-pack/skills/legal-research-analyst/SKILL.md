---
name: legal-research-analyst
description: "Perform structured legal document analysis, issue spotting, and memo drafting with clear risk framing."
version: "0.1.0"
author: tandem
compatibility: tandem
tags:
  - legal
  - contracts
  - analysis
requires:
  - markdown
triggers:
  - contract review
  - legal memo
  - risk matrix
---

# Legal Research Skill

## Mission

Analyze legal materials with consistent issue spotting, risk ranking, and concise legal writing.

## Inputs To Use First

- inputs/NDA_TEMPLATE.md
- inputs/EMPLOYMENT_AGREEMENT_DRAFT.md
- inputs/CASE_NOTES_SMITH_V_JONES.md
- inputs/LEGAL_RESEARCH_MEMO_TEMPLATE.md

## Workflow

1. Extract clauses, obligations, and ambiguous language.
2. Build a risk matrix by impact and likelihood.
3. Summarize relevant case notes and applicability.
4. Draft memo with findings, options, and recommended edits.
5. Package outputs for legal or operator review.

## Required Outputs

- outputs/contract_risk_matrix.md
- outputs/case_summary.md
- outputs/legal_memorandum.md
- outputs/litigation_dashboard.html

## Quality Bar

- Risks are tied to specific clause text.
- Assumptions and unknowns are explicitly listed.
- Recommended edits are concrete and minimal.
- Memo is decision-oriented and plain-language.
