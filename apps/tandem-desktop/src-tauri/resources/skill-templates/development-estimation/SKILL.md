---
name: development-estimation
description: Use when estimating time, effort, cost, or complexity for features, projects, refactors, and bug backlogs. Produces defensible estimates via triage, decomposition, risk handling, and confidence intervals, with clear assumptions and validation steps.
version: 1.1.0
tags:
  - estimation
  - planning
  - scope
  - bugs
  - project-review
  - delivery
owner: engineering
last_updated: 2026-02-10
---

# Development Estimation

## Purpose

Create consistent, defensible estimates by:

- Triaging and clarifying scope (especially for bug lists)
- Decomposing work into deliverable components
- Accounting for testing, QA, and release overhead
- Identifying risks and unknowns with explicit mitigation
- Producing confidence-based ranges (P50/P80/P90)

Optimized for reviewing an existing project or backlog and producing a realistic delivery forecast.

## When to Use

- Estimating feature or project scope and delivery timelines
- Reviewing a backlog and producing a time estimate
- Producing risk-aware estimates with confidence intervals
- Converting a list of problems into an execution plan

## Avoid When

- The task is trivial and a gut-check is sufficient
- Requirements are unknown and discovery is the real work
- You lack access to the codebase or requirements

## Inputs Required

Provide as much as possible; estimate quality depends on clarity.

### Minimum

- List of items (bugs/features) with short descriptions
- Target platform/environment (e.g., iOS/Android/Web, staging/prod)
- Constraints (deadline, must-fix vs can-slip)

### Helpful

- Repro steps, logs, screenshots
- Architecture notes or codebase access
- Deployment pipeline and QA process
- Team capacity model (engineers, focus hours/day, meeting load)
- Definition of done (tests required, QA sign-off, release scope)

## Estimation Frames

Always confirm the estimate frame up front:

- Effort: engineer-days or engineer-weeks
- Calendar: derived from capacity (effort / throughput)
- Cost: optional, requires rate assumptions
- Complexity: t-shirt sizing or points (only if requested)

### Default Capacity Model

Unless provided, assume:

- 5–6 hours/day of effective build time per engineer
- Review and coordination overhead included per item
- QA/release overhead is stated explicitly as an assumption

## Triage Rubric (Bug Backlog)

For each bug, capture:

### Repro

- A: Always
- B: Sometimes / flaky
- C: Cannot repro / unclear

### Clarity

- A: Clear cause or clear fix path
- B: Partial (needs investigation)
- C: Unknown (needs discovery)

### Impact

- Blocker / High / Medium / Low

### Fix Type

- UI / Backend / Infra / Data / Dependency / Config / Unknown

### Risk Flags

- Concurrency / migrations / auth/security / payments / third-party API
- Performance / cross-platform behavior / state persistence
- Touches core path / unknown blast radius

Rule: If Repro=C or Clarity=C, include a discovery slice.

## Workflow

### 1) Define Scope and Estimate Type

- Confirm what is in/out of scope
- Confirm estimate type: effort, calendar, cost, complexity
- Confirm confidence target(s): P50 / P80 (default), optionally P90
- Confirm constraints: deadlines, must-fix items, release boundary

### 2) Intake and Normalize the Backlog

- De-duplicate issues
- Merge near-duplicates and note uncertainty
- Group items by subsystem (auth, billing, UI, sync, infra)
- Identify unknown-unknown areas (legacy zones, flaky tests, poor observability)

### 3) Per-Item Breakdown

Estimate each item using this structure:

- Triage / reproduce
- Investigation / diagnosis (if needed)
- Implementation
- Tests (unit/integration/e2e as appropriate)
- Code review and iteration
- QA verification
- Release / deployment steps (if applicable)

Use hours only for work expected to be under one day; otherwise use engineer-days.

### 4) Dependencies and Sequencing

- Identify dependencies (internal modules, data migrations, external APIs, app-store releases)
- Mark blockers and parallelizable streams
- Flag sequencing needs (must land before X)

### 5) Risks, Unknowns, and Buffers

Maintain an Unknowns Register:

- Unknown → discovery task → decision/validation method → timebox

Risk buffer guidance:

- Low risk: +0–10%
- Medium risk: +10–25%
- High risk: +25–50%
- Unknown-heavy: include discovery and a re-estimate gate

### 6) Roll-Up Totals and Confidence

Produce:

- Per-item ranges (min/most-likely/max or P50/P80)
- Totals by bucket (must/should/nice)
- Overall confidence:
  - P50: likely if things go normally
  - P80: realistic commitment range
  - P90: conservative range for high-stakes delivery

### 7) Validation and Re-Estimation Plan

Provide:

- Fastest validation steps to reduce uncertainty (logs, repro harness, spike)
- Re-estimation checkpoints (after discovery spikes, after first tranche)

## Output Format (Required)

### A) Executive Summary

- Scope statement (included/excluded)
- Estimate frame (effort vs calendar + capacity assumptions)
- Total estimates: P50 / P80 (and P90 if requested)
- Key risks and biggest unknowns

### B) Backlog Estimate Table

For each item:

- ID / Title
- Bucket (Must/Should/Nice)
- Subsystem
- Triage scores (Repro/Clarity/Impact)
- Estimate (P50/P80)
- Risk flags
- Notes / assumptions

### C) Totals by Bucket

- Must-fix total (P50/P80)
- Should-fix total (P50/P80)
- Nice-to-have total (P50/P80)

### D) Assumptions and Exclusions

- Explicit assumptions (env access, test maturity, release cadence)
- Explicit exclusions (UX redesign, perf overhaul, refactors unless listed)

### E) Risks and Unknowns Register

- Unknown → discovery plan → timebox → impact if true

### F) Next Steps

- Top 3 actions to validate quickly
- Proposed sequencing / milestones
- Re-estimation trigger points

## Common Mistakes

- Estimating a bug list as if it is already well-specified
- Skipping repro/triage and undercounting diagnosis time
- Ignoring testing, QA verification, and release overhead
- Mixing effort and calendar without a capacity model
- Overstating precision (single-number estimates with no confidence)
- Not separating must-fix from nice-to-have

## Quick Reference

- Reference template: skills/development-estimation/references/estimate.md
- Preferred output: P50/P80 effort plus derived calendar via stated capacity
- Default unit: engineer-days (hours only for sub-day tasks)

## Quality Bar

A good estimate is:

- Transparent about assumptions
- Broken down into components
- Risk-aware with explicit unknown handling
- Delivered as a range with confidence levels
- Paired with a plan to validate and tighten the range
