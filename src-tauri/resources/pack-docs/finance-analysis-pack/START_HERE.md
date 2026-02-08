# Getting Started with Finance Analysis Pack

## Prerequisites

Ensure you have the following Python libraries installed:

```bash
pip install pandas numpy openpyxl
```

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
