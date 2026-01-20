# Tandem Prompts - Research Synthesis Pack

Copy and paste these prompts sequentially into Tandem. Each prompt builds on the previous outputs.

---

## Prompt 1: Workspace Scan & Research Mapping

```
You are a research synthesis specialist. Your task is to scan all files in the inputs/ directory and produce a comprehensive research mapping document.

Read ALL files in inputs/ including:
- All files in inputs/papers/ (10 research papers)
- inputs/questions.md (research questions)
- inputs/glossary.md (terminology)

Create a summary document (outputs/workspace_scan.md) that includes:

## Research Corpus Overview
- Total papers reviewed and their focus areas
- Publication themes and methodology types
- Overall argument direction and consensus areas

## Key Research Questions Addressed
- How each paper addresses questions from questions.md
- Gaps in question coverage
- Papers that directly answer each question

## Terminology Inventory
- Key terms defined in glossary.md
- How terms are used across papers
- Any inconsistencies in terminology usage

## Preliminary Conflict Areas
- Identify papers with opposing viewpoints
- Note statistical disagreements
- Flag contradictory claims for deeper analysis

## Source Quality Assessment
- Methodology strengths and weaknesses
- Potential biases or limitations
- Reliability indicators

Save your summary to outputs/workspace_scan.md and explain your reasoning before writing.
```

---

## Prompt 2: Synthesis Analysis - Themes & Disagreements

```
You are a research synthesis analyst. Using the workspace scan and all source papers, create a comprehensive synthesis analysis document.

## Common Themes Analysis
Identify 4-5 major themes that appear across multiple papers:
- Theme 1: [Name] - Papers addressing it, key findings, consensus level
- Theme 2: [Name] - Papers addressing it, key findings, consensus level
- [Continue for all major themes...]

## Conflict Identification
For each significant disagreement across sources, provide:

### Conflict Area: [Topic]
**Position A**: [Paper name] claims [specific claim with evidence]
**Position B**: [Paper name] claims [opposing claim with evidence]
**Assessment**: [Your analysis of why they disagree and which may be more valid]
**Implications**: [What this disagreement means for practice]

Identify at least 5 distinct conflicts:

1. [Conflict 1]
2. [Conflict 2]
3. [Conflict 3]
4. [Conflict 4]
5. [Conflict 5]

## Evidence Quality Comparison
- Rank papers by evidence quality
- Identify strongest and weakest sources
- Note which claims are well-supported vs. speculative

## Research Gaps
- Questions from questions.md not adequately addressed
- Methodological gaps in current research
- Areas needing further investigation

Save to outputs/synthesis_analysis.md
```

---

## Prompt 3: Claims & Evidence Table

```
You are a systematic review specialist. Create a structured claims and evidence table that organizes all major claims from the research papers.

Create outputs/claims_evidence_table.md with the following structure:

# Claims and Evidence Matrix

## Methodology for this Review
Brief description of how claims were identified, assessed, and categorized.

## Claims by Category

### Category 1: Privacy & Data Sovereignty

| Claim | Evidence | Source(s) | Strength | Consensus |
|-------|----------|-----------|----------|-----------|
| [Specific claim] | [Supporting data] | [Paper #] | High/Medium/Low | Strong/Partial/None |

### Category 2: Security Implications

| Claim | Evidence | Source(s) | Strength | Consensus |
|-------|----------|-----------|----------|-----------|
| [Specific claim] | [Supporting data] | [Paper #] | High/Medium/Low | Strong/Partial/None |

### Category 3: Governance & Compliance

| Claim | Evidence | Source(s) | Strength | Consensus |
|-------|----------|-----------|----------|-----------|
| [Specific claim] | [Supporting data] | [Paper #] | High/Medium/Low | Strong/Partial/None |

### Category 4: Performance & Implementation

| Claim | Evidence | Source(s) | Strength | Consensus |
|-------|----------|-----------|----------|-----------|
| [Specific claim] | [Supporting data] | [Paper #] | High/Medium/Low | Strong/Partial/None |

### Category 5: Cost & Economics

| Claim | Evidence | Source(s) | Strength | Consensus |
|-------|----------|-----------|----------|-----------|
| [Specific claim] | [Supporting data] | [Paper #] |ow | Strong/ High/Medium/LPartial/None |

## Conflicting Claims Summary

| Topic | Claim A | Claim B | Resolution Attempt |
|-------|---------|---------|-------------------|
| [Conflict topic] | [Position with source] | [Opposing position with source] | [Analysis] |

## Key Findings
- Bullet points of most significant, well-supported findings
- Limitations and caveats

Save to outputs/claims_evidence_table.md
```

---

## Prompt 4: Executive Brief (Non-Technical)

```
You are a science communication specialist. Create a one-page executive brief for a non-technical stakeholder (e.g., executive, board member, policy maker) about local-first AI in regulated environments.

## Requirements

### Audience
- No technical background assumed
- Needs to understand trade-offs for decision-making
- Values practical implications over technical details

### Format
Maximum 1 page (approximately 500-700 words)
Use clear headings, bullet points where appropriate
Avoid jargon or explain it when used

### Content Sections

## Executive Summary
2-3 sentences capturing the essence of local-first AI trade-offs.

## What is Local-First AI?
Simple explanation in 2-3 sentences.

## Key Findings

### Privacy Benefits
- Main benefit (1-2 sentences)
- Supporting evidence briefly

### Security Trade-offs
- Main finding (1-2 sentences)
- Key consideration

### Cost Considerations
- Bottom line on costs (1-2 sentences)
- When local-first makes economic sense

### Regulatory Landscape
- Current state (1-2 sentences)
- What to watch for

## Recommendations for Decision-Makers

### When to Consider Local-First AI
- Bullet points of ideal scenarios

### Questions to Ask Your Team
- Practical questions for evaluation

### Risk Considerations
- Key risks to evaluate

## Bottom Line
Clear conclusion in 2-3 sentences.

Save to outputs/executive_brief.md
```

---

## Prompt 5: HTML Research Brief Dashboard

```
Create a comprehensive HTML dashboard artifact that presents the research synthesis in an interactive, visually engaging format.

## Dashboard Sections Required

### 1. Research Overview Header
- Title: "Local-First AI in Regulated Environments: Research Brief"
- Date of synthesis
- Number of papers reviewed
- Key finding highlight

### 2. Executive Summary Card
- Concise summary (3-4 key points)
- Overall assessment (positive/cautious/mixed)
- Confidence level indicator

### 3. Key Themes Visualization
- Display 4-5 major themes with:
  - Theme name
  - Brief description
  - Number of papers addressing it
  - Consensus indicator (high/medium/low)

### 4. Conflict Dashboard
- Present at least 5 key disagreements:
  - Topic
  - Position A summary
  - Position B summary
  - Assessment summary
  - Visual indicator of disagreement strength

### 5. Claims & Evidence Matrix
- Table or grid showing:
  - Category (Privacy, Security, etc.)
  - Key claims
  - Evidence strength color coding
  - Consensus level

### 6. Recommendations Section
- Practical recommendations based on findings
- Audience-appropriate (non-technical)
- Actionable next steps

### 7. Research Gaps
- What's not known
- Areas needing more research
- Caveats

## Styling Requirements
- Clean, professional design
- Use CSS Grid or Flexbox for layout
- Mobile-responsive
- Print-friendly
- Consistent color scheme (suggest: blues and greens for research theme)
- Readable typography
- Visual hierarchy clear

## Technical Requirements
- Single self-contained HTML file (no external dependencies)
- All CSS inline or in <style> block
- No JavaScript required (or inline only)
- Valid HTML5
- Works offline

## Interactivity (Optional but Recommended)
- Expandable/collapsible sections for details
- Hover effects for additional information
- Smooth transitions between sections

Save to: outputs/research_brief_dashboard.html

Before generating, describe your layout approach, color scheme, and any interactive elements you plan to include.
```

---

## Quick Prompt Reference

| #   | Prompt                  | Output                        | Time  |
| --- | ----------------------- | ----------------------------- | ----- |
| 1   | Workspace Scan          | workspace_scan.md             | 3 min |
| 2   | Synthesis Analysis      | synthesis_analysis.md         | 5 min |
| 3   | Claims & Evidence Table | claims_evidence_table.md      | 4 min |
| 4   | Executive Brief         | executive_brief.md            | 3 min |
| 5   | HTML Dashboard          | research_brief_dashboard.html | 5 min |

**Total estimated time**: 20-25 minutes
