---
name: security-playbook-builder
description: "Transform security context into prioritized controls, response runbooks, and compliance-ready documentation."
version: "0.1.0"
author: tandem
compatibility: tandem
tags:
  - security
  - risk
  - compliance
requires:
  - markdown
triggers:
  - security runbook
  - incident response
  - controls
---

# Security Playbook Skill

## Mission

Create a practical security playbook tailored to team size, threat model, and compliance requirements.

## Inputs To Use First

- inputs/company_context.md
- inputs/team_profile.md
- inputs/threat_landscape.md
- inputs/compliance_requirements.md
- inputs/incident_response_procedures.md

## Workflow

1. Map critical assets and attack surfaces.
2. Identify top threats and likely failure modes.
3. Prioritize controls by risk reduction and implementation cost.
4. Draft incident response runbook with clear ownership.
5. Align controls to compliance requirements.

## Required Outputs

- outputs/security_context_summary.md
- outputs/threat_assessment.md
- outputs/priority_security_checklist.md
- outputs/team_runbook.md
- outputs/security_playbook_dashboard.html

## Quality Bar

- Controls are specific, testable, and owner-assigned.
- Runbook includes detection, triage, containment, recovery.
- Compliance mappings are explicit and auditable.
- Recommendations are realistic for the team profile.
