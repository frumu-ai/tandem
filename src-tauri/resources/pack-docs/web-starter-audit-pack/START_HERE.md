# Getting Started - Web Starter Audit Pack

## Quick Start (10 Minutes)

### Step 1: Open Tandem

Launch the Tandem application and ensure you're logged into your workspace.

### Step 2: Select Workspace Pack

1. Click "Select Workspace" or "Open Folder"
2. Navigate to `workspace-packs/packs/web-starter-audit-pack/`
3. Confirm selection

### Step 3: Begin Prompt 1 - Project Audit

Copy and paste Prompt #1 from PROMPTS.md into the chat:

```
Conduct a comprehensive audit of the web project in src/. Read all files:
- src/index.html
- src/styles.css
- src/app.js

Audit the project across these categories:
1. Accessibility (WCAG compliance, ARIA labels, semantic structure)
2. User Experience (navigation, layout, visual design)
3. Code Quality (structure, maintainability, best practices)
4. JavaScript Functionality (bugs, logic errors, edge cases)
5. CSS Architecture (organization, specificity, responsiveness)

For each category, document:
- Issues found (with line numbers)
- Severity (Critical, Major, Minor, Suggestion)
- Impact on users or developers
- Suggested fix approach

Save your audit findings to outputs/audit_findings.md
```

6. Review the files Tandem wants to read
7. Approve the read operation

### Step 4: Proceed Through the Workflow

Follow prompts sequentially:

- **Prompt 2**: Plan fixes and present for approval
- **Prompt 3**: Implement minimal fixes (write changes to src/)
- **Prompt 4**: Generate changelog + before/after summary
- **Prompt 5**: Create HTML audit report artifact

### Step 5: Review Generated Outputs

1. Open `outputs/` folder in your file explorer
2. Review the changelog for changes made
3. Open `audit_report.html` in a browser
4. Verify the fixes in `src/` files

---

## Approval Workflow Details

### Read Approvals

When Tandem requests to read files:

1. Review the list of files
2. Confirm they match expected sources (3 src files)
3. Click "Approve" to proceed

### Write Approvals

When Tandem requests to write:

1. Check the file path (audit outputs go to outputs/, fixes go to src/)
2. Verify the changes are appropriate
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
| Fixes not applied      | Check that approvals were granted for src/ writes        |

---

## Tips for Best Results

1. **Review Thoroughly**: Each audit category is important
2. **Prioritize Issues**: Focus on critical accessibility issues first
3. **Validate Fixes**: Check that fixes don't introduce new issues
4. **Document Everything**: Changelog captures the journey
5. **Use the Report**: The HTML report is great for stakeholder sharing

---

## Expected Output Locations

All generated content will appear in:

```
workspace-packs/packs/web-starter-audit-pack/
├── outputs/        # Generated audit outputs
│   ├── audit_findings.md
│   ├── remediation_plan.md
│   ├── changelog.md
│   └── audit_report_*.html
└── src/            # Original and modified source files
    ├── index.html
    ├── styles.css
    └── app.js
```

Typical outputs include:

- `outputs/audit_findings.md` (from Prompt 1)
- `outputs/remediation_plan.md` (from Prompt 2)
- `outputs/changelog.md` (from Prompt 4)
- `outputs/audit_report_*.html` (from Prompt 5)
- Modified `src/` files (from Prompt 3)

---

## Next Steps

After completing the basic workflow:

1. Add additional pages to the project
2. Expand the audit to include more categories
3. Create automated testing pipelines
4. Implement accessibility testing in CI/CD
5. Share the audit report with stakeholders
