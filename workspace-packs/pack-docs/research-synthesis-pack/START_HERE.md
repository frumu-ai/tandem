# Getting Started - Research Synthesis Pack

## Quick Start (10 Minutes)

### Step 1: Open Tandem

Launch the Tandem application and ensure you're logged into your workspace.

### Step 2: Select Workspace Pack

1. Click "Select Workspace" or "Open Folder"
2. Navigate to `workspace-packs/packs/research-synthesis-pack/`
3. Confirm selection

### Step 3: Begin Prompt 1 - Workspace Scan

Copy and paste Prompt #1 from PROMPTS.md into the chat:

```
Scan the inputs/ directory and create a summary document that:
1. Lists all available research papers and their focus areas
2. Identifies key research questions from inputs/questions.md
3. Extracts terminology from inputs/glossary.md
4. Notes the overall theme and scope of the literature
5. Identifies potential areas of disagreement across sources
Save your summary to outputs/workspace_scan.md
```

6. Review the files Tandem wants to read
7. Approve the read operation

### Step 4: Proceed Through the Workflow

Follow prompts sequentially:

- **Prompt 2**: Synthesis analysis (common themes + disagreements)
- **Prompt 3**: Claims/evidence table in Markdown
- **Prompt 4**: Executive brief for non-technical stakeholders
- **Prompt 5**: Generate HTML dashboard artifact

### Step 5: Review Generated Outputs

1. Open `outputs/` folder in your file explorer
2. Open `executive_brief.md` to review the summary
3. Open `research_brief_dashboard.html` in a browser
4. Verify the claims/evidence table structure

---

## Approval Workflow Details

### Read Approvals

When Tandem requests to read files:

1. Review the list of files
2. Confirm they match expected inputs (10 papers + questions + glossary)
3. Click "Approve" to proceed

### Write Approvals

When Tandem requests to write:

1. Check the file path (should be in `outputs/`)
2. Verify the format matches your expectations
3. Click "Approve" to confirm

### Multiple Approvals

Some prompts require multiple approvals. Review each request individually.

---

## Troubleshooting

| Issue                  | Solution                                                 |
| ---------------------- | -------------------------------------------------------- |
| Can't find pack folder | Ensure you're in `workspace-packs/packs/` directory      |
| Files not showing      | Click refresh in Tandem or restart                       |
| Prompt not working     | Ensure you're using exact prompt text from PROMPTS.md    |
| HTML not rendering     | Open in a modern browser (Chrome, Firefox, Safari, Edge) |
| Missing papers         | Check inputs/papers/ contains all 10 markdown files      |

---

## Tips for Best Results

1. **Be Patient**: Each prompt builds on previous analysis
2. **Review Conflicts**: Pay attention to conflicting claims flagged
3. **Verify Sources**: Check that claims map to specific papers
4. **Check Glossary**: Ensure terminology is used consistently
5. **Save Work**: Export outputs to your local machine

---

## Expected Output Locations

All generated content will appear in:

```
workspace-packs/packs/research-synthesis-pack/outputs/
```

Typical outputs include:

- `workspace_scan.md` (from Prompt 1)
- `synthesis_analysis.md` (from Prompt 2)
- `claims_evidence_table.md` (from Prompt 3)
- `executive_brief.md` (from Prompt 4)
- `research_brief_dashboard_*.html` (from Prompt 5)

---

## Next Steps

After completing the basic workflow:

1. Add additional papers to inputs/papers/
2. Generate follow-up research questions
3. Create region-specific executive briefs
4. Extend the claims table with additional dimensions
