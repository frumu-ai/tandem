# Migration Plan: Knowledge Work Plugins to Tandem

## 1. Executive Summary

This document outlines the comprehensive migration strategy for transferring the contents of the `knowledge-work-plugins` repository into the Tandem application. The goal is to fully integrate these capabilities as native **Skills** or **Packs**, ensuring brand neutralization (removing Claude/Anthropic references) and technical compatibility with Tandem's architecture (Model-Agnostic, Local-First).

## 2. Analysis of Existing Plugins

The repository contains 9 distinct plugin categories. Each has been analyzed for complexity and dependency requirements.

| Plugin Category        | Key Features                                               | Complexity | Dependencies                | Recommended Target                       |
| ---------------------- | ---------------------------------------------------------- | ---------- | --------------------------- | ---------------------------------------- |
| **Bio-Research**       | Instrument data conversion, Nextflow pipelines, ScVi tools | High       | Python scripts, Local Files | **Pack** (Needs script execution)        |
| **Cowork-Plugin-Mgmt** | MCP server customization                                   | Medium     | MCP Config                  | **Skill** (Instructional)                |
| **Customer Support**   | Ticket triage, Response drafting, Escalation               | Low        | None (Prompt-based)         | **Skill**                                |
| **Data**               | Analysis, Dashboarding, SQL generation                     | Medium     | DB Connection (MCP)         | **Skill** + **Pack** (for viz templates) |
| **Enterprise Search**  | Knowledge synthesis, Search strategy                       | Medium     | Search Integration          | **Skill**                                |
| **Finance**            | Income statements, Reconciliation, SOX testing             | Medium     | ERP Connection (MCP)        | **Skill**                                |
| **Legal**              | NDA triage, Compliance, Contracts                          | Low        | None (Prompt-based)         | **Skill** (Already Migrated)             |
| **Marketing**          | Brand voice, Campaigns, Content                            | Low        | None (Prompt-based)         | **Skill** (Already Migrated)             |
| **Product Management** | (Empty/Placeholder in source?)                             | Low        | TBD                         | **Skill**                                |

## 3. Migration Strategy

### 3.1. Target Architectures

We will use two primary mechanisms in Tandem to host these capabilities:

1.  **Skill Templates (`src-tauri/resources/skill-templates/`)**
    - **Use Case**: Purely prompt-based capabilities, frameworks, and checklists.
    - **Migration Action**: Convert `SKILL.md` and `commands/*.md` into standalone Skill Templates.
    - **Naming Convention**: `<domain>-<capability>` (e.g., `finance-income-statement`).

2.  **Packs (`src-tauri/resources/packs/`)**
    - **Use Case**: Capabilities requiring local scripts, templates, or complex file structures that the user must edit or run.
    - **Migration Action**: Create a Pack directory containing `inputs/` (scripts, templates) and `PROMPTS.md`.
    - **Implementation**: Requires registering the Pack in `src-tauri/src/packs.rs`.

### 3.2. Command Migration

The `commands/` directory in each plugin typically defines specific entry points (e.g., `/analyze`, `/brief`).

- **Recommendation**: Convert each "Command" into a distinct **Skill Template**.
- **Rationale**: Tandem's "Skill" concept is equivalent to a focused agentic workflow. A user selecting the "Income Statement" skill is functionally identical to running `/income-statement`.
- **Action**:
  - Source: `finance/commands/income-statement.md`
  - Destination: `skill-templates/finance-income-statement/SKILL.md`

### 3.3. Brand Neutralization

All migrated content must adhere to the following rules:

- **Replace**: "Claude" -> "AI Assistant" or "Tandem".
- **Replace**: "Anthropic" -> "Tandem" or remove if irrelevant.
- **Replace**: "Plugin" -> "Skill" or "Pack".
- **Remove**: `plugin.json` and `marketplace.json` (not used by Tandem).

## 4. Implementation Roadmap

### Phase 1: Core Business Functions (Finance & Data)

_Focus: High-value, frequent use cases._

- [ ] **Finance**: Migrate `income-statement`, `variance-analysis`, `journal-entry` as Skills.
- [ ] **Data**: Migrate `analyze`, `sql-queries`, `data-visualization` as Skills.
- [ ] **Pack Creation**: Create a `data-visualization-pack` containing the Python visualization templates from `data/skills/data-visualization`.

### Phase 2: Operations & Support (Customer Support & Cowork)

_Focus: Operational efficiency._

- [ ] **Customer Support**: Migrate `triage`, `draft-response`, `escalate` as Skills.
- [ ] **Cowork**: Migrate `plugin-customizer` as `mcp-config-assistant` Skill.

### Phase 3: Specialized Technical Domains (Bio-Research & Enterprise Search)

_Focus: Complex, script-heavy workflows._

- [ ] **Bio-Research**: Create `bio-informatics-pack` containing `instrument-data-to-allotrope` scripts and `nextflow` templates.
- [ ] **Enterprise Search**: Migrate search strategies as Skills.

### Phase 4: Final Polish & Integration

- [ ] **Verification**: Test each Skill/Pack in Tandem.
- [ ] **Documentation**: Update user guides to reference new Skills/Packs.

## 5. Technical Requirements for "Seamless Functionality"

1.  **Local Script Execution**:
    - For `bio-research` and `data` packs, the user must be able to run Python scripts.
    - **Requirement**: Ensure Tandem's `RunCommand` tool is enabled and the user is guided to install dependencies (e.g., `pip install allotropy`).

2.  **MCP Server Connections**:
    - Finance and Data skills rely on data access.
    - **Requirement**: The Skills must include clear "Prerequisites" sections in `SKILL.md` instructing the user to connect relevant MCP servers (e.g., PostgreSQL, Snowflake).

3.  **File Access**:
    - Packs must correctly copy all necessary `references/` and `scripts/` to the user's workspace upon installation.
    - **Requirement**: Update `packs.rs` to include the new Packs and their file mappings.

## 6. Detailed Migration Scope (Files)

### Finance

| Source                           | Type    | Destination                                          | Notes |
| -------------------------------- | ------- | ---------------------------------------------------- | ----- |
| `commands/income-statement.md`   | Command | `skill-templates/finance-income-statement/SKILL.md`  |       |
| `commands/variance-analysis.md`  | Command | `skill-templates/finance-variance-analysis/SKILL.md` |       |
| `skills/reconciliation/SKILL.md` | Skill   | `skill-templates/finance-reconciliation/SKILL.md`    |       |
| `skills/audit-support/SKILL.md`  | Skill   | `skill-templates/finance-audit-support/SKILL.md`     |       |

### Data

| Source                               | Type    | Destination                                 | Notes              |
| ------------------------------------ | ------- | ------------------------------------------- | ------------------ |
| `commands/analyze.md`                | Command | `skill-templates/data-analyze/SKILL.md`     | High priority      |
| `skills/sql-queries/SKILL.md`        | Skill   | `skill-templates/data-sql-queries/SKILL.md` |                    |
| `skills/data-visualization/SKILL.md` | Skill   | `packs/data-viz-pack/`                      | Contains templates |

### Bio-Research

| Source                                  | Type          | Destination                  | Notes             |
| --------------------------------------- | ------------- | ---------------------------- | ----------------- |
| `skills/instrument-data-to-allotrope/*` | Skill+Scripts | `packs/bio-instrument-pack/` | Complex migration |
| `skills/nextflow-development/*`         | Skill+Scripts | `packs/bio-nextflow-pack/`   | Complex migration |

_(Legal and Marketing are completed)_
