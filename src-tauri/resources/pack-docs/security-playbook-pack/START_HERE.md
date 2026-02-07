# Getting Started - Security Playbook Pack

## Quick Start (10 Minutes)

### Step 1: Open Tandem

Launch the Tandem application and ensure you're logged into your workspace.

### Step 2: Select Workspace Pack

1. Click "Select Workspace" or "Open Folder"
2. Navigate to `workspace-packs/packs/security-playbook-pack/`
3. Confirm selection

### Step 3: Begin Prompt 1 - Context Analysis

Copy and paste Prompt #1 from PROMPTS.md into the chat:

```
You are a security architect building a comprehensive security playbook. Your task is to analyze the organizational context and create a security context summary.

Read ALL files in inputs/ including:
- inputs/company_context.md
- inputs/team_profile.md
- inputs/threat_landscape.md
- inputs/compliance_requirements.md

Create a security context summary document (outputs/security_context.md) that includes:

## Organizational Baseline
- Company size, industry, and risk profile
- Current security maturity level
- Key business drivers and constraints

## Threat Environment Overview
- Relevant threat actors and motivations
- Attack vectors most likely to target this organization
- Industry-specific threats and trends

## Compliance Obligations Summary
- Key regulations and standards applicable
- Critical compliance deadlines and requirements
- Audit cycles and documentation needs

## Security Posture Assessment
- Current strengths to leverage
- Critical gaps requiring immediate attention
- Resource constraints affecting security

## Risk Landscape Overview
- High-priority risk areas
- Risk tolerance indicators
- Key risk transfer considerations

Save your summary to outputs/security_context.md and explain your analysis approach before writing.
```

6. Review the files Tandem wants to read
7. Approve the read operation

### Step 4: Proceed Through the Workflow

Follow prompts sequentially:

- **Prompt 2**: Threat assessment with prioritization
- **Prompt 3**: Priority security checklist
- **Prompt 4**: Team-specific runbook
- **Prompt 5**: Generate HTML security playbook artifact

### Step 5: Review Generated Outputs

1. Open `outputs/` folder in your file explorer
2. Open `security_checklist.md` to review priorities
3. Open `team_runbook.md` for team procedures
4. Open `security_playbook.html` in a browser

---

## Approval Workflow Details

### Read Approvals

When Tandem requests to read files:

1. Review the list of files
2. Confirm they match expected inputs (4 input files)
3. Click "Approve" to proceed

### Write Approvals

When Tandem requests to write:

1. Check the file path (should be in `outputs/`)
2. Verify the content is appropriate
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
| Outputs missing        | Check outputs/ directory was created                     |

---

## Tips for Best Results

1. **Review Context First**: Understand the organization before diving into controls
2. **Prioritize Ruthlessly**: Focus on highest-impact, feasible actions first
3. **Tailor to Team**: Ensure runbook matches team capabilities
4. **Make It Actionable**: Every item should have clear next steps
5. **Review Regularly**: Security playbooks need updates as context changes

---

## Expected Output Locations

All generated content will appear in:

```
workspace-packs/packs/security-playbook-pack/outputs/
```

Typical outputs include:

- `outputs/security_context.md` (from Prompt 1)
- `outputs/threat_assessment.md` (from Prompt 2)
- `outputs/security_checklist.md` (from Prompt 3)
- `outputs/team_runbook.md` (from Prompt 4)
- `outputs/security_playbook_*.html` (from Prompt 5)

---

## Next Steps

After completing the basic workflow:

1. Customize the playbook for specific team members
2. Add incident response procedures
3. Develop metrics and KPIs for security program
4. Create a review and update schedule
5. Share with stakeholders for feedback
