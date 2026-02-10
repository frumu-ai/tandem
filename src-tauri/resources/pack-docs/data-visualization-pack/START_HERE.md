# Getting Started with Data Visualization Pack

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

1. **Choose Your Library**:
   - Use `matplotlib_standard.py` for fine-grained control and print media.
   - Use `seaborn_advanced.py` for statistical analysis and quick, beautiful plots.
   - Use `plotly_interactive.py` for web interaction and exploration.

2. **Load Your Data**:
   - Each template contains a "Setup Data" section.
   - Replace the sample data generation with your own data loading logic (e.g., `pd.read_csv()`, `pd.read_sql()`).

3. **Customize**:
   - Adjust titles, labels, and colors in the "Configuration" sections.
   - The templates use professional style defaults, but you can override them to match your brand guidelines.

4. **Run**:
   - Execute the script to generate and view the visualization.
   - Uncomment the `savefig` or `write_html` lines to save the output to a file.
