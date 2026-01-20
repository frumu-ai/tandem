# Expected Outputs - Quality Criteria

This document defines what a successful run of the Research Synthesis pack produces and how to validate quality.

---

## Output Files Checklist

### Required Outputs

| File                                    | Status | Quality Check                                |
| --------------------------------------- | ------ | -------------------------------------------- |
| `outputs/workspace_scan.md`             | ☐      | Contains all sections from Prompt 1          |
| `outputs/synthesis_analysis.md`         | ☐      | Complete theme and conflict analysis         |
| `outputs/claims_evidence_table.md`      | ☐      | Structured claims matrix with all categories |
| `outputs/executive_brief.md`            | ☐      | One-page non-technical summary               |
| `outputs/research_brief_dashboard.html` | ☐      | Valid HTML, all sections present             |

---

## Quality Criteria by Output

### workspace_scan.md

- [ ] Lists ALL 10 papers from inputs/papers/
- [ ] Accurately describes each paper's focus area
- [ ] Identifies all 5+ conflict areas
- [ ] Correctly maps papers to research questions
- [ ] Includes terminology inventory
- [ ] Provides source quality assessment

**Red Flags**:

- Missing papers from review
- Misidentified paper focus areas
- No conflict identification (should be 5+)
- Incorrect question mapping

---

### synthesis_analysis.md

- [ ] Identifies 4-5 major themes across papers
- [ ] Documents 5+ distinct conflicts with evidence
- [ ] Each conflict has Position A and Position B
- [ ] Assessment provided for each conflict
- [ ] Evidence quality comparison included
- [ ] Research gaps identified

**Red Flags**:

- Fewer than 4 themes identified
- Conflicts lack specific evidence citations
- No assessment of why papers disagree
- Missing evidence quality analysis

---

### claims_evidence_table.md

#### Structure Validation

- [ ] All 5 categories represented (Privacy, Security, Governance, Performance, Cost)
- [ ] Claims table format followed
- [ ] Strength and consensus columns populated
- [ ] Conflicting claims summary section present
- [ ] Key findings summary included

#### Content Validation

- [ ] Claims are specific and verifiable
- [ ] Evidence citations match actual paper content
- [ ] Strength assessments are reasonable
- [ ] Consensus levels are accurate
- [ ] Conflicting claims are clearly presented

**Red Flags**:

- Generic or vague claims
- Claims not supported by cited sources
- Missing category coverage
- No conflicting claims documented

---

### executive_brief.md

#### Format Validation

- [ ] Maximum 1 page (~500-700 words)
- [ ] Non-technical language throughout
- [ ] All required sections present
- [ ] Clear headings and structure
- [ ] Bullet points used appropriately

#### Content Validation

- [ ] Accurate representation of research findings
- [ ] Balanced presentation of trade-offs
- [ ] Practical, actionable recommendations
- [ ] Audience-appropriate (non-technical)
- [ ] No jargon without explanation

**Red Flags**:

- Too technical for general audience
- Missing key findings
- Biased presentation (only positives or only negatives)
- Recommendations not grounded in evidence
- Exceeds page length significantly

---

### research_brief_dashboard.html

#### Technical Validation

- [ ] File opens in browser without errors
- [ ] All CSS loads correctly
- [ ] No broken links or missing resources
- [ ] Responsive design works on mobile
- [ ] All sections visible and readable

#### Content Validation

- [ ] Research Overview Header complete
- [ ] Executive Summary Card accurate
- [ ] Key Themes visualization present
- [ ] Conflict Dashboard with 5+ conflicts
- [ ] Claims & Evidence Matrix included
- [ ] Recommendations section actionable
- [ ] Research Gaps identified

#### Design Validation

- [ ] Clean, professional appearance
- [ ] Good visual hierarchy
- [ ] Appropriate use of color
- [ ] Readable typography
- [ ] Consistent spacing and alignment
- [ ] Print-friendly

**Red Flags**:

- Doesn't open in browser
- Missing sections
- Broken layout
- Content doesn't match source documents
- Design is cluttered or hard to read

---

## Conflict Validation Checklist

Verify at least 5 conflicts are identified and documented:

- [ ] Conflict 1: Privacy vs. Security trade-offs
- [ ] Conflict 2: Local vs. Cloud security posture
- [ ] Conflict 3: Cost comparisons (TCO)
- [ ] Conflict 4: Implementation feasibility
- [ ] Conflict 5: Regulatory approach
- [ ] Additional conflicts as identified

Each conflict must include:

- Specific opposing claims from different papers
- Evidence/supporting data from each position
- Assessment/analysis of the disagreement

---

## Constraint Compliance

Verify that outputs:

- [ ] No direct quotes from source papers (synthesized only)
- No personal data or secrets referenced
- [ ] All claims attributed to specific papers
- [ ] Disagreements are presented objectively
- [ ] No overstatement of findings

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
- [ ] Tables render properly
- [ ] Color scheme is professional
- [ ] Content is accurate to source documents

---

## Common Issues & Fixes

| Issue               | Likely Cause                      | Fix                                               |
| ------------------- | --------------------------------- | ------------------------------------------------- |
| Missing conflicts   | Didn't thoroughly read all papers | Re-run Prompt 2 with more careful analysis        |
| Generic claims      | Surface-level reading             | Re-run Prompt 3 with specific evidence extraction |
| Technical language  | Wrong audience assumed            | Re-run Prompt 4 with simpler language             |
| HTML broken         | Missing closing tags              | Re-run Prompt 5                                   |
| Incomplete coverage | Rushed reading                    | Re-run from Prompt 1                              |

---

## Approvals Checklist

Before considering the pack run complete:

- [ ] Read approvals granted for all inputs files (10 papers + 2 docs)
- [ ] Write approval granted for each output file
- [ ] All outputs saved to correct paths
- [ ] HTML dashboard opens successfully
- [ ] Claims are properly attributed
- [ ] Conflicts are clearly documented
- [ ] Executive brief is non-technical
- [ ] Quality criteria met
