# Getting Started - Web Research Refresh Pack

## Quick Start (10 Minutes)

### Step 1: Open Tandem

Launch the Tandem application and ensure you're logged into your workspace.

### Step 2: Select Workspace Pack

1. Click "Select Workspace" or "Open Folder"
2. Navigate to `workspace-packs/packs/web-research-refresh-pack/`
3. Confirm selection

### Step 3: Begin Prompt 1 - Stale Claim Identification

Copy and paste Prompt #1 from PROMPTS.md into the chat:

```
You are a research analyst tasked with identifying outdated claims in existing documentation.

Read the file in inputs/stale_brief.md and create a claims inventory document (outputs/claims_inventory.md) that:

1. Lists each factual claim in the stale brief
2. Identifies indicators that the claim may be outdated (dates, version numbers, "old understanding" language)
3. Estimates the priority for verification (Critical/High/Medium/Low)
   - Critical: Affects current decisions or security
   - High: Frequently referenced information
   - Medium: Background context
   - Low: Nice-to-know facts
4. Matches each claim to verification questions from inputs/verification_questions.md

Format as a table with columns:
- Claim | Evidence of Staleness | Priority | Verification Question

Save to outputs/claims_inventory.md and explain your reasoning before writing.
```

6. Review the file Tandem wants to read
7. Approve the read operation

### Step 4: Proceed Through the Workflow

Follow prompts sequentially:

- **Prompt 2**: Research plan creation
- **Prompt 3**: Webfetch evidence gathering
- **Prompt 4**: Updated facts sheet
- **Prompt 5**: Final HTML report with citations

### Step 5: Review Generated Outputs

1. Open `outputs/` folder in your file explorer
2. Review `evidence.md` for source verification
3. Review `updated_facts.md` for corrected information
4. Open `web_research_report.html` in a browser

---

## Approval Workflow Details

### Read Approvals

When Tandem requests to read files:

1. Review the list of files
2. Confirm they match expected inputs (2 input files)
3. Click "Approve" to proceed

### Write Approvals

When Tandem requests to write:

1. Check the file path (should be in `outputs/`)
2. Verify the content format
3. Click "Approve" to confirm

### Webfetch Approvals

When Tandem requests to fetch web sources:

1. Review the URLs (should be official/authoritative)
2. Approve legitimate research requests
3. Note: Only approve official documentation sources

---

## Troubleshooting

| Issue                  | Solution                                                 |
| ---------------------- | -------------------------------------------------------- |
| Can't find pack folder | Ensure you're in `workspace-packs/packs/` directory      |
| Files not showing      | Click refresh in Tandem or restart                       |
| Prompt not working     | Ensure you're using exact prompt text from PROMPTS.md    |
| HTML not rendering     | Open in a modern browser (Chrome, Firefox, Safari, Edge) |
| Missing outputs        | Check outputs/ directory was created                     |

---

## Tips for Best Results

1. **Be Thorough**: Each prompt builds on previous research
2. **Verify Sources**: Focus on official documentation
3. **Document Everything**: Evidence log is crucial for auditability
4. **Use Citations**: Every fact should have a source
5. **Compare Old vs New**: Highlight what changed from stale brief

---

## Expected Output Locations

All generated content will appear in:

```
workspace-packs/packs/web-research-refresh-pack/outputs/
```

Typical outputs include:

- `outputs/claims_inventory.md` (from Prompt 1)
- `outputs/research_plan.md` (from Prompt 2)
- `outputs/evidence.md` (from Prompt 3)
- `outputs/updated_facts.md` (from Prompt 4)
- `outputs/web_research_report_*.html` (from Prompt 5)

---

## Next Steps

After completing the basic workflow:

1. Try refreshing a different stale brief
2. Add more verification categories
3. Create templates for different fact types
4. Share the methodology with others
