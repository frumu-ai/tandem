# Getting Started with Bio-Informatics Pack

## Prerequisites

This pack uses Python-heavy workflows. Use the workspace virtual environment so
dependencies stay isolated.

1. Open Tandem's **Python Setup (Workspace Venv)** wizard.
2. Create the venv in the workspace.
3. Install dependencies used by your selected scripts.

### Windows

```bash
cd "<your-pack-folder>"
.tandem\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

### macOS / Linux

```bash
cd "<your-pack-folder>"
.tandem/.venv/bin/python3 -m pip install -r requirements.txt
```

If a script requires additional tools (for example Nextflow), follow the script
or reference notes before running.

## How to Use

1. **Pick your path**:
   - Conversion-first: start in `allotrope-conversion/`
   - Pipeline-first: start in `nextflow-pipelines/`
   - Analysis-first: start in `single-cell-analysis/`

2. **Read references first**:
   - Review the corresponding `references/` docs for assumptions and expected inputs.

3. **Run scripts in stages**:
   - Environment check
   - Data preparation/conversion
   - Pipeline execution
   - QC and summary generation

4. **Write outputs under `outputs/`**:
   - Keep logs, QC summaries, and final conclusions together for easy review.
