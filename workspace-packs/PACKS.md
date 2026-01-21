# Tandem Workspace Packs Catalog

This catalog provides an overview of all available Tandem Workspace Packs for the Tandem master repository.

---

## Available Packs

| Pack Name                                                                 | Description                                                          | Complexity            | Time Estimate |
| ------------------------------------------------------------------------- | -------------------------------------------------------------------- | --------------------- | ------------- |
| [Micro-Drama Script Studio Pack](./packs/micro-drama-script-studio-pack/) | Create short-form micro-drama scripts with structured workflows      | Intermediate          | 15-20 min     |
| [Research Synthesis Pack](./packs/research-synthesis-pack/)               | Synthesize research across multiple documents with conflict analysis | Intermediate-Advanced | 20-25 min     |
| [Web Research Refresh Pack](./packs/web-research-refresh-pack/)           | Verify and refresh stale facts using web research and citations      | Beginner-Intermediate | 15-20 min     |
| [Web Starter Audit Pack](./packs/web-starter-audit-pack/)                 | Audit web projects for accessibility, UX, and code quality           | Beginner-Intermediate | 15-20 min     |
| [Security Playbook Pack](./packs/security-playbook-pack/)                 | Build comprehensive security runbooks for small teams                | Intermediate          | 20-25 min     |
| [Legal Research Pack](./packs/legal-research-pack/)                       | Analyze legal contracts and synthesize case notes                    | Intermediate-Advanced | 20-25 min     |

**Note**: Pack documentation (prompts, guides, quality criteria) is located in [`pack-docs/`](./pack-docs/) to keep the pack directories clean and realistic for LLM evaluation.

---

## Pack Overview

### Micro-Drama Script Studio Pack

**Purpose**: Demonstrate Tandem's multi-file context, supervised writes, and HTML artifact generation through micro-drama script creation.

**Key Capabilities Showcased**:

- Multi-file context reading
- Structured output generation
- Iterative workflow progression
- HTML dashboard creation

**Outputs**:

- Episode script (Markdown)
- Writer's Room Dashboard (HTML)
- Episode beats document (Markdown)
- Cast sheet (Markdown)

**Best For**: Content creators, scriptwriters, demonstrating Tandem to stakeholders

---

### Research Synthesis Pack

**Purpose**: Demonstrate cross-document synthesis, conflict detection, and stakeholder communication through research analysis.

**Key Capabilities Showcased**:

- Multi-document synthesis (10+ sources)
- Conflict identification and analysis
- Structured claims/evidence tables
- Executive-level communication
- Interactive HTML dashboards

**Inputs**: 10 research papers, glossary, research questions (all synthetic/original)

**Outputs**:

- Workspace scan summary (Markdown)
- Synthesis analysis (Markdown)
- Claims/evidence table (Markdown)
- Executive brief (Markdown)
- Research Brief Dashboard (HTML)

**Best For**: Researchers, policy analysts, knowledge workers, demonstrating synthesis capabilities

---

### Web Research Refresh Pack

**Purpose**: Demonstrate supervised web research using webfetch to verify and update stale documentation with authoritative sources.

**Key Capabilities Showcased**:

- Stale document analysis and claim identification
- Web research with authoritative source verification
- Evidence logging with URLs and access dates
- Citable report generation with inline citations
- Auditable research workflow documentation

**Inputs**: Stale brief with outdated claims, verification questions for research topics

**Outputs**:

- Claims inventory (Markdown)
- Research plan (Markdown)
- Evidence log with sources (Markdown)
- Updated facts sheet (Markdown)
- Web Research Report (HTML)

**Best For**: Technical writers, documentation teams, fact-checking workflows, demonstrating research methodology

---

### Web Starter Audit Pack

**Purpose**: Demonstrate code auditing, accessibility review, and quality assessment through systematic project analysis.

**Key Capabilities Showcased**:

- Code auditing across multiple languages
- Accessibility compliance checking (WCAG)
- Bug identification and fixes
- Changelog generation
- Professional audit reporting

**Inputs**: HTML, CSS, JavaScript files with intentional issues (accessibility, bugs, code quality)

**Outputs**:

- Audit findings (Markdown)
- Remediation plan (Markdown)
- Changelog (Markdown)
- Project Audit Report (HTML)

**Best For**: Developers, QA engineers, technical leads, demonstrating audit capabilities

---

### Security Playbook Pack

**Purpose**: Demonstrate contextual analysis, structured planning, and compliance documentation through security program development.

**Key Capabilities Showcased**:

- Contextual risk analysis
- Compliance mapping (SOC 2, GDPR, CCPA)
- Prioritized security controls
- Team-specific runbooks
- Professional playbook documentation

**Inputs**: Company context, team profile, threat landscape, compliance requirements (all synthetic)

**Outputs**:

- Security context summary (Markdown)
- Threat assessment (Markdown)
- Priority security checklist (Markdown)
- Team runbook (Markdown)
- Security Playbook Dashboard (HTML)

**Best For**: Security professionals, DevOps leads, small teams, demonstrating security planning

---

### Legal Research Pack

**Purpose**: Demonstrate legal analysis, contract review, and case summarization capabilities.

**Key Capabilities Showcased**:

- Contract risk analysis
- Case law summarization
- Conflict detection in legal documents
- Professional legal memo drafting
- Litigation status visualization

**Inputs**: NDA template, employment agreement draft, case notes (Smith v. Jones)

**Outputs**:

- Contract Risk Matrix (Markdown)
- Case Summary (Markdown)
- Legal Memorandum (Markdown)
- Litigation Dashboard (HTML)

**Best For**: Lawyers, paralegals, legal tech teams, demonstrating document analysis

---

## Pack Selection Guide

### By Use Case

| Use Case                   | Recommended Pack          |
| -------------------------- | ------------------------- |
| Content creation/writing   | Micro-Drama Script Studio |
| Research and analysis      | Research Synthesis        |
| Web research/fact-checking | Web Research Refresh      |
| Code review/quality        | Web Starter Audit         |
| Security/compliance        | Security Playbook         |
| Legal/contracts            | Legal Research Pack       |

### By Complexity Level

| Level                 | Packs                                        |
| --------------------- | -------------------------------------------- |
| Beginner              | Web Starter Audit, Web Research Refresh      |
| Intermediate          | Micro-Drama Script Studio, Security Playbook |
| Intermediate-Advanced | Research Synthesis, Legal Research Pack      |
| Advanced              | All packs can scale in depth                 |

### By Time Available

| Time Available | Pack                                    |
| -------------- | --------------------------------------- |
| 10-15 minutes  | Web Starter Audit, Web Research Refresh      |
| 20-25 minutes  | Research Synthesis, Security Playbook, Legal Research Pack |

---

## Pack Structure Standard

Each pack directory contains **only realistic inputs** - no meta files or documentation. This ensures LLMs evaluate the packs as they would appear in real use:

```
packs/<PACK_SLUG>/
├── inputs/                # Curated source materials (visible to LLM)
│   ├── *.md               # Input documents
│   └── papers/            # (optional) Paper collection
├── outputs/               # Generated content (runtime only)
│   └── .gitkeep           # Keep empty outputs directory
└── src/                   # (optional) Source files for audit packs
```

### Pack Documentation (in pack-docs/)

Metadata and instructions are stored separately to maintain realism:

```
pack-docs/<PACK_SLUG>/
├── PACK_INFO.md           # Pack metadata and description
├── START_HERE.md          # Step-by-step user instructions
├── PROMPTS.md             # Five copy/paste prompts for Tandem
└── EXPECTED_OUTPUTS.md    # Quality criteria and validation
```

### Why Separate Documentation?

Keeping meta files outside the pack directories ensures:

- **Realistic evaluation**: LLMs see only the inputs, not the "answers"
- **Clean user experience**: Users follow prompts from pack-docs/
- **Separation of concerns**: Pack content vs. pack instructions are distinct

---

## Global Constraints

All packs adhere to these standards:

- **Safe to publish**: Synthetic/original content only, no secrets, no personal data, no copyrighted third-party assets
- **Size limit**: Each pack < 15 MB (prefer < 10 MB)
- **HTML artifact**: Prompt 5 must generate an HTML file to `outputs/<name>.html`
- **Consistent prompts**: Exactly 5 prompts per pack

---

## Creating New Packs

To create a new pack:

1. **Create pack directory**: `workspace-packs/packs/<PACK_SLUG>/`
2. **Add realistic inputs**: Create inputs/ with appropriate source materials
3. **Add outputs/.gitkeep**: Create empty outputs directory
4. **Add src/ if needed**: For audit/code packs, add source files
5. **Create pack docs**: Add files to `workspace-packs/pack-docs/<PACK_SLUG>/`
   - PACK_INFO.md
   - START_HERE.md
   - PROMPTS.md
   - EXPECTED_OUTPUTS.md
6. **Update catalog**: Add pack to PACKS.md

### Pack Naming Convention

- Use kebab-case: `<purpose>-pack`
- Example: `research-synthesis-pack`, `web-starter-audit-pack`
- Keep under 40 characters

### Content Guidelines

- All synthetic content (no real company data, no personal information)
- Original text written for the pack
- Realistic but fictional examples
- Safe for public distribution

---

## Version Information

Last Updated: January 2026

Pack Format Version: 1.0

Tandem Compatible: All current versions
