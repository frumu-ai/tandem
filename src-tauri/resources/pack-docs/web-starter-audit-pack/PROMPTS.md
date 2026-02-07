# Tandem Prompts - Web Starter Audit Pack

Copy and paste these prompts sequentially into Tandem. Each prompt builds on the previous outputs.

---

## Prompt 1: Comprehensive Project Audit

```
You are a web development auditor specializing in accessibility, user experience, and code quality. Your task is to conduct a comprehensive audit of the web project in src/.

Read ALL source files:
- src/index.html
- src/styles.css
- src/app.js

Also review inputs/TODO.md for context on known issues.

Audit the project across these 5 categories:

### 1. Accessibility (WCAG Compliance)
- Check for proper semantic HTML elements
- Verify ARIA labels and roles where needed
- Check heading hierarchy (h1-h6 in order)
- Verify form labels and associations
- Check image alt text
- Verify keyboard accessibility
- Check color contrast ratios

### 2. User Experience
- Navigation clarity and ease of use
- Visual hierarchy and layout
- Button and link clarity
- Form usability
- Search functionality
- Filter and sort UX

### 3. Code Quality
- HTML structure and semantics
- CSS organization and specificity
- JavaScript structure and readability
- Code duplication
- Best practices compliance
- Maintainability indicators

### 4. JavaScript Functionality
- Sorting logic correctness
- Filtering logic correctness
- Event handler implementation
- Edge case handling
- Error handling
- Performance considerations

### 5. CSS Architecture
- Selector specificity
- Responsive design implementation
- CSS organization
- Repetitive code patterns
- Accessibility-related CSS (focus states, etc.)

For EACH issue found, document:
- **File and Line**: Specific location
- **Issue Description**: What the problem is
- **Severity**: Critical (blocks access), Major (significant issue), Minor (cosmetic/suggestion)
- **Impact**: Who/what is affected
- **Suggested Fix**: How to resolve it

Group issues by category and severity.

Save your audit findings to outputs/audit_findings.md
```

---

## Prompt 2: Remediation Planning

```
You are a web project manager creating a remediation plan. Based on the audit findings in outputs/audit_findings.md, create a structured plan for addressing the identified issues.

## Remediation Plan Requirements

### Executive Summary
Brief overview of audit results and remediation scope.

### Prioritized Issue List

For each issue to be addressed, provide:

#### Issue: [Name]
- **Severity**: [Critical/Major/Minor/Suggestion]
- **File**: [src/file.html, etc.]
- **Description**: What the issue is
- **Proposed Fix**: How you will fix it
- **Files to Modify**: Which files need changes
- **Effort Estimate**: Small/Medium/Large
- **Dependencies**: Any other fixes this depends on

### Implementation Phases

**Phase 1: Critical Issues (Must fix before launch)**
- List critical issues with fix approach
- Dependencies between fixes

**Phase 2: Major Issues (Should fix)**
- List major issues with fix approach
- Rationale for prioritization

**Phase 3: Minor Issues (Consider fixing)**
- List minor issues
- Trade-offs for fixing vs. deferring

### Before/After Summary
Brief description of what will change from current state.

### Questions for Stakeholder Approval

Present the plan and ask:
1. Are you comfortable with this prioritization?
2. Should any issues be deferred to a later phase?
3. Are there additional issues we should consider?

Save to outputs/remediation_plan.md
```

---

## Prompt 3: Implement Minimal Fixes

```
Based on the approved remediation plan, implement the minimal fixes needed to address the identified issues.

## Fix Implementation Requirements

Implement fixes ONLY for:
- Critical issues (Phase 1)
- Major issues that are straightforward (Phase 2)

For each fix, explain what you're changing and why, then ask approval before writing.

### Accessibility Fixes to Implement
1. Fix heading hierarchy (add h2, h3 levels)
2. Add proper form labels
3. Fix image alt text
4. Add aria-labels where needed
5. Improve keyboard accessibility

### JavaScript Fixes to Implement
1. Fix sorting logic bug (numeric comparison)
2. Ensure proper product visibility handling

### CSS Fixes to Implement
1. Add/fix focus styles where missing
2. Address color contrast issues
3. Consolidate repetitive button styles (if time permits)

### HTML Fixes to Implement
1. Fix heading structure
2. Add semantic elements where appropriate
3. Improve link text clarity

## Process

For each file modification:
1. Read the current file
2. Explain the changes you will make
3. Ask for approval
4. Write the modified file to src/

Start by reading the files and presenting your fix plan for approval.
```

---

## Prompt 4: Changelog Generation

```
You are a technical writer documenting project changes. Create a comprehensive changelog documenting all fixes implemented.

## Changelog Requirements

### Change Summary
- Total issues resolved
- Breakdown by severity
- Files modified

### Detailed Change Log

For each change, document:

#### [Change Title]
- **File(s) Modified**: src/[filename]
- **Issue Reference**: [From audit findings]
- **Before**: Description of original code/state
- **After**: Description of fixed code/state
- **Rationale**: Why this fix was necessary
- **Impact**: What this improves

### Before/After Comparison

Create a side-by-side or summary showing:
- Number of accessibility issues reduced
- Code quality improvements
- Bug fixes applied

### Files Modified Summary

| File | Changes Made | Lines Modified |
|------|--------------|----------------|
| src/index.html | [summary] | [#] |
| src/styles.css | [summary] | [#] |
| src/app.js | [summary] | [#] |

### Testing Notes
- What testing was performed
- Any manual verification needed
- Known limitations

Save to outputs/changelog.md
```

---

## Prompt 5: HTML Project Audit Report

```
Create a comprehensive HTML audit report artifact that presents the entire audit process, findings, and remediation in a professional, visual format.

## Report Sections Required

### 1. Report Header
- Report title: "Web Project Audit Report"
- Audit date
- Project name/version
- Auditor identification

### 2. Executive Summary
- Overall project health score
- Total issues found by severity
- Summary of critical issues addressed
- Project status indicator (Pass/Conditional Pass/Needs Work)

### 3. Audit Scope & Methodology
- Files audited
- Categories evaluated
- Standards referenced (WCAG 2.1, etc.)
- Tools/methods used

### 4. Findings Summary Dashboard
- Visual summary of findings by category
- Severity distribution chart (Critical/Major/Minor/Suggestion)
- Category breakdown

### 5. Critical Issues Section
- List of critical issues found
- Impact assessment
- Remediation status (Fixed/Outstanding)
- For each critical issue:
  - Description
  - Location (file:line)
  - Severity
  - Impact
  - Fix applied

### 6. Major Issues Section
- List of major issues found
- Remediation status
- Impact on users/developers

### 7. Minor Issues & Suggestions
- List of minor issues
- Suggestions for improvement
- Trade-offs considered

### 8. Code Quality Metrics
- HTML quality assessment
- CSS architecture review
- JavaScript quality review
- Maintainability indicators

### 9. Before/After Comparison
- Visual comparison of key metrics
- Accessibility improvements
- Code quality improvements
- Bug fixes summary

### 10. Remediation Summary
- Total issues fixed
- Files modified
- Effort summary
- Remaining issues (if any)

### 11. Recommendations
- Immediate actions needed
- Short-term improvements
- Long-term considerations
- Future audit recommendations

### 12. Appendices
- Detailed findings by file
- References and resources

## Styling Requirements
- Clean, professional design
- Use CSS Grid or Flexbox for layout
- Mobile-responsive
- Print-friendly
- Professional color scheme (suggest: navy blue, white, accent colors)
- Readable typography with good hierarchy
- Charts/visualizations using CSS or inline SVG

## Technical Requirements
- Single self-contained HTML file (no external dependencies)
- All CSS inline or in <style> block
- No JavaScript required (or inline only)
- Valid HTML5
- Works offline

## Interactivity
- Expandable/collapsible sections
- Smooth transitions
- Print stylesheet for PDF export
- Progress indicators for finding categories

Save to: outputs/audit_report.html

Before generating, describe your layout approach, color scheme, and any interactive elements you plan to include.
```

---

## Quick Prompt Reference

| #   | Prompt            | Output              | Time  |
| --- | ----------------- | ------------------- | ----- |
| 1   | Project Audit     | audit_findings.md   | 4 min |
| 2   | Remediation Plan  | remediation_plan.md | 3 min |
| 3   | Implement Fixes   | Modified src/ files | 5 min |
| 4   | Changelog         | changelog.md        | 2 min |
| 5   | HTML Audit Report | audit_report.html   | 5 min |

**Total estimated time**: 20-25 minutes
