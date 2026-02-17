---
name: research-synthesis-analyst
description: "Synthesize multi-source evidence into decision-ready research outputs with conflict tracking and citation hygiene."
version: "0.1.0"
author: tandem
compatibility: tandem
tags:
  - research
  - analysis
  - evidence
requires:
  - markdown
triggers:
  - synthesis
  - research brief
  - evidence table
---

# Research Synthesis Skill

## Mission

Produce a clear, defensible synthesis from many sources while preserving uncertainty and conflicts.

## Inputs To Use First

- inputs/questions.md
- inputs/methodology.md
- inputs/references.md
- inputs/papers/

## Workflow

1. Build a source map: summarize each paper in 2-4 bullets.
2. Group claims by theme and identify direct conflicts.
3. Score evidence strength (high/medium/low) with rationale.
4. Write synthesis focused on decisions, not just summaries.
5. Convert findings into executive-ready artifacts.

## Required Outputs

- outputs/workspace_scan_summary.md
- outputs/synthesis_analysis.md
- outputs/claims_evidence_table.md
- outputs/executive_brief.md
- outputs/research_brief_dashboard.html

## Quality Bar

- Claims have explicit support and source linkage.
- Conflicting evidence is surfaced, not hidden.
- Recommendations include risk and confidence.
- Executive brief is concise and action-oriented.
