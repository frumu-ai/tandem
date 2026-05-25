---
name: bio-informatics-analyst
description: "Coordinate bioinformatics data conversion, pipeline setup, and analysis reporting with reproducible workflow checkpoints."
version: "0.1.0"
author: tandem
compatibility: tandem
tags:
  - bioinformatics
  - pipelines
  - analysis
requires:
  - python
  - nextflow
triggers:
  - single cell
  - pipeline
  - bioinformatics
---

# Bio-Informatics Skill

## Mission

Run a reproducible bioinformatics workflow from source preparation to analysis-ready deliverables.

## Inputs To Use First

- allotrope-conversion/
- nextflow-pipelines/
- single-cell-analysis/
- scripts/

## Workflow

1. Validate environment and required tooling.
2. Classify input data and select appropriate pipeline path.
3. Execute conversion/prep steps with QC checkpoints.
4. Run analysis workflow and capture parameters.
5. Summarize outputs, caveats, and next experiments.

## Required Outputs

- outputs/environment_checklist.md
- outputs/pipeline_execution_log.md
- outputs/qc_summary.md
- outputs/analysis_findings.md
- outputs/bioinformatics_report.html

## Quality Bar

- Every run is reproducible with exact config references.
- QC thresholds and failures are explicitly documented.
- Biological conclusions are separated from technical artifacts.
- Follow-up experiments are prioritized by confidence and impact.
