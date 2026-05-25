---
name: bio-single-cell
description: Deep learning for single-cell analysis using scvi-tools and scverse ecosystem. This skill should be used when users need (1) data integration and batch correction with scVI/scANVI, (2) ATAC-seq analysis with PeakVI, (3) CITE-seq multi-modal analysis with totalVI, (4) multiome RNA+ATAC analysis with MultiVI, (5) spatial transcriptomics deconvolution with DestVI, (6) label transfer and reference mapping, (7) RNA velocity with veloVI, or (8) QC analysis of single-cell RNA-seq data. Triggers include scVI, scANVI, totalVI, QC, quality control, batch correction, integration, multi-modal.
requires: [python, bash]
---

# scvi-tools Deep Learning & QC Skill

This skill provides guidance for deep learning-based single-cell analysis using scvi-tools and standard QC workflows.

> **Note:** This skill utilizes the **Bio-Informatics Pack**.
> Scripts and references are located in: `src-tauri/resources/packs/bio-informatics-pack/single-cell-analysis/`

## How to Use This Skill

1. Identify the appropriate workflow (QC or Modeling)
2. Use scripts in the pack's `scripts/` folder
3. For installation or GPU issues, consult `references/environment_setup.md` in the pack

## QC Workflow (Run First)

Before running any deep learning models, ensure data quality.

```bash
# Run standard QC analysis
python src-tauri/resources/packs/bio-informatics-pack/single-cell-analysis/scripts/qc_analysis.py input.h5ad output_qc.h5ad
```

See `references/scverse_qc_guidelines.md` for detailed metrics thresholds.

## Model Selection Guide

| Data Type                 | Model       | Primary Use Case                            |
| ------------------------- | ----------- | ------------------------------------------- |
| scRNA-seq                 | **scVI**    | Unsupervised integration, DE, imputation    |
| scRNA-seq + labels        | **scANVI**  | Label transfer, semi-supervised integration |
| CITE-seq (RNA+protein)    | **totalVI** | Multi-modal integration, protein denoising  |
| scATAC-seq                | **PeakVI**  | Chromatin accessibility analysis            |
| Multiome (RNA+ATAC)       | **MultiVI** | Joint modality analysis                     |
| Spatial + scRNA reference | **DestVI**  | Cell type deconvolution                     |
| RNA velocity              | **veloVI**  | Transcriptional dynamics                    |
| Cross-technology          | **sysVI**   | System-level batch correction               |

## CLI Scripts

Modular scripts for common workflows. Chain together or modify as needed.

### Pipeline Scripts

Scripts are located at `src-tauri/resources/packs/bio-informatics-pack/single-cell-analysis/scripts/`.

| Script                       | Purpose                    | Usage                                                                         |
| ---------------------------- | -------------------------- | ----------------------------------------------------------------------------- |
| `prepare_data.py`            | QC, filter, HVG selection  | `python prepare_data.py raw.h5ad prepared.h5ad --batch-key batch`             |
| `train_model.py`             | Train any scvi-tools model | `python train_model.py prepared.h5ad results/ --model scvi`                   |
| `cluster_embed.py`           | Neighbors, UMAP, Leiden    | `python cluster_embed.py adata.h5ad results/`                                 |
| `differential_expression.py` | DE analysis                | `python differential_expression.py model/ adata.h5ad de.csv --groupby leiden` |
| `transfer_labels.py`         | Label transfer with scANVI | `python transfer_labels.py ref_model/ query.h5ad results/`                    |
| `integrate_datasets.py`      | Multi-dataset integration  | `python integrate_datasets.py results/ data1.h5ad data2.h5ad`                 |
| `validate_adata.py`          | Check data compatibility   | `python validate_adata.py data.h5ad --batch-key batch`                        |

### Example Workflow

```bash
# Set script path
$SC_SCRIPTS = "src-tauri/resources/packs/bio-informatics-pack/single-cell-analysis/scripts"

# 1. Validate input data
python $SC_SCRIPTS/validate_adata.py raw.h5ad --batch-key batch --suggest

# 2. Prepare data (QC, HVG selection)
python $SC_SCRIPTS/prepare_data.py raw.h5ad prepared.h5ad --batch-key batch --n-hvgs 2000

# 3. Train model
python $SC_SCRIPTS/train_model.py prepared.h5ad results/ --model scvi --batch-key batch

# 4. Cluster and visualize
python $SC_SCRIPTS/cluster_embed.py results/adata_trained.h5ad results/ --resolution 0.8
```
