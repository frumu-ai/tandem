# Tandem Prompts - Web Research Refresh Pack

Copy and paste these prompts sequentially into Tandem. Each prompt builds on the previous outputs.

---

## Prompt 1: Stale Claim Identification

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

---

## Prompt 2: Research Plan Creation

```
You are a research project manager. Using the claims inventory from outputs/claims_inventory.md and verification questions from inputs/verification_questions.md, create a research plan document (outputs/research_plan.md).

For each category of claims, provide:

## Research Plan: [Category Name]

### Claims to Verify
- List each claim that needs verification

### Source Strategy
- Identify authoritative sources to check
- Prioritize sources (official docs first)
- Note any source dependencies

### Research Steps
1. Step 1: [What to verify first]
2. Step 2: [What to verify next]
3. ...

### Success Criteria
- How will you know verification is complete?
- What constitutes "sufficient evidence"?

### Timeline Estimate
- Quick lookups (minutes)
- Deep dives (if needed)

Also create a source checklist tracking:
- Source type (official docs, vendor site, etc.)
- URL to verify
- Status (pending, verified, no info)

Save to outputs/research_plan.md
```

---

## Prompt 3: Evidence Gathering with webfetch

```
You are a research specialist gathering evidence from authoritative sources. Using the research plan from outputs/research_plan.md, use webfetch to verify each claim from the claims inventory.

For each claim, perform research and document in outputs/evidence.md:

## Claim: [Brief description]

**Original Claim from stale_brief.md**:
> [Exact quote from original]

**Verification Question**: [From verification_questions.md]

**Research Process**:
- Source 1: [URL]
  - Access Date: [YYYY-MM-DD]
  - Finding: [What the source says]
  - Status: [CONFIRMED / CONTRADICTED / INSUFFICIENT EVIDENCE]

- Source 2: [URL] (if needed)
  - Access Date: [YYYY-MM-DD]
  - Finding: [What the source says]
  - Status: [CONFIRMED / CONTRADICTED / INSUFFICIENT EVIDENCE]

**Updated Understanding**:
[What the correct information is, based on sources]

**Citation**: [1] for first source, [2] for second, etc.

---

## Format for evidence.md:

Use this format for each claim:

### Category: [GitHub Actions / Tauri / Ollama / Tandem]

#### [Specific Claim]

**Original**: [Quote from stale brief]
**Status**: VERIFIED | PARTIALLY VERIFIED | CONTRADICTED | NEEDS MORE RESEARCH

**Sources**:
1. [Title] - [URL] - Accessed [YYYY-MM-DD]
   > [Relevant quote or summary]

2. [Title] - [URL] - Accessed [YYYY-MM-DD]
   > [Relevant quote or summary]

**Corrected Information**:
[Your synthesis of the verified information]

**Impact Assessment**:
- How significant is this change?
- What decisions were affected?

---

Continue for ALL claims. If sources are unavailable, note "NO OFFICIAL SOURCE FOUND" and recommend where to look.

Save progress periodically. When complete, save to outputs/evidence.md
```

---

## Prompt 4: Updated Facts Sheet

```
You are a technical writer creating an updated facts document. Using the evidence log from outputs/evidence.md, create outputs/updated_facts.md.

## Document Structure

### Executive Summary
Brief overview of what was verified and what changed.

### What Changed Since [Original Date]
Table showing:
| Original Claim | Status | Corrected Information | Impact |

### Verified Facts by Category

#### GitHub Actions & Billing
| Fact | Status | Source | Notes |
|------|--------|--------|-------|
| [Fact] | [Status] | [1] | [Notes] |

#### Tauri Platform
| Fact | Status | Source | Notes |
|------|--------|--------|-------|
| [Fact] | [Status] | [1] | [Notes] |

#### Ollama
| Fact | Status | Source | Notes |
|------|--------|--------|-------|
| [Fact] | [Status] | [1] | [Notes] |

#### Tandem Platform
| Fact | Status | Source | Notes |
|------|--------|--------|-------|
| [Fact] | [Status] | [1] | [Notes] |

### Unverifiable Claims
List any claims that could not be verified with current sources.

### Recommendations
- Priority updates for documentation
- Claims that need ongoing monitoring
- Suggested review frequency

Save to outputs/updated_facts.md
```

---

## Prompt 5: Final HTML Report with Citations

````
You are a research communications specialist creating a polished, shareable report. Using updated_facts.md and evidence.md, create a comprehensive HTML report saved to outputs/web_research_report_YYYYMMDD.html (use today's date).

## Report Requirements

### HTML Structure
```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Web Research Report - [Date]</title>
  <style>
    /* Clean, professional styling */
    body { font-family: system-ui, sans-serif; max-width: 900px; margin: 0 auto; padding: 2rem; }
    table { border-collapse: collapse; width: 100%; margin: 1rem 0; }
    th, td { border: 1px solid #ddd; padding: 0.75rem; text-align: left; }
    th { background: #f5f5f5; }
    .citation { color: #0066cc; font-size: 0.9em; }
    .status-verified { color: green; }
    .status-contradicted { color: red; }
    .status-partial { color: orange; }
    .executive-summary { background: #f9f9f9; padding: 1.5rem; border-left: 4px solid #0066cc; margin: 1rem 0; }
  </style>
</head>
<body>
  <!-- Report content here -->
</body>
</html>
````

### Required Sections

1. **Title Header**
   - Report title: "Web Research Report: Product & Platform Facts"
   - Date of report generation
   - Brief description

2. **Executive Summary** (2-3 paragraphs)
   - Overview of research scope
   - Key findings summary
   - Number of claims verified

3. **What Changed Since [Original Date]**
   - Table showing original claims vs. current facts
   - Highlight significant changes
   - Use status colors (green=verified, red=contradicted, orange=partial)

4. **Verified Facts** (organized by category)

   For each category:

   ### [Category Name]

   **Summary**: [Brief overview]

   | Claim   | Status   | Current Understanding |
   | ------- | -------- | --------------------- |
   | [Claim] | [Status] | [Updated fact] [1]    |

5. **Sources Section** (full citations)

   ## Sources

   [1] Title - URL - Accessed YYYY-MM-DD

   > Brief description of what this source provides

   [2] Title - URL - Accessed YYYY-MM-DD

   > Brief description of what this source provides

   (Continue for all sources used)

### Citation Format

- Use inline numeric citations: [1], [2], [3]
- Number sources in order of first appearance
- Map numbers in Sources section to full citations

### Evidence Log Reference

Note: A detailed evidence log is available in outputs/evidence.md with full source URLs and access dates.

### Output

Save as: outputs/web_research_report_YYYYMMDD.html

Before generating, describe your HTML structure and styling approach.

```

---

## Quick Prompt Reference

| # | Prompt | Output | Time |
|---|--------|--------|------|
| 1 | Stale Claim Identification | claims_inventory.md | 2 min |
| 2 | Research Plan | research_plan.md | 2 min |
| 3 | Evidence Gathering | evidence.md | 6 min |
| 4 | Updated Facts | updated_facts.md | 3 min |
| 5 | HTML Report | web_research_report_*.html | 5 min |

**Total estimated time**: 18-20 minutes
```
