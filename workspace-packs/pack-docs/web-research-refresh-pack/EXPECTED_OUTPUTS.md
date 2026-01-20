# Expected Outputs - Quality Criteria

This document defines what a successful run of the Web Research Refresh Pack produces and how to validate quality.

---

## Output Files Checklist

### Required Outputs

| File                                 | Status | Quality Check                           |
| ------------------------------------ | ------ | --------------------------------------- |
| `outputs/claims_inventory.md`        | ☐      | Complete claim identification           |
| `outputs/research_plan.md`           | ☐      | Organized research strategy             |
| `outputs/evidence.md`                | ☐      | Source verification with URLs and dates |
| `outputs/updated_facts.md`           | ☐      | Corrected facts by category             |
| `outputs/web_research_report_*.html` | ☐      | Polished HTML with citations            |

---

## Quality Criteria by Output

### claims_inventory.md

- [ ] All claims from stale_brief.md identified
- [ ] Staleness indicators noted for each claim
- [ ] Priority assigned (Critical/High/Medium/Low)
- [ ] Verification questions matched
- [ ] Table format used

**Red Flags**:

- Missing claims from original brief
- No prioritization
- No connection to verification questions

---

### research_plan.md

- [ ] Research plan for each category
- [ ] Source strategy defined
- [ ] Authoritative sources identified
- [ ] Success criteria stated
- [ ] Source checklist included

**Red Flags**:

- Vague source strategy
- No prioritization of sources
- Missing categories

---

### evidence.md

- [ ] Every claim from claims_inventory addressed
- [ ] Source URLs provided for each finding
- [ ] Access dates recorded (YYYY-MM-DD)
- [ ] Status assigned (VERIFIED/CONTRADICTED/INSUFFICIENT)
- [ ] Corrected information synthesized
- [ ] Official/authoritative sources used

**Red Flags**:

- Missing claims
- No access dates
- Non-authoritative sources (Reddit, Wikipedia primary)
- No clear status assignment

---

### updated_facts.md

- [ ] Executive summary present
- [ ] Changes table showing old vs. new
- [ ] Facts organized by category
- [ ] Status indicators included
- [ ] Recommendations section present
- [ ] Unverifiable claims noted

**Red Flags**:

- No change summary
- Missing categories
- No status indicators

---

### web*research_report*\*.html

#### Technical Validation

- [ ] File opens in browser without errors
- [ ] All CSS loads correctly
- [ ] Responsive design works
- [ ] Tables render properly
- [ ] Print-friendly styling

#### Content Validation

- [ ] Title header present
- [ ] Executive summary (2-3 paragraphs)
- [ ] What changed section with table
- [ ] Verified facts by category
- [ ] Inline citations [1], [2], etc.
- [ ] Sources section with full citations
- [ ] Each citation has URL and access date
- [ ] Evidence log reference included

#### Citation Validation

- [ ] All facts have inline citations
- [ ] Citations map to Sources section
- [ ] Sources use official/authoritative URLs
- [ ] Access dates recorded

**Red Flags**:

- Doesn't open in browser
- Missing citations on facts
- Citations don't map to sources
- No access dates
- Non-authoritative sources

---

## Source Quality Checklist

Verify sources meet authority requirements:

- [ ] Official documentation (docs.github.com, taurapps.org, etc.)
- [ ] Official vendor websites
- [ ] GitHub repositories (official)
- [ ] Authoritative industry standards

**Reject**:

- Paywalled reports
- Wikipedia as primary source
- Reddit/community forums as primary
- Third-party blogs without official verification

---

## Constraint Compliance

Verify outputs:

- [ ] No secrets or personal data
- [ ] All claims have citations
- [ ] Evidence log is auditable
- [ ] HTML report is shareable
- [ ] No copyrighted text (summarized in own words)

---

## How to Validate HTML Report

### Browser Testing

1. Open file in Chrome, Firefox, Safari, Edge
2. Check console for errors (F12 → Console)
3. Test responsiveness (resize browser window)
4. Verify all links work
5. Print to PDF to test print styles

### Manual Checklist

- [ ] Title displays correctly
- [ ] Tables render with proper borders
- [ ] Citations are clickable/visible
- [ ] Sources section complete
- [ ] Styling is professional

---

## Common Issues & Fixes

| Issue           | Likely Cause                       | Fix                                 |
| --------------- | ---------------------------------- | ----------------------------------- |
| Missing claims  | Didn't read stale_brief completely | Re-run Prompt 1 with full review    |
| No access dates | Didn't record during research      | Re-run Prompt 3 with date recording |
| Bad sources     | Used non-authoritative sources     | Re-run Prompt 3 with official docs  |
| HTML broken     | Missing closing tags               | Re-run Prompt 5                     |
| No citations    | Skipped citation step              | Re-run Prompt 5                     |

---

## Approvals Checklist

Before considering the pack run complete:

- [ ] Read approvals granted for inputs/stale_brief.md
- [ ] Read approvals granted for inputs/verification_questions.md
- [ ] Write approval granted for claims_inventory.md
- [ ] Write approval granted for research_plan.md
- [ ] Webfetch approvals for all sources
- [ ] Write approval granted for evidence.md
- [ ] Write approval granted for updated_facts.md
- [ ] Write approval granted for web*research_report*\*.html
- [ ] All outputs saved to outputs/
- [ ] HTML report opens successfully
- [ ] All claims verified
- [ ] Quality criteria met
