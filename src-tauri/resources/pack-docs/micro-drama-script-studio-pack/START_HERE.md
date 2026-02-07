# Getting Started - Micro-Drama Script Studio

## Quick Start (5 Minutes)

### Step 1: Open Tandem

Launch the Tandem application and ensure you're logged into your workspace.

### Step 2: Select Workspace Pack

1. Click "Select Workspace" or "Open Folder"
2. Navigate to `workspace-packs/packs/micro-drama-script-studio-pack/`
3. Confirm selection

### Step 3: Begin Prompt 1 - Workspace Scan

Copy and paste Prompt #1 from PROMPTS.md into the chat:

```
Scan the inputs/ directory and create a summary document that:
1. Lists all available reference materials
2. Extracts tone guidelines and style constraints
3. Identifies script format requirements
4. Notes any special conventions or banned content
Save your summary to outputs/workspace_scan.md
```

5. Review the files Tandem wants to read
6. Approve the read operation

### Step 4: Proceed Through the Workflow

Follow prompts sequentially:

- **Prompt 2**: Develop 3 premise options, choose one
- **Prompt 3**: Create detailed episode beats/outline
- **Prompt 4**: Write the full episode script
- **Prompt 5**: Generate HTML dashboard artifact

### Step 5: Review Generated Outputs

1. Open `outputs/` folder in your file explorer
2. Open `episode_001.md` to review the script
3. Open `writers_room_dashboard.html` in a browser
4. Check that all sections render correctly

---

## Approval Workflow Details

### Read Approvals

When Tandem requests to read files:

1. Review the list of files
2. Confirm they match expected inputs
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

| Issue                  | Solution                                                  |
| ---------------------- | --------------------------------------------------------- |
| Can't find pack folder | Ensure you're in the `workspace-packs/packs/` directory   |
| Files not showing      | Click refresh in Tandem or restart the application        |
| Prompt not working     | Ensure you're using the exact prompt text from PROMPTS.md |
| HTML not rendering     | Open in a modern browser (Chrome, Firefox, Safari, Edge)  |
| Character names wrong  | Check CHARACTER_SHEETS.md for correct format              |

---

## Tips for Best Results

1. **Be Patient**: Each prompt builds on the previous output
2. **Review Early**: Check intermediate outputs before proceeding
3. **Iterate**: You can re-run prompts with modified instructions
4. **Save Work**: Export important outputs to your local machine
5. **Provide Feedback**: Use Tandem's feedback features to improve results

---

## Expected Output Locations

All generated content will appear in:

```
workspace-packs/packs/micro-drama-script-studio-pack/outputs/
```

Typical outputs include:

- `workspace_scan.md` (from Prompt 1)
- `premise_selection.md` (from Prompt 2)
- `episode_beats_001.md` (from Prompt 3)
- `episode_001.md` (from Prompt 4)
- `writers_room_dashboard_*.html` (from Prompt 5)

---

## Next Steps

After completing the basic workflow:

1. Try alternative premises from Prompt 2
2. Generate episode variants (episode_001_alt.md)
3. Create a multi-episode arc
4. Adapt the format for different genres
