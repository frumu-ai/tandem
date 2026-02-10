# Getting Started with Finance Analysis Pack

## Prerequisites

This pack uses Python. To keep installs safe and reproducible, use Tandem's workspace venv:

1. Open Tandem's **Python Setup (Workspace Venv)** wizard.
2. Click **Create venv in workspace** (creates `.opencode/.venv`).
3. Install dependencies into the venv:

### Windows

```bash
cd "<your-pack-folder>"
.opencode\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

### macOS / Linux

```bash
cd "<your-pack-folder>"
.opencode/.venv/bin/python3 -m pip install -r requirements.txt
```

If you already ran `pip install ...` globally by accident, you can still continue, but future runs should use the workspace venv.

## How to Use

1. **Prepare Your Data**:
   - Ensure your data is in a CSV or Excel format.
   - For the Income Statement, you need columns for Account, Sub-Account, and Amount.
   - For Variance Analysis, you need columns for Category (e.g., Department), Actual Amount, and Budget Amount.

2. **Select a Template**:
   - Open `templates/income_statement_generator.py` for P&L generation.
   - Open `templates/variance_analysis_template.py` for budget comparison.

3. **Configure the Script**:
   - Update the `pd.read_csv()` lines to point to your data file.
   - Adjust the `structure` dictionary in the Income Statement script to match your chart of accounts.
   - Set your variance thresholds (e.g., +/- 10%) in the Variance Analysis script.

4. **Run and Export**:
   - Run the script to see the report in the console.
   - Uncomment the `.to_excel()` lines to save the report as an Excel file.
