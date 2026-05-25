---
name: finance-analysis-analyst
description: "Drive repeatable finance variance analysis with clear assumptions, reconciliations, and executive summaries."
version: "0.1.0"
author: tandem
compatibility: tandem
tags:
  - finance
  - analysis
  - reporting
requires:
  - python
triggers:
  - variance analysis
  - financial summary
  - budget vs actual
---

# Finance Analysis Skill

## Mission

Turn financial inputs into reliable variance analysis and executive-ready recommendations.

## Inputs To Use First

- inputs/sample_financial_data.csv
-     emplates/income_statement_generator.py
-     emplates/variance_analysis_template.py
- equirements.txt

## Workflow

1. Validate schema, period coverage, and currency consistency.
2. Reconcile totals and isolate material variances.
3. Explain key drivers by category and period.
4. Generate concise management commentary.
5. Publish report artifacts for review.

## Required Outputs

- outputs/data_reconciliation_notes.md
- outputs/variance_analysis.md
- outputs/executive_finance_summary.md
- outputs/finance_dashboard.html

## Quality Bar

- Variances are quantified with clear baselines.
- Assumptions and data gaps are disclosed.
- Recommendations are specific and measurable.
- Summary is ready for leadership consumption.
