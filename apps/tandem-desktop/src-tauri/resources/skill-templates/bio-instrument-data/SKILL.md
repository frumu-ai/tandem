---
name: bio-instrument-data
description: Convert laboratory instrument output files (PDF, CSV, Excel, TXT) to Allotrope Simple Model (ASM) JSON format or flattened 2D CSV. Use this skill when scientists need to standardize instrument data for LIMS systems, data lakes, or downstream analysis. Supports auto-detection of instrument types. Outputs include full ASM JSON, flattened CSV for easy import, and exportable Python code for data engineers. Common triggers include converting instrument files, standardizing lab data, preparing data for upload to LIMS/ELN systems, or generating parser code for production pipelines.
requires: [python, bash]
---

# Instrument Data to Allotrope Converter

Convert instrument files into standardized Allotrope Simple Model (ASM) format for LIMS upload, data lakes, or handoff to data engineering teams.

> **Note:** This skill utilizes the **Bio-Informatics Pack**.
> Scripts and references are located in: `src-tauri/resources/packs/bio-informatics-pack/allotrope-conversion/`

## Workflow Overview

1. **Detect instrument type** from file contents (auto-detect or user-specified)
2. **Parse file** using allotropy library (native) or flexible fallback parser
3. **Generate outputs**:
   - ASM JSON (full semantic structure)
   - Flattened CSV (2D tabular format)
   - Python parser code (for data engineer handoff)
4. **Deliver** files with summary and usage instructions

> **When Uncertain:** If you're unsure how to map a field to ASM (e.g., is this raw data or calculated?), ask the user for clarification. Refer to the pack's `references/field_classification_guide.md` for guidance.

## Quick Start

```python
# Install requirements first
pip install allotropy pandas openpyxl pdfplumber --break-system-packages

# Core conversion
from allotropy.parser_factory import Vendor
from allotropy.to_allotrope import allotrope_from_file

# Convert with allotropy
asm = allotrope_from_file("instrument_data.csv", Vendor.BECKMAN_VI_CELL_BLU)
```

## Output Format Selection

**ASM JSON (default)** - Full semantic structure with ontology URIs

- Best for: LIMS systems expecting ASM, data lakes, long-term archival
- Validates against Allotrope schemas

**Flattened CSV** - 2D tabular representation

- Best for: Quick analysis, Excel users, systems without JSON support
- Each measurement becomes one row with metadata repeated

**Both** - Generate both formats for maximum flexibility

## Calculated Data Handling

**IMPORTANT:** Separate raw measurements from calculated/derived values.

- **Raw data** → `measurement-document` (direct instrument readings)
- **Calculated data** → `calculated-data-aggregate-document` (derived values)

Calculated values MUST include traceability via `data-source-aggregate-document`.

## Validation

Always validate ASM output before delivering to the user:

```bash
# Run validation script from the pack
python src-tauri/resources/packs/bio-informatics-pack/allotrope-conversion/scripts/validate_asm.py output.json
```
