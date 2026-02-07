# Expected Outputs - Quality Criteria

This document defines what a successful run of the Web Starter Audit Pack produces and how to validate quality.

---

## Output Files Checklist

### Required Outputs

| File                          | Status | Quality Check                        |
| ----------------------------- | ------ | ------------------------------------ |
| `outputs/audit_findings.md`   | ☐      | Complete audit with all 5 categories |
| `outputs/remediation_plan.md` | ☐      | Prioritized fix plan                 |
| `outputs/changelog.md`        | ☐      | Documented changes made              |
| `outputs/audit_report.html`   | ☐      | Valid HTML, all sections present     |

### Modified Source Files

| File             | Status | Quality Check |
| ---------------- | ------ | ------------- |
| `src/index.html` | ☐      | Issues fixed  |
| `src/styles.css` | ☐      | Issues fixed  |
| `src/app.js`     | ☐      | Issues fixed  |

---

## Quality Criteria by Output

### audit_findings.md

- [ ] All 5 categories covered (Accessibility, UX, Code Quality, JS Functionality, CSS)
- [ ] Issues documented with file and line numbers
- [ ] Severity assigned correctly (Critical/Major/Minor/Suggestion)
- [ ] Impact described for each issue
- [ ] Suggested fix provided
- [ ] References to inputs/TODO.md acknowledged

**Red Flags**:

- Missing categories
- No specific file/line references
- Generic recommendations without context
- Severity not justified

---

### remediation_plan.md

- [ ] Executive summary present
- [ ] All critical issues prioritized in Phase 1
- [ ] Issues grouped by phase
- [ ] Proposed fixes specific and actionable
- [ ] Dependencies between fixes noted
- [ ] Stakeholder questions included

**Red Flags**:

- Critical issues not prioritized
- Vague or generic fix descriptions
- No stakeholder approval questions
- Missing issue references

---

### src/ Files (Modified)

#### HTML Validation

- [ ] Heading hierarchy fixed (h1, h2, h3 in sequence)
- [ ] Form labels added where missing
- [ ] Image alt text corrected
- [ ] Semantic HTML used appropriately
- [ ] No accessibility regressions introduced

#### CSS Validation

- [ ] Focus styles present and visible
- [ ] Color contrast meets WCAG AA (4.5:1)
- [ ] Responsive design maintained
- [ ] No unintended style changes
- [ ] Repetitive code consolidated (if addressed)

#### JavaScript Validation

- [ ] Sorting bug fixed (numeric comparison working)
- [ ] Filtering continues to work correctly
- [ ] No new JavaScript errors in console
- [ ] Event handlers functioning properly

**Red Flags**:

- Accessibility issues remain unfixed
- New issues introduced
- Sorting still broken (numeric comparison)
- Code functionality impaired

---

### changelog.md

- [ ] Change summary with totals
- [ ] Each change has before/after description
- [ ] File modification summary table included
- [ ] Testing notes included
- [ ] Issue references match audit findings

**Red Flags**:

- Missing changes
- Vague descriptions
- No before/after comparison
- Doesn't match actual file modifications

---

### audit_report.html

#### Technical Validation

- [ ] File opens in browser without errors
- [ ] All CSS loads correctly
- [ ] No broken links or missing resources
- [ ] Responsive design works on mobile
- [ ] All sections visible and readable
- [ ] Print stylesheet functional

#### Content Validation

- [ ] Report header complete
- [ ] Executive summary accurate
- [ ] Findings summary dashboard present
- [ ] Critical issues section complete
- [ ] Major issues section complete
- [ ] Minor issues documented
- [ ] Code quality metrics included
- [ ] Before/after comparison present
- [ ] Remediation summary complete
- [ ] Recommendations actionable
- [ ] Appendices included

#### Design Validation

- [ ] Clean, professional appearance
- [ ] Good visual hierarchy
- [ ] Appropriate use of color
- [ ] Readable typography
- [ ] Consistent spacing and alignment
- [ ] Charts/visualizations render correctly

**Red Flags**:

- Doesn't open in browser
- Missing sections
- Broken layout
- Content doesn't match audit findings
- Design is cluttered or hard to read

---

## Accessibility Validation Checklist

Verify critical accessibility issues are addressed:

- [ ] Heading hierarchy follows proper sequence
- [ ] Form inputs have associated labels
- [ ] Images have meaningful alt text
- [ ] Interactive elements have accessible names
- [ ] Keyboard navigation works
- [ ] Color contrast meets WCAG AA

---

## Functional Testing Checklist

Verify JavaScript functionality:

- [ ] Filter buttons work correctly
- [ ] Sorting works (numeric and alphabetic)
- [ ] Search filters products correctly
- [ ] Add to cart buttons function
- [ ] No console errors

---

## Common Issues & Fixes

| Issue                   | Likely Cause                    | Fix                                  |
| ----------------------- | ------------------------------- | ------------------------------------ |
| Incomplete audit        | Rushed reading of files         | Re-run Prompt 1 with full review     |
| Wrong severity          | Lack of accessibility expertise | Re-run Prompt 1 with WCAG reference  |
| Sort still broken       | Fix didn't address root cause   | Re-run Prompt 3 with focus on JS     |
| HTML broken             | Syntax errors in edits          | Re-run Prompt 3 with careful editing |
| Report missing sections | Prompt not followed completely  | Re-run Prompt 5                      |

---

## Approvals Checklist

Before considering the pack run complete:

- [ ] Read approvals granted for all source files (3 files)
- [ ] Write approval granted for audit_findings.md
- [ ] Write approval granted for remediation_plan.md
- [ ] Write approvals granted for src/ file modifications
- [ ] Write approval granted for changelog.md
- [ ] Write approval granted for audit_report.html
- [ ] All outputs saved to correct paths
- [ ] HTML report opens successfully
- [ ] All critical issues addressed
- [ ] No new issues introduced
- [ ] Quality criteria met
