# Tandem Workspace Packs Catalog

This catalog provides an overview of all available Tandem Workspace Packs for the Tandem master repository.

---

## Available Packs

| Pack Name                                                                 | Description                                                          | Complexity            | Time Estimate |
| ------------------------------------------------------------------------- | -------------------------------------------------------------------- | --------------------- | ------------- |
| [Micro-Drama Script Studio Pack](./packs/micro-drama-script-studio-pack/) | Create short-form micro-drama scripts with structured workflows      | Intermediate          | 15-20 min     |
| [Research Synthesis Pack](./packs/research-synthesis-pack/)               | Synthesize research across multiple documents with conflict analysis | Intermediate-Advanced | 20-25 min     |
| [Web Starter Audit Pack](./packs/web-starter-audit-pack/)                 | Audit web projects for accessibility, UX, and code quality           | Beginner-Intermediate | 15-20 min     |
| [Security Playbook Pack](./packs/security-playbook-pack/)                 | Build comprehensive security runbooks for small teams                | Intermediate          | 20-25 min     |

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

## Pack Selection Guide

### By Use Case

| Use Case                 | Recommended Pack          |
| ------------------------ | ------------------------- |
| Content creation/writing | Micro-Drama Script Studio |
| Research and analysis    | Research Synthesis        |
| Code review/quality      | Web Starter Audit         |
| Security/compliance      | Security Playbook         |

### By Complexity Level

| Level                 | Packs                                        |
| --------------------- | -------------------------------------------- |
| Beginner              | Web Starter Audit                            |
| Intermediate          | Micro-Drama Script Studio, Security Playbook |
| Intermediate-Advanced | Research Synthesis                           |
| Advanced              | All packs can scale in depth                 |

### By Time Available

| Time Available | Pack                                         |
| -------------- | -------------------------------------------- |
| 10-15 minutes  | Web Starter Audit, Micro-Drama Script Studio |
| 20-25 minutes  | Research Synthesis, Security Playbook        |

---

## Pack Structure Standard

All packs follow this consistent structure:

```
packs/<PACK_SLUG>/
├── PACK_INFO.md           # Pack metadata and description
├── START_HERE.md          # Step-by-step user instructions
├── PROMPTS.md             # Five copy/paste prompts for Tandem
├── EXPECTED_OUTPUTS.md    # Quality criteria and validation
├── inputs/                # Curated source materials
│   ├── *.md               # Input documents
│   └── papers/            # (optional) Paper collection
├── outputs/               # Generated content
│   └── .gitkeep           # Keep empty outputs directory
└── src/                   # (optional) Source files for audit packs
```

### Required Files per Pack

1. **PACK_INFO.md**: Pack metadata, capabilities demonstrated, use cases, prerequisites
2. **START_HERE.md**: Click-by-click instructions for getting started
3. **PROMPTS.md**: Five sequential prompts (Prompt 5 generates HTML artifact)
4. **EXPECTED_OUTPUTS.md**: Quality criteria and validation checklist

### Optional Files

- **inputs/**: Source materials appropriate to the pack
- **src/**: Source code/files for audit-type packs
- **outputs/**: Empty directory with .gitkeep (runtime outputs only)

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

1. Create directory: `workspace-packs/packs/<PACK_SLUG>/`
2. Create required files: PACK_INFO.md, START_HERE.md, PROMPTS.md, EXPECTED_OUTPUTS.md
3. Add inputs appropriate to the pack type
4. Add outputs/.gitkeep
5. Update this catalog (PACKS.md)

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
