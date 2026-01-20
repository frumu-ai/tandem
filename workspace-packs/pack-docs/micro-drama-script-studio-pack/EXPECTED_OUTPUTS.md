# Expected Outputs - Quality Criteria

This document defines what a successful run of the Micro-Drama Script Studio pack produces and how to validate quality.

---

## Output Files Checklist

### Required Outputs

| File                                  | Status | Quality Check                       |
| ------------------------------------- | ------ | ----------------------------------- |
| `outputs/workspace_scan.md`           | ☐      | Contains all sections from Prompt 1 |
| `outputs/premises_options.md`         | ☐      | 3 distinct premises with all fields |
| `outputs/episode_beats_001.md`        | ☐      | Complete scene breakdown            |
| `outputs/episode_001.md`              | ☐      | Full script in correct format       |
| `outputs/writers_room_dashboard.html` | ☐      | Valid HTML, all sections present    |

### Optional Outputs

| File                         | When Generated          |
| ---------------------------- | ----------------------- |
| `outputs/episode_001_alt.md` | If generating variant   |
| `outputs/episode_002.md`     | If extending series     |
| `outputs/cast_sheet.md`      | If requested separately |

---

## Quality Criteria by Output

### workspace_scan.md

- [ ] Lists ALL files from inputs/
- [ ] Accurately describes tone and style
- [ ] Correctly identifies script format rules
- [ ] Notes all content constraints
- [ ] Provides useful example analysis

**Red Flags**:

- Missing files from inputs/
- Misidentified format requirements
- Contradictory style notes

---

### premises_options.md

- [ ] Three distinct premises (different genres/themes)
- [ ] Each premise has all required fields filled
- [ ] Loglines are under 20 words
- [ ] Cliffhangers are compelling and specific
- [ ] Structure breakdown shows proper timing
- [ ] Recommendation is justified

**Red Flags**:

- Duplicate or too-similar premises
- Missing required fields
- Unrealistic runtime estimates
- Weak or unclear cliffhangers

---

### episode_beats_001.md

- [ ] Working title is catchy and relevant
- [ ] Scene count matches genre expectations (3-5)
- [ ] Each scene has timing, location, purpose
- [ ] Cliffhanger clearly identified
- [ ] Arc notes show larger story awareness
- [ ] Dialogue guidelines are specific

**Red Flags**:

- Scenes too long/short for runtime
- Unclear scene purposes
- Missing or weak cliffhanger
- No arc consideration

---

### episode_001.md

#### Format Validation

- [ ] Proper header with all metadata
- [ ] Scene headings in correct format (CAPS)
- [ ] Character names in correct format (NAME:)
- [ ] Action lines present tense, visual
- [ ] Dialogue formatting consistent throughout

#### Content Validation

- [ ] Opens with strong visual/dialogue hook (first 10 seconds)
- [ ] Every scene has clear purpose
- [ ] Dialogue is punchy and character-distinct
- [ ] Conflict is clear and escalates
- [ ] Episode ends with cliffhanger
- [ ] Runtime is reasonable (60-180 seconds)

#### Style Validation

- [ ] Matches tone from STYLE_GUIDE.md
- [ ] Follows pacing guidelines
- [ ] Respects content constraints
- [ ] Demonstrates genre conventions

**Red Flags**:

- Wrong format entirely
- Dialogue too long or exposition-heavy
- No clear conflict
- Missing or weak opening/closing
- Inconsistent character names

---

### writers_room_dashboard.html

#### Technical Validation

- [ ] File opens in browser without errors
- [ ] All CSS loads correctly
- [ ] No broken links or missing resources
- [ ] Responsive design works on mobile
- [ ] All sections are visible and readable

#### Content Validation

- [ ] Episode header complete
- [ ] Premise card accurate
- [ ] Cast sheet has all characters
- [ ] Scene breakdown matches beats document
- [ ] Checklist is actionable
- [ ] Quick reference section useful

#### Design Validation

- [ ] Clean, professional appearance
- [ ] Good visual hierarchy
- [ ] Appropriate use of color
- [ ] Readable typography
- [ ] Consistent spacing and alignment

**Red Flags**:

- Doesn't open in browser
- Missing sections
- Broken layout
- Hard to read
- Doesn't match source documents

---

## Tone Validation Checklist

Across all outputs, check for:

- [ ] Consistent tone throughout
- [ ] Appropriate emotional beats
- [ ] Genre conventions respected
- [ ] Target audience considered
- [ ] Platform requirements met (short-form, mobile)

---

## Constraint Compliance

Verify that outputs:

- [ ] Avoid all topics in "banned content" list
- [ ] Follow character conventions from inputs/
- [ ] Respect any platform-specific rules
- [ ] Contain no real names/places (unless template)
- [ ] Are original and not derivative

---

## How to Validate HTML Dashboard

### Browser Testing

1. Open file in Chrome, Firefox, Safari, Edge
2. Check console for errors (F12 → Console)
3. Test responsiveness (resize browser window)
4. Verify all links work
5. Print to PDF to test print styles

### Manual Checklist

- [ ] Title displays correctly
- [ ] All cards/sections visible
- [ ] Checklist items can be mentally checked
- [ ] Cast information accurate
- ] Scene breakdown clear

---

## Common Issues & Fixes

| Issue          | Likely Cause                 | Fix                                      |
| -------------- | ---------------------------- | ---------------------------------------- |
| Format wrong   | Didn't read SCRIPT_FORMAT.md | Re-run Prompt 1, then Prompt 4           |
| Weak premise   | Premise too generic          | Re-run Prompt 2 with more specific genre |
| No cliffhanger | Outline missed it            | Re-run Prompt 3 focusing on ending       |
| HTML broken    | Missing closing tags         | Re-run Prompt 5                          |
| Tone off       | Style guide ignored          | Re-run from Prompt 1                     |

---

## Approvals Checklist

Before considering the pack run complete:

- [ ] Read approvals granted for all inputs files
- [ ] Write approval granted for each output file
- [ ] All outputs saved to correct paths
- [ ] HTML dashboard opens successfully
- [ ] Script format validated
- [ ] Content constraints respected
- [ ] Quality criteria met

---

## Reporting Results

If outputs don't meet quality criteria:

1. Note which specific criteria failed
2. Identify which prompt likely needs re-running
3. Consider modifying the prompt with more specific instructions
4. Document any edge cases or unusual requirements
