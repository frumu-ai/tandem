---
name: bio-nextflow-manager
description: Run nf-core bioinformatics pipelines (rnaseq, sarek, atacseq) on sequencing data. Use when analyzing RNA-seq, WGS/WES, or ATAC-seq data—either local FASTQs or public datasets from GEO/SRA. Triggers on nf-core, Nextflow, FASTQ analysis, variant calling, gene expression, differential expression, GEO reanalysis, GSE/GSM/SRR accessions, or samplesheet creation.
requires: [python, bash]
---

# nf-core Pipeline Deployment

Run nf-core bioinformatics pipelines on local or public sequencing data.

> **Note:** This skill utilizes the **Bio-Informatics Pack**.
> Scripts and references are located in: `src-tauri/resources/packs/bio-informatics-pack/nextflow-pipelines/`

**Target users:** Bench scientists and researchers without specialized bioinformatics training who need to run large-scale omics analyses.

## Workflow Checklist

```
- [ ] Step 0: Acquire data (if from GEO/SRA)
- [ ] Step 1: Environment check (MUST pass)
- [ ] Step 2: Select pipeline (confirm with user)
- [ ] Step 3: Run test profile (MUST pass)
- [ ] Step 4: Create samplesheet
- [ ] Step 5: Configure & run (confirm genome with user)
- [ ] Step 6: Verify outputs
```

---

## Step 0: Acquire Data (GEO/SRA Only)

**Skip this step if user has local FASTQ files.**

For public datasets, fetch from GEO/SRA first. See pack's `references/geo-sra-acquisition.md`.

**Quick start:**

```bash
# Set path to pack scripts
$PACK_SCRIPTS = "src-tauri/resources/packs/bio-informatics-pack/nextflow-pipelines/scripts"

# 1. Get study info
python $PACK_SCRIPTS/sra_geo_fetch.py info GSE110004

# 2. Download (interactive mode)
python $PACK_SCRIPTS/sra_geo_fetch.py download GSE110004 -o ./fastq -i

# 3. Generate samplesheet
python $PACK_SCRIPTS/sra_geo_fetch.py samplesheet GSE110004 --fastq-dir ./fastq -o samplesheet.csv
```

**DECISION POINT:** After fetching study info, confirm with user:

- Which sample subset to download (if multiple data types)
- Suggested genome and pipeline

Then continue to Step 1.

---

## Step 1: Environment Check

**Run first. Pipeline will fail without passing environment.**

```bash
python src-tauri/resources/packs/bio-informatics-pack/nextflow-pipelines/scripts/check_environment.py
```

All critical checks must pass. If any fail, provide fix instructions (Docker, Nextflow, Java).

---

## Step 2: Select Pipeline

**DECISION POINT: Confirm with user before proceeding.**

| Data Type | Pipeline  | Goal                    |
| --------- | --------- | ----------------------- |
| RNA-seq   | `rnaseq`  | Gene expression         |
| WGS/WES   | `sarek`   | Variant calling         |
| ATAC-seq  | `atacseq` | Chromatin accessibility |

Auto-detect from data:

```bash
python src-tauri/resources/packs/bio-informatics-pack/nextflow-pipelines/scripts/detect_data_type.py /path/to/data
```
