---
name: web-starter-audit
description: "Run a full UX, accessibility, and quality audit for starter web projects and produce prioritized remediation."
version: "0.1.0"
author: tandem
compatibility: tandem
tags:
  - web
  - audit
  - quality
requires:
  - html
  - css
  - javascript
triggers:
  - accessibility audit
  - web audit
  - qa pass
---

# Web Starter Audit Skill

## Mission

Audit a web starter project and deliver actionable, prioritized fixes with verification notes.

## Inputs To Use First

- src/ and inputs/src/
- inputs/TODO.md

## Workflow

1. Scan structure and identify user-critical flows.
2. Audit accessibility: semantics, focus, labels, contrast, keyboard nav.
3. Audit quality: bugs, unsafe assumptions, dead code, poor defaults.
4. Propose prioritized remediation with effort and impact.
5. Produce updated report artifacts.

## Required Outputs

- outputs/audit_findings.md
- outputs/remediation_plan.md
- outputs/changelog.md
- outputs/project_audit_report.html

## Quality Bar

- Findings include severity and reproduction guidance.
- Fixes are scoped and sequenced realistically.
- Accessibility issues map to concrete standards.
- Report is understandable by engineering and product.
